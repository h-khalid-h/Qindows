//! # Global Descriptor Table (GDT)
//!
//! Defines the CPU segment layout for privilege separation.
//! Qindows uses a flat memory model with 4 segments:
//! - Kernel Code (Ring 0, 64-bit)
//! - Kernel Data (Ring 0)
//! - User Code (Ring 3, 64-bit)
//! - User Data (Ring 3)
//!
//! Plus a Task State Segment (TSS) for stack switching on
//! privilege level transitions (user → kernel syscall).

use core::mem::size_of;

/// GDT segment selectors (byte offsets into the GDT).
pub mod selectors {
    pub const NULL: u16 = 0x00;
    pub const KERNEL_CODE: u16 = 0x08;
    pub const KERNEL_DATA: u16 = 0x10;
    pub const USER_DATA: u16 = 0x18;
    pub const USER_CODE: u16 = 0x20;
    pub const TSS: u16 = 0x28;
}

/// A 64-bit GDT entry.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    granularity: u8,
    base_high: u8,
}

impl GdtEntry {
    /// NULL descriptor
    pub const fn null() -> Self {
        GdtEntry {
            limit_low: 0,
            base_low: 0,
            base_mid: 0,
            access: 0,
            granularity: 0,
            base_high: 0,
        }
    }

    /// Create a code/data segment descriptor.
    ///
    /// # Parameters
    /// - `access`: Access byte (present, privilege, type bits)
    /// - `long_mode`: true for 64-bit code segments
    pub const fn new(access: u8, long_mode: bool) -> Self {
        let granularity = if long_mode {
            0x20 // L bit set (64-bit code)
        } else {
            0x00
        };

        GdtEntry {
            limit_low: 0xFFFF,
            base_low: 0,
            base_mid: 0,
            access,
            granularity: granularity | 0x0F, // Limit[19:16] + flags
            base_high: 0,
        }
    }
}

/// Task State Segment (TSS) — used for privilege-level stack switching.
///
/// When a user-space Silo makes a syscall, the CPU automatically
/// switches to the kernel stack specified in RSP0.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct TaskStateSegment {
    _reserved_0: u32,
    /// Stack pointer for Ring 0 — loaded on privilege escalation
    pub rsp0: u64,
    /// Stack pointer for Ring 1 (unused in Qindows)
    pub rsp1: u64,
    /// Stack pointer for Ring 2 (unused in Qindows)
    pub rsp2: u64,
    _reserved_1: u64,
    /// Interrupt Stack Table — dedicated stacks for specific exceptions
    pub ist: [u64; 7],
    _reserved_2: u64,
    _reserved_3: u16,
    /// I/O permission bitmap offset
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        TaskStateSegment {
            _reserved_0: 0,
            rsp0: 0,
            rsp1: 0,
            rsp2: 0,
            _reserved_1: 0,
            ist: [0; 7],
            _reserved_2: 0,
            _reserved_3: 0,
            iomap_base: size_of::<Self>() as u16,
        }
    }
}

/// TSS descriptor (16 bytes — occupies 2 GDT slots).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct TssDescriptor {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    limit_flags: u8,
    base_mid2: u8,
    base_high: u32,
    _reserved: u32,
}

impl TssDescriptor {
    /// Create a TSS descriptor pointing to the given TSS.
    pub fn new(tss: &TaskStateSegment) -> Self {
        let base = tss as *const _ as u64;
        let limit = (size_of::<TaskStateSegment>() - 1) as u64;

        TssDescriptor {
            limit_low: limit as u16,
            base_low: base as u16,
            base_mid: (base >> 16) as u8,
            access: 0x89, // Present, 64-bit TSS (available)
            limit_flags: ((limit >> 16) as u8) & 0x0F,
            base_mid2: (base >> 24) as u8,
            base_high: (base >> 32) as u32,
            _reserved: 0,
        }
    }
}

/// The full GDT structure.
///
/// Layout:
/// [0] NULL
/// [1] Kernel Code (0x08) — Ring 0, 64-bit, execute/read
/// [2] Kernel Data (0x10) — Ring 0, read/write
/// [3] User Data   (0x18) — Ring 3, read/write
/// [4] User Code   (0x20) — Ring 3, 64-bit, execute/read
/// [5-6] TSS       (0x28) — 16-byte TSS descriptor
#[repr(C, packed)]
pub struct Gdt {
    pub null: GdtEntry,
    pub kernel_code: GdtEntry,
    pub kernel_data: GdtEntry,
    pub user_data: GdtEntry,
    pub user_code: GdtEntry,
    pub tss: TssDescriptor,
}

/// GDT pointer for LGDT instruction.
#[repr(C, packed)]
pub struct GdtPointer {
    pub limit: u16,
    pub base: u64,
}

// Static GDT and TSS
static mut TSS: TaskStateSegment = TaskStateSegment::new();
static mut GDT: Gdt = Gdt {
    null: GdtEntry::null(),
    // Kernel Code: Present | DPL 0 | Code | Execute/Read
    kernel_code: GdtEntry::new(0x9A, true),
    // Kernel Data: Present | DPL 0 | Data | Read/Write
    kernel_data: GdtEntry::new(0x92, false),
    // User Data: Present | DPL 3 | Data | Read/Write
    user_data: GdtEntry::new(0xF2, false),
    // User Code: Present | DPL 3 | Code | Execute/Read
    user_code: GdtEntry::new(0xFA, true),
    // TSS placeholder (will be filled in init)
    tss: TssDescriptor {
        limit_low: 0,
        base_low: 0,
        base_mid: 0,
        access: 0,
        limit_flags: 0,
        base_mid2: 0,
        base_high: 0,
        _reserved: 0,
    },
};

/// Kernel stack for the TSS (8 KiB).
/// When the CPU switches from Ring 3 to Ring 0, it loads RSP from here.
static mut KERNEL_STACK: [u8; 8192] = [0; 8192];

/// Double-fault stack (separate to avoid triple faults).
static mut DOUBLE_FAULT_STACK: [u8; 4096] = [0; 4096];

/// Initialize the GDT and TSS.
///
/// Must be called before `interrupts::init()`.
pub fn init() {
    unsafe {
        // Configure TSS
        TSS.rsp0 = (&KERNEL_STACK as *const _ as u64) + 8192;
        TSS.ist[0] = (&DOUBLE_FAULT_STACK as *const _ as u64) + 4096;

        // Write TSS descriptor into GDT
        GDT.tss = TssDescriptor::new(&TSS);

        // Load GDT
        let gdt_ptr = GdtPointer {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: &GDT as *const _ as u64,
        };

        core::arch::asm!(
            "lgdt [{}]",
            in(reg) &gdt_ptr,
            options(readonly, nostack)
        );

        // Reload segment registers
        core::arch::asm!(
            // Set data segments to kernel data
            "mov ax, 0x10",         // KERNEL_DATA selector
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            // Far return to reload CS with kernel code selector
            "push 0x08",           // KERNEL_CODE selector
            "lea rax, [rip + 2f]", // Push return address
            "push rax",
            "retfq",              // Far return
            "2:",
            out("rax") _,
            options(preserves_flags)
        );

        // Load TSS
        core::arch::asm!(
            "ltr {0:x}",
            in(reg) selectors::TSS,
            options(nostack, preserves_flags)
        );
    }

    crate::serial_println!("[OK] GDT loaded (Kernel Ring-0 / User Ring-3 / TSS)");
}
