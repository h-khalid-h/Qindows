//! # Qernel — The Qindows Microkernel
//!
//! A Rust-based, zero-trust microkernel. Only the absolute minimum runs in Ring 0:
//! - CPU Scheduling (Fiber-based)
//! - Inter-Process Communication (Q-Ring async buffers)
//! - Memory Mapping & Capability Management
//!
//! All drivers and system services run in isolated User-Mode Silos.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(naked_functions)]
#![feature(alloc_error_handler)]

extern crate alloc;

pub mod capability;
pub mod drivers;
pub mod interrupts;
pub mod memory;
pub mod scheduler;
pub mod sentinel;
pub mod silo;

use core::panic::PanicInfo;

/// Boot information received from the UEFI bootloader.
#[repr(C)]
pub struct BootInfo {
    pub framebuffer_addr: u64,
    pub framebuffer_size: u64,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixels_per_scanline: u32,
    pub memory_map_addr: u64,
    pub memory_map_entries: u64,
    pub memory_map_desc_size: u64,
}

/// The Qernel Entry Point.
///
/// Called by the bootloader after UEFI boot services have exited.
/// This is the absolute beginning of Qindows — no standard library,
/// no OS layer. We are talking directly to the CPU.
#[no_mangle]
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
    // ── Phase 1: Memory ─────────────────────────────────────────
    // Initialize the physical memory manager with the UEFI memory map.
    // This gives us the ability to allocate frames for page tables and Silos.
    let mut frame_allocator = memory::FrameAllocator::init(
        boot_info.memory_map_addr,
        boot_info.memory_map_entries,
        boot_info.memory_map_desc_size,
    );

    // Set up kernel page tables with identity mapping
    memory::paging::init(&mut frame_allocator);

    // Initialize the kernel heap (for dynamic allocations via `alloc`)
    memory::heap::init(&mut frame_allocator);

    // ── Phase 2: Interrupts ─────────────────────────────────────
    // Install the IDT (Interrupt Descriptor Table) so the CPU can
    // handle exceptions, hardware IRQs, and system calls.
    interrupts::init();

    // ── Phase 3: Aether Visual Root ─────────────────────────────
    // Initialize the framebuffer using GOP data from the bootloader.
    let mut display = drivers::gpu::AetherFrameBuffer::new(
        boot_info.framebuffer_addr as *mut u32,
        boot_info.horizontal_resolution as usize,
        boot_info.vertical_resolution as usize,
        boot_info.pixels_per_scanline as usize,
    );

    // Clear to the signature Qindows deep black
    display.clear(0x00_06_06_0E); // #06060E — the Qindows void

    // Draw the boot indicator
    drivers::gpu::draw_boot_logo(&mut display);

    // ── Phase 4: Sentinel ───────────────────────────────────────
    // Start the AI law enforcement monitor on a dedicated CPU core.
    // The Sentinel enforces the 10 Laws of the Q-Manifest.
    sentinel::init();

    // ── Phase 5: Scheduler ──────────────────────────────────────
    // Initialize the Fiber-based scheduler with SMP support.
    scheduler::init();

    // ── Boot Complete ───────────────────────────────────────────
    serial_println!("╔══════════════════════════════════════╗");
    serial_println!("║    QINDOWS QERNEL v1.0.0 ONLINE     ║");
    serial_println!("║    Memory · Interrupts · Aether      ║");
    serial_println!("║    Sentinel · Scheduler              ║");
    serial_println!("║    THE MESH AWAITS.                  ║");
    serial_println!("╚══════════════════════════════════════╝");

    // Enter the idle loop — HLT until an interrupt fires
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

/// Serial print macro — writes to COM1 (port 0x3F8) for debugging
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::drivers::serial::_print(format_args!($($arg)*))
    };
}

/// Serial println macro
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ($crate::serial_print!("{}\n", format_args!($($arg)*)))
}

/// Panic handler — the "Black Screen of Death"
///
/// In Qindows, a kernel panic is catastrophic. The Sentinel should
/// have caught any anomaly before this point. If we reach here,
/// something truly unexpected occurred.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("!!! QERNEL PANIC !!!");
    serial_println!("{}", info);

    // Halt all cores
    loop {
        unsafe { core::arch::asm!("cli; hlt") };
    }
}

/// Allocation error handler
#[alloc_error_handler]
fn alloc_error(layout: alloc::alloc::Layout) -> ! {
    panic!("Heap allocation failed: {:?}", layout);
}
