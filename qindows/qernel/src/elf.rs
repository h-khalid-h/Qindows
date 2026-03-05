//! # Qernel ELF Binary Loader
//!
//! Parses and loads ELF64 binaries into Silo address spaces.
//! Handles program headers, relocations, and entry point setup.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// ELF magic number.
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF class (32 or 64 bit).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfClass {
    Elf32 = 1,
    Elf64 = 2,
}

/// ELF object type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElfType {
    None,
    Relocatable,
    Executable,
    SharedObject,
    Core,
    Unknown(u16),
}

impl ElfType {
    fn from_u16(v: u16) -> Self {
        match v {
            0 => ElfType::None,
            1 => ElfType::Relocatable,
            2 => ElfType::Executable,
            3 => ElfType::SharedObject,
            4 => ElfType::Core,
            x => ElfType::Unknown(x),
        }
    }
}

/// ELF64 file header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Header {
    pub magic: [u8; 4],
    pub class: u8,
    pub data: u8,       // 1=LE, 2=BE
    pub version: u8,
    pub os_abi: u8,
    pub abi_version: u8,
    pub _pad: [u8; 7],
    pub elf_type: u16,
    pub machine: u16,   // 0x3E = x86-64
    pub version2: u32,
    pub entry: u64,
    pub ph_offset: u64,
    pub sh_offset: u64,
    pub flags: u32,
    pub eh_size: u16,
    pub ph_entry_size: u16,
    pub ph_count: u16,
    pub sh_entry_size: u16,
    pub sh_count: u16,
    pub sh_str_index: u16,
}

/// Program header types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhType {
    Null,
    Load,
    Dynamic,
    Interp,
    Note,
    Phdr,
    Tls,
    Unknown(u32),
}

impl PhType {
    fn from_u32(v: u32) -> Self {
        match v {
            0 => PhType::Null,
            1 => PhType::Load,
            2 => PhType::Dynamic,
            3 => PhType::Interp,
            4 => PhType::Note,
            6 => PhType::Phdr,
            7 => PhType::Tls,
            x => PhType::Unknown(x),
        }
    }
}

/// ELF64 program header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Phdr {
    pub p_type: u32,
    pub p_flags: u32,   // PF_X=1, PF_W=2, PF_R=4
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

/// ELF64 section header.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64Shdr {
    pub sh_name: u32,
    pub sh_type: u32,
    pub sh_flags: u64,
    pub sh_addr: u64,
    pub sh_offset: u64,
    pub sh_size: u64,
    pub sh_link: u32,
    pub sh_info: u32,
    pub sh_addralign: u64,
    pub sh_entsize: u64,
}

/// Memory protection flags.
#[derive(Debug, Clone, Copy)]
pub struct MemProt {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl MemProt {
    pub fn from_elf_flags(flags: u32) -> Self {
        MemProt {
            execute: flags & 1 != 0,
            write: flags & 2 != 0,
            read: flags & 4 != 0,
        }
    }
}

/// A loadable segment (parsed from program headers).
#[derive(Debug, Clone)]
pub struct Segment {
    pub seg_type: PhType,
    pub vaddr: u64,
    pub offset: u64,
    pub file_size: u64,
    pub mem_size: u64,
    pub alignment: u64,
    pub prot: MemProt,
}

/// Parsed ELF binary information.
#[derive(Debug, Clone)]
pub struct ElfInfo {
    pub elf_type: ElfType,
    pub machine: u16,
    pub entry_point: u64,
    pub segments: Vec<Segment>,
    pub is_pie: bool,     // Position-independent executable
    pub has_tls: bool,    // Thread-local storage
    pub interp: Option<String>, // Dynamic linker path
}

/// ELF loading errors.
#[derive(Debug, Clone)]
pub enum ElfError {
    InvalidMagic,
    Not64Bit,
    NotLittleEndian,
    NotX86_64,
    NoLoadableSegments,
    SegmentOverlap,
    TooLarge,
    InvalidAlignment,
}

/// The ELF Loader.
pub struct ElfLoader {
    /// Maximum binary size (bytes)
    pub max_binary_size: usize,
    /// Maximum virtual address range
    pub max_vaddr_range: u64,
    /// Stats
    pub binaries_loaded: u64,
    pub segments_mapped: u64,
    pub total_bytes_loaded: u64,
    pub load_errors: u64,
}

impl ElfLoader {
    pub fn new() -> Self {
        ElfLoader {
            max_binary_size: 256 * 1024 * 1024, // 256 MiB
            max_vaddr_range: 0x0000_8000_0000_0000, // User-space limit
            binaries_loaded: 0,
            segments_mapped: 0,
            total_bytes_loaded: 0,
            load_errors: 0,
        }
    }

