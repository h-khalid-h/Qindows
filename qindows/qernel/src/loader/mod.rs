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

        // In production:
        // 1. Calculate number of pages needed: ceil(p_memsz / 4096)
        // 2. Allocate that many frames
        // 3. Map frames at p_vaddr..p_vaddr+p_memsz in Silo page table
        // 4. Copy data[p_offset..p_offset+p_filesz] to p_vaddr
        // 5. Zero p_vaddr+p_filesz..p_vaddr+p_memsz (BSS)

        let pages_needed = (phdr.p_memsz + 4095) / 4096;
        total_memory += pages_needed * 4096;
        segments_mapped += 1;

        // Set page permissions based on p_flags:
        // R → PRESENT
        // W → WRITABLE
        // X → !NO_EXECUTE
    }

    // Allocate user stack (1 MiB)
    let stack_size: u64 = 1024 * 1024;
    let stack_top: u64 = 0x0000_7FFF_FFFF_F000; // Just below canonical hole
    total_memory += stack_size;

    // In production: map stack pages at stack_top - stack_size .. stack_top
    // with a guard page (unmapped, causes page fault on overflow)

    Ok(LoadedBinary {
        entry_point: header.e_entry,
        stack_top,
        segments_mapped,
        memory_used: total_memory,
    })
}
