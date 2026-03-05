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

pub mod acpi;
pub mod capability;
pub mod crypto;
pub mod dma;
pub mod drivers;
pub mod elf;
pub mod framebuffer;
pub mod gdt;
pub mod interrupts;
pub mod ioapic;
pub mod ipc;
pub mod lapic;
pub mod loader;
pub mod logging;
pub mod manifest;
pub mod memory;
pub mod pci_scan;
pub mod power;
pub mod process;
pub mod profiler;
pub mod scheduler;
pub mod sentinel;
pub mod settings;
pub mod smbios;
pub mod iommu;
pub mod usb;
pub mod hotplug;
pub mod telemetry;
pub mod silo;
pub mod smp;
pub mod syscall;
pub mod syscall_table;
pub mod timer;
pub mod timer_wheel;
pub mod watchdog;

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

/// The Qernel Entry Point — 8-Phase Boot Sequence.
///
/// Called by the bootloader after UEFI boot services have exited.
/// This is the absolute beginning of Qindows — no standard library,
/// no OS layer. We are talking directly to the CPU.
#[no_mangle]
pub extern "C" fn _start(boot_info: &'static BootInfo) -> ! {
    // Initialize serial port first (for debug output)
    drivers::serial::SerialWriter::init();
    serial_println!("Qernel boot sequence initiated...");

    // ── Phase 1: Memory ─────────────────────────────────────────
    // Initialize the physical memory manager with the UEFI memory map.
    let mut frame_allocator = memory::FrameAllocator::init(
        boot_info.memory_map_addr,
        boot_info.memory_map_entries,
        boot_info.memory_map_desc_size,
    );
    memory::paging::init(&mut frame_allocator);
    memory::heap::init(&mut frame_allocator);
    serial_println!("[OK] Phase 1: Memory (frames + paging + heap)");

    // ── Phase 2: GDT ────────────────────────────────────────────
    // Set up privilege levels (Ring 0 ↔ Ring 3) and the TSS
    // for stack switching on system calls.
    gdt::init();
    serial_println!("[OK] Phase 2: GDT (Ring-0/Ring-3 segments + TSS)");

    // ── Phase 3: IDT ────────────────────────────────────────────
    // Install exception handlers, hardware IRQ dispatch, and
    // the Q-Ring system call vector.
    interrupts::init();
    serial_println!("[OK] Phase 3: IDT (256 vectors, exceptions + IRQs)");

    // ── Phase 4: APIC ───────────────────────────────────────────
    // Replace the legacy 8259 PIC with the Local APIC.
    // Configure the APIC timer for preemptive scheduling.
    drivers::apic::init();
    drivers::apic::init_ioapic();
    serial_println!("[OK] Phase 4: APIC (Local + IO, timer @ vector 32)");

    // ── Phase 5: Aether Display ─────────────────────────────────
    // Initialize the framebuffer and draw the boot banner.
    let mut display = drivers::gpu::AetherFrameBuffer::new(
        boot_info.framebuffer_addr as *mut u32,
        boot_info.horizontal_resolution as usize,
        boot_info.vertical_resolution as usize,
        boot_info.pixels_per_scanline as usize,
    );
    display.clear(0x00_06_06_0E); // #06060E — the Qindows void
    drivers::gpu::draw_boot_logo(&mut display);

    // Boot console — text output on the framebuffer
    let mut console = drivers::console::FramebufferConsole::new(
        boot_info.horizontal_resolution as usize,
        boot_info.vertical_resolution as usize,
    );
    console.print_banner(&mut display);
    console.print_ok(&mut display, "Memory: frames + paging + kernel heap");
    console.print_ok(&mut display, "GDT: Ring-0 / Ring-3 / TSS loaded");
    console.print_ok(&mut display, "IDT: 256 interrupt vectors installed");
    console.print_ok(&mut display, "APIC: Local + IO APIC, timer periodic");
    console.print_ok(&mut display, "Aether: Framebuffer initialized");
    serial_println!("[OK] Phase 5: Aether Display + Boot Console");

    // ── Phase 6: System Calls ───────────────────────────────────
    // Configure SYSCALL/SYSRET fast-path via MSRs.
    syscall::init();
    console.print_ok(&mut display, "SYSCALL/SYSRET fast-path configured");
    serial_println!("[OK] Phase 6: SYSCALL/SYSRET MSRs configured");

    // ── Phase 7: Sentinel ───────────────────────────────────────
    // Start the AI law enforcement monitor.
    sentinel::init();
    console.print_ok(&mut display, "Sentinel: AI Law Enforcement ACTIVE");
    serial_println!("[OK] Phase 7: Sentinel AI Auditor online");

    // ── Phase 8: Scheduler ──────────────────────────────────────
    // Initialize the Fiber-based scheduler with SMP support.
    scheduler::init();
    console.print_ok(&mut display, "Scheduler: Fiber engine ready");
    serial_println!("[OK] Phase 8: Fiber Scheduler initialized");

    // ── Boot Complete ───────────────────────────────────────────
    console.write_str(&mut display, "\n");
    console.set_fg(0x00_06_D6_A0);
    console.write_str(&mut display, "  QINDOWS QERNEL v1.0.0 ONLINE\n");
    console.write_str(&mut display, "  THE MESH AWAITS.\n");

    serial_println!("╔══════════════════════════════════════╗");
    serial_println!("║    QINDOWS QERNEL v1.0.0 ONLINE     ║");
    serial_println!("║    8/8 Phases Complete               ║");
    serial_println!("║    Memory · GDT · IDT · APIC        ║");
    serial_println!("║    Aether · Syscall · Sentinel       ║");
    serial_println!("║    Scheduler · THE MESH AWAITS.      ║");
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
