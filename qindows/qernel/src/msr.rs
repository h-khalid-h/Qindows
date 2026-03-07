//! # x86 Model Specific Registers (MSR)
//!
//! Provides read/write access to CPU-specific features like
//! SYSCALL configuration, APIC base, thermals, and performance
//! counters (Section 9.2).
//!
//! Features:
//! - Safe `rdmsr` and `wrmsr` wrappers
//! - Standard MSR constants (EFER, STAR, LSTAR, APIC_BASE)
//! - Feature enablement (e.g., fast system calls)

#![allow(dead_code)]

/// Extended Feature Enable Register.
pub const MSR_EFER: u32 = 0xC0000080;
/// SYSCALL Target Address Register.
pub const MSR_STAR: u32 = 0xC0000081;
/// Long Mode SYSCALL Target Address.
pub const MSR_LSTAR: u32 = 0xC0000082;
/// SYSCALL Flag Mask.
pub const MSR_FMASK: u32 = 0xC0000084;
/// FS Base Address (thread-local storage).
pub const MSR_FS_BASE: u32 = 0xC0000100;
/// GS Base Address.
pub const MSR_GS_BASE: u32 = 0xC0000101;
/// Kernel GS Base (swapgs target).
pub const MSR_KERNEL_GS_BASE: u32 = 0xC0000102;
/// APIC Base Address.
pub const MSR_APIC_BASE: u32 = 0x0000001B;
/// Time Stamp Counter.
pub const MSR_TSC: u32 = 0x00000010;

/// Read a 64-bit value from a Model Specific Register.
#[inline]
pub unsafe fn rdmsr(msr: u32) -> u64 {
    let mut low: u32;
    let mut high: u32;
    core::arch::asm!(
        "rdmsr",
        in("ecx") msr,
        out("eax") low,
        out("edx") high,
        options(nomem, nostack, preserves_flags)
    );
    ((high as u64) << 32) | (low as u64)
}

/// Write a 64-bit value to a Model Specific Register.
#[inline]
pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low = value as u32;
    let high = (value >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") msr,
        in("eax") low,
        in("edx") high,
        options(nomem, nostack, preserves_flags)
    );
}

/// Enable the fast SYSCALL/SYSRET instructions.
///
/// Configures EFER to enable SYSCALL, sets the CS/SS segments
/// in STAR, and sets the instruction pointer for ring-0 entry in LSTAR.
pub unsafe fn enable_fast_syscalls(handler_addr: u64) {
    // Enable SYSCALL in EFER (bit 0 = SCE)
    let efer = rdmsr(MSR_EFER);
    wrmsr(MSR_EFER, efer | 1);

    // STAR: sysret_cs, syscall_cs
    // Bits 32-47: Kernel CS (Target for SYSCALL) -> 0x08
    // Bits 48-63: User CS (Target for SYSRET) -> 0x18 | 3
    let star = (0x001B_0008_u64) << 32;
    wrmsr(MSR_STAR, star);

    // LSTAR: handler RIP
    wrmsr(MSR_LSTAR, handler_addr);

    // FMASK: clear IF (disable interrupts on entry) and DF
    let fmask = 0x0200 | 0x0400; // RFLAGS.IF | RFLAGS.DF
    wrmsr(MSR_FMASK, fmask);
}

/// Configure the APIC Base address and enable it.
pub unsafe fn enable_apic(base_addr: u64) {
    // APIC Base MSR: 
    // Bits 12-35: Page base
    // Bit 11: Global Enable
    // Bit 8: BSP (Bootstrap Processor) flag
    let mut val = rdmsr(MSR_APIC_BASE);
    val &= 0xFFF; // Keep flags
    val |= base_addr & !0xFFF; // Set new base (must be page-aligned)
    val |= 1 << 11; // Set Global Enable bit
    wrmsr(MSR_APIC_BASE, val);
}

/// Read the Time Stamp Counter.
#[inline]
pub fn read_tsc() -> u64 {
    let mut low: u32;
    let mut high: u32;
    unsafe {
        core::arch::asm!(
            "rdtsc",
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
    ((high as u64) << 32) | (low as u64)
}
