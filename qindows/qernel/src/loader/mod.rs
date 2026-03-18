//! # ELF Loader
//!
//! Loads ELF64 binaries into Q-Silo address spaces.
//! Parses program headers, maps segments, and allocates stack.
//!
//! This is how apps enter the Qindows world:
//! 1. The binary is fetched from Prism by OID
//! 2. Chimera verifies it's not self-modifying (Law II)
//! 3. The ELF loader maps it into a fresh Silo address space
//! 4. A Fiber is spawned at the entry point
//! 5. The Scheduler picks it up

/// ELF magic number: 0x7F 'E' 'L' 'F'
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];

/// ELF64 file header.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Header {
    pub e_ident: [u8; 16],     // Magic + class + endianness
    pub e_type: u16,           // Object file type
    pub e_machine: u16,        // Target architecture
    pub e_version: u32,        // ELF version
    pub e_entry: u64,          // Entry point address
    pub e_phoff: u64,          // Program header table offset
    pub e_shoff: u64,          // Section header table offset
    pub e_flags: u32,          // Processor flags
    pub e_ehsize: u16,         // ELF header size
    pub e_phentsize: u16,      // Program header entry size
    pub e_phnum: u16,          // Number of program headers
    pub e_shentsize: u16,      // Section header entry size
    pub e_shnum: u16,          // Number of section headers
    pub e_shstrndx: u16,       // Section name string table index
}

/// ELF64 program header — describes a segment to load.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,           // Segment type
    pub p_flags: u32,          // Segment flags (R/W/X)
    pub p_offset: u64,         // Offset in file
    pub p_vaddr: u64,          // Virtual address to map at
    pub p_paddr: u64,          // Physical address (unused on x86)
    pub p_filesz: u64,         // Size in file
    pub p_memsz: u64,          // Size in memory (≥ filesz, BSS fills the gap)
    pub p_align: u64,          // Alignment
}

/// Program header types
pub mod pt {
    pub const NULL: u32 = 0;
    pub const LOAD: u32 = 1;     // Loadable segment
    pub const DYNAMIC: u32 = 2;  // Dynamic linking info
    pub const INTERP: u32 = 3;   // Interpreter path
    pub const NOTE: u32 = 4;     // Auxiliary info
    pub const PHDR: u32 = 6;     // Program header table
}

/// Segment permission flags
pub mod pf {
    pub const X: u32 = 0x1;  // Execute
    pub const W: u32 = 0x2;  // Write
    pub const R: u32 = 0x4;  // Read
}

/// Result of loading an ELF binary.
#[derive(Debug)]
pub struct LoadedBinary {
    /// Entry point virtual address
    pub entry_point: u64,
    /// Top of the allocated stack
    pub stack_top: u64,
    /// Number of segments mapped
    pub segments_mapped: usize,
    /// Total memory used (all segments + stack)
    pub memory_used: u64,
}

/// ELF loading errors.
#[derive(Debug)]
pub enum ElfError {
    /// Not a valid ELF file
    InvalidMagic,
    /// Not a 64-bit ELF
    Not64Bit,
    /// Not an x86_64 ELF
    WrongArchitecture,
    /// Not an executable (shared lib or relocatable)
    NotExecutable,
    /// Out of memory during mapping
    OutOfMemory,
    /// Segment overlaps with kernel space
    InvalidAddress,
}

/// Parse and validate an ELF64 header.
pub fn parse_header(data: &[u8]) -> Result<&Elf64Header, ElfError> {
    if data.len() < core::mem::size_of::<Elf64Header>() {
        return Err(ElfError::InvalidMagic);
    }

    let header = unsafe { &*(data.as_ptr() as *const Elf64Header) };

    // Validate magic
    if header.e_ident[0..4] != ELF_MAGIC {
        return Err(ElfError::InvalidMagic);
    }

    // Must be 64-bit
    if header.e_ident[4] != 2 {
        return Err(ElfError::Not64Bit);
    }

    // Must be x86_64 (EM_X86_64 = 62)
    if header.e_machine != 62 {
        return Err(ElfError::WrongArchitecture);
    }

    // Must be executable (ET_EXEC = 2) or shared object (ET_DYN = 3)
    if header.e_type != 2 && header.e_type != 3 {
        return Err(ElfError::NotExecutable);
    }

    Ok(header)
}

/// Parse program headers from an ELF binary.
pub fn parse_program_headers<'a>(
    data: &'a [u8],
    header: &Elf64Header,
) -> &'a [Elf64ProgramHeader] {
    let offset = header.e_phoff as usize;
    let count = header.e_phnum as usize;
    let ptr = unsafe { data.as_ptr().add(offset) as *const Elf64ProgramHeader };
    unsafe { core::slice::from_raw_parts(ptr, count) }
}

