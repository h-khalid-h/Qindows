#![allow(deprecated)]
//! # Qindows UEFI Bootloader
//!
//! The bridge between hardware firmware and the Qernel.
//! Responsibilities:
//! - Initialize UEFI Graphics Output Protocol (GOP)
//! - Obtain the memory map from firmware
//! - Load the Qernel binary into memory
//! - Hand control to the Qernel entry point

#![no_std]
#![no_main]

extern crate alloc;

use uefi::prelude::*;
use uefi::proto::console::gop::GraphicsOutput;
use uefi::table::boot::{MemoryDescriptor, MemoryType};
use log::info;

/// Re-export shared BootInfo from qindows-types.
use qindows_types::boot::BootInfo;

/// UEFI entry point — the very first code that runs on Qindows hardware.
#[entry]
fn efi_main(image: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // Initialize UEFI services (logging, allocator)
    uefi_services::init(&mut system_table).expect("Failed to initialize UEFI services");

    info!("╔══════════════════════════════════════╗");
    info!("║   QINDOWS BOOTLOADER v1.0.0-genesis  ║");
    info!("║   The Final Operating System          ║");
    info!("╚══════════════════════════════════════╝");

    // ── Step 1: Initialize Graphics Output Protocol ────────────────
    let gop_handle = system_table
        .boot_services()
        .get_handle_for_protocol::<GraphicsOutput>()
        .expect("Qindows requires a UEFI-compliant display");

    let mut gop = system_table
        .boot_services()
        .open_protocol_exclusive::<GraphicsOutput>(gop_handle)
        .expect("Failed to open Graphics Output Protocol");

    // Select the highest available resolution
    let mode = gop
        .modes(system_table.boot_services())
        .last()
        .expect("No display modes available");

    gop.set_mode(&mode).expect("Failed to set display mode");

    let mode_info = gop.current_mode_info();
    let (h_res, v_res) = mode_info.resolution();
    let stride = mode_info.stride();

    let mut fb = gop.frame_buffer();
    let fb_addr = fb.as_mut_ptr() as u64;
    let fb_size = fb.size() as u64;

    info!(
        "Aether Display: {}x{} @ stride {} | FB: {:#x} ({} KB)",
        h_res, v_res, stride, fb_addr, fb_size / 1024
    );

    // ── Step 2: Obtain Memory Map ──────────────────────────────────
    info!("Scanning physical memory layout...");

    let mmap_size = system_table.boot_services().memory_map_size().map_size
        + 8 * core::mem::size_of::<MemoryDescriptor>();

    let mmap_buffer = system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, mmap_size)
        .expect("Failed to allocate memory map buffer");

    let mmap_slice = unsafe { core::slice::from_raw_parts_mut(mmap_buffer, mmap_size) };

    let memory_map = system_table
        .boot_services()
        .memory_map(mmap_slice)
        .expect("Failed to obtain UEFI memory map");

    let mut usable_ram: u64 = 0;
    let mut entry_count: u64 = 0;
    for desc in memory_map.entries() {
        if desc.ty == MemoryType::CONVENTIONAL {
            usable_ram += desc.page_count * 4096;
        }
        entry_count += 1;
    }

    info!(
        "Memory: {} entries, {} MB usable RAM",
        entry_count,
        usable_ram / (1024 * 1024)
    );

    // ── Step 3: Prepare Boot Info for Qernel ───────────────────────
    // Allocate BootInfo in LOADER_DATA memory — persists after ExitBootServices.
    // This address is passed to the Qernel and must remain valid.
    let boot_info_ptr = system_table
        .boot_services()
        .allocate_pool(MemoryType::LOADER_DATA, core::mem::size_of::<BootInfo>())
        .expect("Failed to allocate BootInfo") as *mut BootInfo;

    unsafe {
        boot_info_ptr.write(BootInfo {
            framebuffer_addr: fb_addr,
            framebuffer_size: fb_size,
            horizontal_resolution: h_res as u32,
            vertical_resolution: v_res as u32,
            pixels_per_scanline: stride as u32,
            memory_map_addr: mmap_buffer as u64,
            memory_map_entries: entry_count,
            memory_map_desc_size: core::mem::size_of::<MemoryDescriptor>() as u64,
        });
    }

    info!("Boot info allocated at {:#x}", boot_info_ptr as u64);
    info!("Genesis Protocol: BOOTLOADER COMPLETE.");

    // ── Step 4: Exit Boot Services & Jump to Qernel ────────────────
    // Production flow:
    // 1. Load Qernel ELF from EFI System Partition
    // 2. Parse ELF, map segments into physical memory
    // 3. Exit UEFI boot services (no firmware calls after this)
    // 4. Jump to Qernel: _start(boot_info_ptr as &'static BootInfo)
    //
    // The kernel entry point expects:
    //   extern "C" fn _start(boot_info: &'static BootInfo) -> !
    //
    // For now, we halt — the jump is implemented when we add the ELF loader.
    let _ = boot_info_ptr;

    info!("Qindows Qernel handoff would execute here.");
    info!("THE MESH AWAITS.");

    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
