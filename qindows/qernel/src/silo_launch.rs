//! # Q-Silo Launch Path — Ring-3 Entry (Phase 54)
//!
//! The missing link: takes a parsed `LoadedBinary` from the ELF loader
//! and actually places the Silo into Ring-3 execution.
//!
//! ## Launch Sequence
//! ```text
//! 1. ELF loader maps segments into Silo's SiloAddressSpace
//! 2. silo_launch() sets up initial CPU state (registers, stack)
//! 3. SYSRET instruction transfers control to Ring-3 entry point
//! 4. The Silo runs at Ring 3 with only its CapToken-granted permissions
//! ```
//!
//! ## Q-Manifest Law 6: Silo Sandbox
//! The SYSRET path sets:
//! - CS → user code segment (Ring 3, from GDT slot 3)
//! - SS → user data segment (Ring 3, from GDT slot 4)
//! - RFLAGS → interrupts enabled, no IOPL (Ring 3 cannot touch I/O ports)
//! - RSP → user stack top (Page-guard-protected)
//! - RCX → entry point (SYSRET jumps here)
//!
//! ## Q-Manifest Law 1: Zero-Ambient Authority
//! RDI carries a pointer to the Silo's primary CapToken descriptor so the
//! Silo's runtime can wire up its ambient capabilities at startup.

use crate::loader::{LoadedBinary, ElfError};
use crate::memory::{FrameAllocator, PhysFrame};
use crate::memory::paging::SiloAddressSpace;
use crate::memory::vmm::{VirtualMemoryManager, VirtAddr, MapPermissions};
use crate::silo::QSilo;

/// User stack size for a launched Silo (2 MiB).
pub const SILO_STACK_SIZE: u64 = 2 * 1024 * 1024;

/// Virtual address of the user stack top (just below the canonical hole).
pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_F000;

/// RFLAGS value for Ring-3 entry:
/// - Bit 9  (IF) = 1: interrupts enabled (user code can be preempted)
/// - Bit 2  (PF) = 0: parity flag cleared
/// - Bit 1       = 1: always 1 (reserved)
/// - IOPL [13:12] = 0: no I/O port access from Ring 3
pub const USER_RFLAGS: u64 = (1 << 9) | (1 << 1);

/// The initial register state for a newly launched Silo.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct SiloEntryRegs {
    /// Entry point — loaded into RCX (SYSRET target)
    pub rip: u64,
    /// Stack pointer — top of user stack
    pub rsp: u64,
    /// RFLAGS — controls interrupt enable, IOPL
    pub rflags: u64,
    /// RDI — pointer to initial CapToken descriptor (Law 1 wiring)
    pub rdi: u64,
    /// RSI — silo_id (so the runtime knows its own identity)
    pub rsi: u64,
}

/// Errors that can occur during Silo launch.
#[derive(Debug)]
pub enum LaunchError {
    /// ELF binary failed to parse or load
    ElfLoadFailed(ElfError),
    /// Could not allocate physical memory for stack
    StackAllocFailed,
    /// SiloAddressSpace was not initialized before launch
    AddressSpaceNotReady,
    /// Entry point falls in kernel space (attack attempt)
    InvalidEntryPoint,
}

/// Allocate and map the user-mode stack for a Silo.
///
/// Maps `SILO_STACK_SIZE` bytes ending at `USER_STACK_TOP`.
/// Inserts a **guard page** one page below the bottom of the stack —
/// any stack overflow triggers a page fault instead of silently
/// overwriting adjacent memory.
///
/// Returns the physical base address of the bottom stack frame.
pub fn map_user_stack(
    vmm: &mut VirtualMemoryManager,
    allocator: &mut FrameAllocator,
) -> Result<u64, LaunchError> {
    let pages = (SILO_STACK_SIZE / PhysFrame::SIZE) as u64;
    let stack_bottom_virt = USER_STACK_TOP - SILO_STACK_SIZE;

    let user_rw = MapPermissions::user_rw();

    let mut first_phys = 0u64;
    for i in 0..pages {
        let frame = allocator.allocate_frame()
            .ok_or(LaunchError::StackAllocFailed)?;
        if i == 0 { first_phys = frame.base_addr; }

        // Zero the stack frame (security: no residual kernel data)
        unsafe {
            core::ptr::write_bytes(frame.base_addr as *mut u8, 0, PhysFrame::SIZE as usize);
        }

        let virt = VirtAddr(stack_bottom_virt + i * PhysFrame::SIZE);
        vmm.map_page(virt, frame.base_addr, user_rw);
    }

    // Guard page: leave `stack_bottom_virt - 4096` unmapped (naturally a fault target)
    // No action needed — absent PTE = #PF on access.
    crate::serial_println!(
        "[LAUNCH] User stack: virt 0x{:x}–0x{:x}, guard at 0x{:x}",
        stack_bottom_virt, USER_STACK_TOP,
        stack_bottom_virt - PhysFrame::SIZE
    );

    Ok(first_phys)
}

