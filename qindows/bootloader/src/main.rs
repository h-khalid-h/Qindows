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

/// Boot information passed from the bootloader to the Qernel.
/// Contains everything the kernel needs to initialize.
#[repr(C)]
pub struct BootInfo {
    /// Framebuffer base address for direct pixel access
    pub framebuffer_addr: u64,
    /// Framebuffer size in bytes
    pub framebuffer_size: u64,
    /// Horizontal resolution in pixels
    pub horizontal_resolution: u32,
    /// Vertical resolution in pixels
    pub vertical_resolution: u32,
    /// Pixels per scanline (may differ from horizontal_resolution)
    pub pixels_per_scanline: u32,
    /// Pointer to UEFI memory map descriptors
    pub memory_map_addr: u64,
    /// Number of memory map entries
    pub memory_map_entries: u64,
    /// Size of each memory map descriptor
    pub memory_map_desc_size: u64,
}

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
        .modes()
        .last()
        .expect("No display modes available");

    gop.set_mode(&mode).expect("Failed to set display mode");

    let mode_info = gop.current_mode_info();
    let (h_res, v_res) = mode_info.resolution();
    let stride = mode_info.stride();

    let fb = gop.frame_buffer();
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

    let (_key, desc_iter) = system_table
        .boot_services()
        .memory_map(mmap_slice)
        .expect("Failed to obtain UEFI memory map");

    let mut usable_ram: u64 = 0;
    let mut entry_count: u64 = 0;
    for desc in desc_iter {
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
    let boot_info = BootInfo {
        framebuffer_addr: fb_addr,
        framebuffer_size: fb_size,
        horizontal_resolution: h_res as u32,
        vertical_resolution: v_res as u32,
        pixels_per_scanline: stride as u32,
        memory_map_addr: mmap_buffer as u64,
        memory_map_entries: entry_count,
        memory_map_desc_size: core::mem::size_of::<MemoryDescriptor>() as u64,
    };

    info!("Boot info assembled. Preparing Qernel handoff...");
    info!("Genesis Protocol: BOOTLOADER COMPLETE.");

    // ── Step 4: Exit Boot Services & Jump to Qernel ────────────────
    // In a full build, we would:
    // 1. Load the Qernel ELF binary from the EFI System Partition
    // 2. Exit UEFI boot services (no more firmware calls after this)
    // 3. Jump to the Qernel entry point with &boot_info as argument
    //
    // For now, we demonstrate the boot sequence structure:
    let _ = boot_info; // Will be passed to qernel::_start(&boot_info)

    info!("Qindows Qernel handoff would execute here.");
    info!("THE MESH AWAITS.");

    // Halt — in production this is replaced by the jump to Qernel
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}
