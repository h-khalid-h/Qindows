//! # Chimera PE Loader
//!
//! Loads Win32/64 PE (Portable Executable) binaries into Chimera Silos.
//! Parses DOS/PE headers, maps sections, resolves imports via
//! the Virtual DLL Table, and patches the IAT.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// DOS Header magic: "MZ"
pub const DOS_MAGIC: u16 = 0x5A4D;
/// PE signature: "PE\0\0"
pub const PE_MAGIC: u32 = 0x0000_4550;

/// DOS Header — the first thing in every .exe file.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct DosHeader {
    pub e_magic: u16,
    pub _padding: [u8; 58],
    /// Offset to PE header
    pub e_lfanew: u32,
}

/// COFF File Header (after PE signature).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct CoffHeader {
    pub machine: u16,
    pub number_of_sections: u16,
    pub time_date_stamp: u32,
    pub pointer_to_symbol_table: u32,
    pub number_of_symbols: u32,
    pub size_of_optional_header: u16,
    pub characteristics: u16,
}

/// PE Optional Header (64-bit).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct OptionalHeader64 {
    pub magic: u16,
    pub major_linker_version: u8,
    pub minor_linker_version: u8,
    pub size_of_code: u32,
    pub size_of_initialized_data: u32,
    pub size_of_uninitialized_data: u32,
    pub address_of_entry_point: u32,
    pub base_of_code: u32,
    pub image_base: u64,
    pub section_alignment: u32,
    pub file_alignment: u32,
    pub major_os_version: u16,
    pub minor_os_version: u16,
    pub major_image_version: u16,
    pub minor_image_version: u16,
    pub major_subsystem_version: u16,
    pub minor_subsystem_version: u16,
    pub win32_version_value: u32,
    pub size_of_image: u32,
    pub size_of_headers: u32,
    pub checksum: u32,
    pub subsystem: u16,
    pub dll_characteristics: u16,
    pub size_of_stack_reserve: u64,
    pub size_of_stack_commit: u64,
    pub size_of_heap_reserve: u64,
    pub size_of_heap_commit: u64,
    pub loader_flags: u32,
    pub number_of_rva_and_sizes: u32,
}

/// PE Section Header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SectionHeader {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: u32,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: u32,
    pub pointer_to_relocations: u32,
    pub pointer_to_linenumbers: u32,
    pub number_of_relocations: u16,
    pub number_of_linenumbers: u16,
    pub characteristics: u32,
}

/// Section characteristics flags
pub mod section_flags {
    pub const CODE: u32 = 0x00000020;
    pub const INITIALIZED_DATA: u32 = 0x00000040;
    pub const UNINITIALIZED_DATA: u32 = 0x00000080;
    pub const MEM_EXECUTE: u32 = 0x20000000;
    pub const MEM_READ: u32 = 0x40000000;
    pub const MEM_WRITE: u32 = 0x80000000;
}

/// Loaded PE result.
#[derive(Debug)]
pub struct LoadedPe {
    /// Entry point RVA
    pub entry_point: u64,
    /// Image base (preferred or relocated)
    pub image_base: u64,
    /// Sections mapped
    pub sections: Vec<MappedSection>,
    /// Required DLLs
    pub required_dlls: Vec<String>,
    /// Total memory used
    pub memory_used: u64,
}

/// A mapped PE section.
#[derive(Debug)]
pub struct MappedSection {
    pub name: String,
    pub virtual_address: u64,
    pub size: u64,
    pub is_executable: bool,
    pub is_writable: bool,
}

/// PE loading errors
#[derive(Debug)]
pub enum PeError {
    InvalidDosHeader,
    InvalidPeSignature,
    Not64Bit,
    UnsupportedMachine,
    OutOfMemory,
}

/// Parse a PE64 binary.
pub fn parse_pe(data: &[u8]) -> Result<LoadedPe, PeError> {
    if data.len() < core::mem::size_of::<DosHeader>() {
        return Err(PeError::InvalidDosHeader);
    }

    // Parse DOS header
    let dos = unsafe { &*(data.as_ptr() as *const DosHeader) };
    if dos.e_magic != DOS_MAGIC {
        return Err(PeError::InvalidDosHeader);
    }

    let pe_offset = dos.e_lfanew as usize;

    // Validate PE signature
    if data.len() < pe_offset + 4 {
        return Err(PeError::InvalidPeSignature);
    }
    let pe_sig = u32::from_le_bytes(data[pe_offset..pe_offset + 4].try_into().unwrap_or([0; 4]));
    if pe_sig != PE_MAGIC {
        return Err(PeError::InvalidPeSignature);
    }

    // Parse COFF header
    let coff_offset = pe_offset + 4;
    let coff = unsafe { &*(data[coff_offset..].as_ptr() as *const CoffHeader) };

    // Must be x86_64 (0x8664)
    if coff.machine != 0x8664 {
        return Err(PeError::UnsupportedMachine);
    }

    // Parse Optional Header
    let opt_offset = coff_offset + core::mem::size_of::<CoffHeader>();
    let opt = unsafe { &*(data[opt_offset..].as_ptr() as *const OptionalHeader64) };

    // Must be PE32+ (0x20b)
    if opt.magic != 0x020B {
        return Err(PeError::Not64Bit);
    }

    // Parse sections
    let sections_offset = opt_offset + coff.size_of_optional_header as usize;
    let num_sections = coff.number_of_sections as usize;
    let mut mapped_sections = Vec::new();
    let mut total_memory: u64 = 0;

    for i in 0..num_sections {
        let sec_ptr = sections_offset + i * core::mem::size_of::<SectionHeader>();
        if sec_ptr + core::mem::size_of::<SectionHeader>() > data.len() {
            break;
        }
        let section = unsafe { &*(data[sec_ptr..].as_ptr() as *const SectionHeader) };

        let name_bytes = &section.name;
        let name_end = name_bytes.iter().position(|&b| b == 0).unwrap_or(8);
        let name = String::from_utf8_lossy(&name_bytes[..name_end]).into_owned();

        let size = if section.virtual_size > 0 {
            section.virtual_size as u64
        } else {
            section.size_of_raw_data as u64
        };

        mapped_sections.push(MappedSection {
            name,
            virtual_address: opt.image_base + section.virtual_address as u64,
            size,
            is_executable: section.characteristics & section_flags::MEM_EXECUTE != 0,
            is_writable: section.characteristics & section_flags::MEM_WRITE != 0,
        });

        total_memory += (size + 4095) / 4096 * 4096; // Align to pages
    }

    Ok(LoadedPe {
        entry_point: opt.image_base + opt.address_of_entry_point as u64,
        image_base: opt.image_base,
        sections: mapped_sections,
        required_dlls: Vec::new(), // Would be filled by import table parsing
        memory_used: total_memory,
    })
}