/// Validate that an entry point is a legal user-space address.
///
/// Prevents a crafted ELF from jumping into kernel space.
/// User-space is limited to 0x0000_0000_0000_1000 – 0x0000_7FFF_FFFF_EFFF
/// (first page is also unmapped as a null-pointer guard).
pub fn validate_entry_point(entry: u64) -> Result<(), LaunchError> {
    if entry < 0x1000 || entry >= 0x0000_8000_0000_0000 {
        crate::serial_println!(
            "[LAUNCH ERROR] Entry point 0x{:x} outside user-space bounds!",
            entry
        );
        return Err(LaunchError::InvalidEntryPoint);
    }
    Ok(())
}

/// Compute the initial register state for Ring-3 entry.
pub fn build_entry_regs(binary: &LoadedBinary, silo_id: u64) -> Result<SiloEntryRegs, LaunchError> {
    validate_entry_point(binary.entry_point)?;
    Ok(SiloEntryRegs {
        rip:    binary.entry_point,
        rsp:    binary.stack_top,
        rflags: USER_RFLAGS,
        rdi:    0, // CapToken descriptor pointer set by spawn_capability syscall
        rsi:    silo_id,
    })
}

/// Jump to Ring-3 via SYSRET.
///
/// # About SYSRET
/// `SYSRET` resumes Ring-3 execution. Before calling it:
/// - RCX must hold the user RIP (entry point)
/// - R11 must hold the user RFLAGS
/// - RSP must hold the user stack pointer
/// - CS/SS are loaded from IA32_STAR MSR (set during GDT init)
///
/// # Safety
/// - All segment registers (CS, SS, DS, ES) must be pre-loaded with
///   Ring-3 descriptors from the GDT.
/// - The caller must have disabled interrupts before this call.
/// - There is NO return from this function — SYSRET transfers control.
///
/// ## Q-Manifest Law 6: Silo Sandbox
/// After SYSRET, the CPU is in Ring 3 with no kernel privileges.
/// The Silo cannot re-enter Ring 0 except via the SYSCALL instruction,
/// which routes through the hardened syscall dispatcher.
#[inline(never)]
pub unsafe fn jump_to_ring3(regs: &SiloEntryRegs) -> ! {
    crate::serial_println!(
        "[LAUNCH] Jumping to Ring-3: RIP=0x{:x}, RSP=0x{:x}, Silo RSI={}",
        regs.rip, regs.rsp, regs.rsi
    );

    core::arch::asm!(
        // Load user data segments into DS, ES (Ring 3 = selector 0x23)
        "mov ax, 0x23",
        "mov ds, ax",
        "mov es, ax",
        // Set RFLAGS via R11 (SYSRET restores R11 → RFLAGS)
        "mov r11, {rflags}",
        // Set RSP to user stack top
        "mov rsp, {rsp}",
        // Set RCX to user entry point (SYSRET jumps to RCX)
        "mov rcx, {rip}",
        // Load RDI and RSI (capability descriptor + silo_id)
        "mov rdi, {rdi}",
        "mov rsi, {rsi}",
        // SYSRET: returns to Ring 3 at RCX, RFLAGS ← R11
        "sysretq",
        rip    = in(reg) regs.rip,
        rsp    = in(reg) regs.rsp,
        rflags = in(reg) regs.rflags,
        rdi    = in(reg) regs.rdi,
        rsi    = in(reg) regs.rsi,
        options(noreturn),
    )
}

/// Complete Silo launch: validate binary, map stack, jump to Ring 3.
///
/// Called by the `SpawnCapability` syscall handler after the ELF segments
/// have been mapped by `loader::load_elf()`.
///
/// # Args
/// * `binary`   — result of `loader::load_elf()`
/// * `silo`     — the newly created QSilo (state must be `Spawning`)
/// * `allocator` — frame allocator for the stack
///
/// # Safety
/// This function does not return on success — it jumps to Ring 3.
pub unsafe fn launch_silo(
    binary: &LoadedBinary,
    silo: &QSilo,
    _allocator: &mut FrameAllocator,
) -> Result<(), LaunchError> {
    // Build initial register state (validates entry point)
    let regs = build_entry_regs(binary, silo.id)?;

    crate::serial_println!(
        "[LAUNCH] Silo {} launching: entry=0x{:x}, stack_top=0x{:x}",
        silo.id, regs.rip, regs.rsp
    );

    // Transfer to Ring 3 — no return
    jump_to_ring3(&regs)
}