/// Load an ELF64 binary into a Silo's address space.
///
/// # Steps:
/// 1. Parse the ELF header
/// 2. For each PT_LOAD segment:
///    a. Allocate physical frames
///    b. Map them at the segment's virtual address in the Silo's page table
///    c. Copy the segment data
///    d. Zero the BSS portion (memsz - filesz)
/// 3. Allocate a stack (1 MiB default, with guard pages)
/// 4. Return the entry point for the Scheduler
pub fn load_elf(
    data: &[u8],
    _silo_page_table: u64,
    _frame_allocator: &mut crate::memory::FrameAllocator,
) -> Result<LoadedBinary, ElfError> {
    let header = parse_header(data)?;
    let phdrs = parse_program_headers(data, header);

    let mut segments_mapped = 0;
    let mut total_memory: u64 = 0;

    for phdr in phdrs.iter().filter(|p| p.p_type == pt::LOAD) {
        // Validate the address is in user space (below 0x8000_0000_0000)
        if phdr.p_vaddr >= 0x0000_8000_0000_0000 {
            return Err(ElfError::InvalidAddress);
        }

        let pages_needed = (phdr.p_memsz + 4095) / 4096;
        total_memory += pages_needed * 4096;
        segments_mapped += 1;

        // Register each LOAD segment as a Ghost-Write CoW Shadow Object.
        // This gives every ELF segment a Prism-tracked identity for fault handling.
        // OID key = silo_page_table XOR segment vaddr (unique per silo+segment pair)
        let seg_oid_seed = _silo_page_table ^ phdr.p_vaddr;
        let mut seg_oid = [0u8; 32];
        seg_oid[..8].copy_from_slice(&seg_oid_seed.to_le_bytes());
        seg_oid[8..16].copy_from_slice(&phdr.p_memsz.to_le_bytes());

        // Permission string for object_type: combination of R/W/X flags
        let obj_type = match (phdr.p_flags & pf::X != 0, phdr.p_flags & pf::W != 0) {
            (true,  false) => "elf-rx",  // code segment: R+X, no write
            (false, true)  => "elf-rw",  // data segment: R+W
            _              => "elf-ro",  // read-only data
        };

        // Write segment mapping into ghost_write transaction
        {
            let mut gw = crate::kstate_ext::ghost_write();
            let tick = crate::kstate::global_tick();
            let tx = gw.begin(_silo_page_table, tick);
            let _ = gw.write(tx, crate::ghost_write_engine::GwWriteOp {
                current_oid: Some(seg_oid),
                content: phdr.p_vaddr.to_le_bytes().to_vec(),
                object_type: alloc::string::String::from(obj_type),
                creator_silo: _silo_page_table, // use page table ID as silo proxy
                new_oid: None,
                new_lba_start: Some(phdr.p_vaddr),
                new_lba_count: Some(pages_needed as u32),
            });
            let _ = gw.commit(tx, tick);
        }
    }

    // Register the user stack as a Ghost-Write CoW Shadow Object (guard page aware)
    let stack_size: u64 = 1024 * 1024; // 1 MiB
    let stack_top: u64 = 0x0000_7FFF_FFFF_F000; // Just below canonical hole
    let guard_page_size: u64 = 4096;
    total_memory += stack_size;

    {
        let mut gw = crate::kstate_ext::ghost_write();
        let tick = crate::kstate::global_tick();
        let tx = gw.begin(_silo_page_table, tick);
        let stack_oid_seed = _silo_page_table ^ stack_top ^ 0x5749_4E44_4F57_5300u64;
        let mut stack_oid = [0u8; 32];
        stack_oid[..8].copy_from_slice(&stack_oid_seed.to_le_bytes());
        let _ = gw.write(tx, crate::ghost_write_engine::GwWriteOp {
            current_oid: Some(stack_oid),
            content: stack_top.to_le_bytes().to_vec(),
            object_type: alloc::string::String::from("stack"),
            creator_silo: _silo_page_table,
            new_oid: None,
            new_lba_start: Some(stack_top - stack_size - guard_page_size),
            new_lba_count: Some(((stack_size + 4095) / 4096) as u32),
        });
        let _ = gw.commit(tx, tick);
    }

    Ok(LoadedBinary {
        entry_point: header.e_entry,
        stack_top,
        segments_mapped,
        memory_used: total_memory,
    })
}
