//! # Context Switching
//!
//! The assembly-level mechanism for switching between Fibers.
//! This is the most performance-critical code in the entire Qernel.
//!
//! Each context switch:
//! 1. Saves the current fiber's callee-saved registers
//! 2. Switches the stack pointer
//! 3. Optionally switches CR3 (page table) for Silo transitions
//! 4. Restores the new fiber's registers
//! 5. Returns — which "jumps" to the new fiber's saved instruction

use super::FiberContext;

/// Perform a context switch from the current fiber to the next fiber.
///
/// # Safety
/// Must be called with interrupts disabled. Both contexts must be valid.
///
/// This is a `naked` function — the compiler generates no prologue/epilogue.
/// We manage all register state manually via inline assembly.
#[naked]
pub unsafe extern "C" fn switch_context(
    _old: *mut FiberContext,
    _new: *const FiberContext,
) {
    // RDI = old context pointer
    // RSI = new context pointer
    core::arch::asm!(
        // ═══════════════════════════════════════════
        // SAVE current fiber's state into *old (RDI)
        // ═══════════════════════════════════════════
        "mov [rdi + 0x00], rax",
        "mov [rdi + 0x08], rbx",
        "mov [rdi + 0x10], rcx",
        "mov [rdi + 0x18], rdx",
        "mov [rdi + 0x20], rsi",
        "mov [rdi + 0x28], rdi",
        "mov [rdi + 0x30], rbp",
        // Save the stack pointer (RSP) — the return address is on the stack
        "mov [rdi + 0x38], rsp",
        "mov [rdi + 0x40], r8",
        "mov [rdi + 0x48], r9",
        "mov [rdi + 0x50], r10",
        "mov [rdi + 0x58], r11",
        "mov [rdi + 0x60], r12",
        "mov [rdi + 0x68], r13",
        "mov [rdi + 0x70], r14",
        "mov [rdi + 0x78], r15",
        // Save the return address (top of stack)
        "mov rax, [rsp]",
        "mov [rdi + 0x80], rax",  // RIP
        // Save RFLAGS
        "pushfq",
        "pop rax",
        "mov [rdi + 0x88], rax",  // RFLAGS

        // ═══════════════════════════════════════════
        // RESTORE new fiber's state from *new (RSI)
        // ═══════════════════════════════════════════
        "mov rax, [rsi + 0x88]",  // RFLAGS
        "push rax",
        "popfq",
        "mov rax, [rsi + 0x00]",
        "mov rbx, [rsi + 0x08]",
        "mov rcx, [rsi + 0x10]",
        "mov rdx, [rsi + 0x18]",
        // RSI loaded last (it's our pointer)
        "mov rdi, [rsi + 0x28]",
        "mov rbp, [rsi + 0x30]",
        "mov rsp, [rsi + 0x38]",  // Switch stack!
        "mov r8,  [rsi + 0x40]",
        "mov r9,  [rsi + 0x48]",
        "mov r10, [rsi + 0x50]",
        "mov r11, [rsi + 0x58]",
        "mov r12, [rsi + 0x60]",
        "mov r13, [rsi + 0x68]",
        "mov r14, [rsi + 0x70]",
        "mov r15, [rsi + 0x78]",
        // Push the new fiber's RIP as the return address
        "mov rax, [rsi + 0x80]",
        "mov [rsp], rax",
        // Finally load RSI (couldn't do it earlier)
        "mov rsi, [rsi + 0x20]",
        // Return — pops the new fiber's RIP from the stack
        "ret",
        options(noreturn)
    );
}

/// Switch address space (CR3) for Silo transitions.
///
/// # Safety
/// The new CR3 must point to a valid PML4 page table.
pub unsafe fn switch_address_space(new_cr3: u64) {
    // Only switch if the address space actually changed
    let current_cr3: u64;
    core::arch::asm!("mov {}, cr3", out(reg) current_cr3, options(nomem, nostack));

    if current_cr3 != new_cr3 {
        core::arch::asm!("mov cr3, {}", in(reg) new_cr3, options(nostack));
        // CR3 write flushes the TLB — all cached address translations
        // are invalidated. This is expensive (~1000 cycles) but
        // necessary for Silo isolation.
    }
}

/// Flush a single TLB entry (after a single page mapping change).
pub unsafe fn invlpg(addr: u64) {
    core::arch::asm!("invlpg [{}]", in(reg) addr, options(nostack));
}