    /// Parse an ELF binary from raw bytes.
    pub fn parse(&mut self, data: &[u8]) -> Result<ElfInfo, ElfError> {
        if data.len() < core::mem::size_of::<Elf64Header>() {
            self.load_errors += 1;
            return Err(ElfError::InvalidMagic);
        }

        // Validate header
        let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

        if header.magic != ELF_MAGIC {
            self.load_errors += 1;
            return Err(ElfError::InvalidMagic);
        }
        if header.class != 2 {
            self.load_errors += 1;
            return Err(ElfError::Not64Bit);
        }
        if header.data != 1 {
            self.load_errors += 1;
            return Err(ElfError::NotLittleEndian);
        }
        if header.machine != 0x3E {
            self.load_errors += 1;
            return Err(ElfError::NotX86_64);
        }
        if data.len() > self.max_binary_size {
            self.load_errors += 1;
            return Err(ElfError::TooLarge);
        }

        // Parse program headers
        let mut segments = Vec::new();
        let mut interp = None;
        let mut has_tls = false;
        let is_pie = header.elf_type == 3; // ET_DYN

        let ph_offset = header.ph_offset as usize;
        let ph_size = header.ph_entry_size as usize;

        for i in 0..header.ph_count as usize {
            let offset = ph_offset + i * ph_size;
            if offset + ph_size > data.len() { break; }

            let phdr = unsafe { &*(data.as_ptr().add(offset) as *const Elf64Phdr) };
            let seg_type = PhType::from_u32(phdr.p_type);

            match seg_type {
                PhType::Load => {
                    if phdr.p_vaddr >= self.max_vaddr_range {
                        self.load_errors += 1;
                        return Err(ElfError::TooLarge);
                    }

                    segments.push(Segment {
                        seg_type,
                        vaddr: phdr.p_vaddr,
                        offset: phdr.p_offset,
                        file_size: phdr.p_filesz,
                        mem_size: phdr.p_memsz,
                        alignment: phdr.p_align,
                        prot: MemProt::from_elf_flags(phdr.p_flags),
                    });
                }
                PhType::Interp => {
                    let start = phdr.p_offset as usize;
                    let end = start + phdr.p_filesz as usize;
                    if end <= data.len() {
                        let bytes = &data[start..end];
                        // Strip null terminator
                        let len = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
                        if let Ok(s) = core::str::from_utf8(&bytes[..len]) {
                            interp = Some(String::from(s));
                        }
                    }
                }
                PhType::Tls => { has_tls = true; }
                _ => {}
            }
        }

        if segments.is_empty() {
            self.load_errors += 1;
            return Err(ElfError::NoLoadableSegments);
        }

        // Check for segment overlaps
        for i in 0..segments.len() {
            for j in (i + 1)..segments.len() {
                let a = &segments[i];
                let b = &segments[j];
                let a_end = a.vaddr + a.mem_size;
                let b_end = b.vaddr + b.mem_size;
                if a.vaddr < b_end && b.vaddr < a_end {
                    self.load_errors += 1;
                    return Err(ElfError::SegmentOverlap);
                }
            }
        }

        let total: u64 = segments.iter().map(|s| s.mem_size).sum();
        self.total_bytes_loaded += total;
        self.segments_mapped += segments.len() as u64;
        self.binaries_loaded += 1;

        Ok(ElfInfo {
            elf_type: ElfType::from_u16(header.elf_type),
            machine: header.machine,
            entry_point: header.entry,
            segments,
            is_pie,
            has_tls,
            interp,
        })
    }
}
