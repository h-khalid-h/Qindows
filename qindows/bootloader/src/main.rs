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
fn efi_main(_image: Handle, mut system_table: SystemTable<Boot>) -> Status {
    // Initialize UEFI services (logging, allocator)
    uefi_services::init(&mut system_table).expect("Failed to initialize UEFI services");

    info!("╔══════════════════════════════════════╗");
    info!("║   QINDOWS BOOTLOADER v1.0.0-genesis  ║");
    info!("║   The Final Operating System          ║");
    info!("╚══════════════════════════════════════╝");

    // ── Step 1: Initialize Graphics & collect framebuffer info ──────
    // Scoped so GOP borrow is dropped before exit_boot_services.
    let (fb_addr, fb_size, h_res, v_res, stride) = {
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
        let (h, v) = mode_info.resolution();
        let s = mode_info.stride();

        let mut fb = gop.frame_buffer();
        let addr = fb.as_mut_ptr() as u64;
        let size = fb.size() as u64;

        info!(
            "Aether Display: {}x{} @ stride {} | FB: {:#x} ({} KB)",
            h, v, s, addr, size / 1024
        );

        (addr, size, h, v, s)
    }; // ← GOP borrow dropped here

    // ── Step 2: Obtain Memory Map ──────────────────────────────────
    let (mmap_buffer_addr, entry_count, usable_ram) = {
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

        let mut ram: u64 = 0;
        let mut count: u64 = 0;
        for desc in memory_map.entries() {
            if desc.ty == MemoryType::CONVENTIONAL {
                ram += desc.page_count * 4096;
            }
            count += 1;
        }

        info!(
            "Memory: {} entries, {} MB usable RAM",
            count,
            ram / (1024 * 1024)
        );

        (mmap_buffer as u64, count, ram)
    }; // ← memory_map borrow dropped here

    let _ = usable_ram; // used for logging above



    // ── Step 4: Load Qernel Binary ─────────────────────────────────
    // Embed the raw kernel ELF at compile time.
    // We read the entry point from the ELF header (e_entry at byte 24).
    const KERNEL_LOAD_ADDR: u64 = 0x20_0000; // 2 MiB
    static KERNEL_ELF: &[u8] = include_bytes!("../blob/qernel.elf");

    // Read ELF64 entry point (little-endian u64 at offset 24)
    let entry_point = u64::from_le_bytes([
        KERNEL_ELF[24], KERNEL_ELF[25], KERNEL_ELF[26], KERNEL_ELF[27],
        KERNEL_ELF[28], KERNEL_ELF[29], KERNEL_ELF[30], KERNEL_ELF[31],
    ]);

    info!(
        "Loading Qernel: {} bytes (ELF), entry @ {:#x}",
        KERNEL_ELF.len(), entry_point
    );

    // Parse ELF program headers to load segments into memory
    // ELF64: e_phoff at offset 32, e_phentsize at 54, e_phnum at 56
    let ph_off = u64::from_le_bytes([
        KERNEL_ELF[32], KERNEL_ELF[33], KERNEL_ELF[34], KERNEL_ELF[35],
        KERNEL_ELF[36], KERNEL_ELF[37], KERNEL_ELF[38], KERNEL_ELF[39],
    ]) as usize;
    let ph_size = u16::from_le_bytes([KERNEL_ELF[54], KERNEL_ELF[55]]) as usize;
    let ph_num = u16::from_le_bytes([KERNEL_ELF[56], KERNEL_ELF[57]]) as usize;

    // Allocate enough pages to cover the kernel address range
    // We'll allocate generously: 256 pages (1 MiB) starting at 2 MiB
    let kernel_pages = 256; // 1 MiB of space
    system_table
        .boot_services()
        .allocate_pages(
            uefi::table::boot::AllocateType::Address(KERNEL_LOAD_ADDR),
            MemoryType::LOADER_DATA,
            kernel_pages,
        )
        .expect("Failed to allocate memory at kernel load address");

    // Zero out the allocated region first
    unsafe {
        core::ptr::write_bytes(KERNEL_LOAD_ADDR as *mut u8, 0, kernel_pages * 4096);
    }

    // ── Step 3: Prepare Boot Info for Qernel ───────────────────────
    // Write BootInfo at a fixed address within the kernel's allocated
    // 1 MiB region. This ensures the pointer survives ExitBootServices.
    // We place it at the end of the 1 MiB region at 0x2FF000.
    const BOOT_INFO_ADDR: u64 = 0x2F_F000;
    let boot_info_ptr = BOOT_INFO_ADDR as *mut BootInfo;

    unsafe {
        boot_info_ptr.write(BootInfo {
            framebuffer_addr: fb_addr,
            framebuffer_size: fb_size,
            horizontal_resolution: h_res as u32,
            vertical_resolution: v_res as u32,
            pixels_per_scanline: stride as u32,
            memory_map_addr: mmap_buffer_addr,
            memory_map_entries: entry_count,
            memory_map_desc_size: core::mem::size_of::<MemoryDescriptor>() as u64,
        });
    }

    info!("Boot info written at {:#x}", BOOT_INFO_ADDR);

    // Load each PT_LOAD segment
    let mut segments_loaded = 0u32;
    for i in 0..ph_num {
        let ph = &KERNEL_ELF[ph_off + i * ph_size..];
        let p_type = u32::from_le_bytes([ph[0], ph[1], ph[2], ph[3]]);

        if p_type != 1 { continue; } // PT_LOAD = 1

        let p_offset = u64::from_le_bytes([ph[8], ph[9], ph[10], ph[11], ph[12], ph[13], ph[14], ph[15]]) as usize;
        let p_vaddr = u64::from_le_bytes([ph[16], ph[17], ph[18], ph[19], ph[20], ph[21], ph[22], ph[23]]);
        let p_filesz = u64::from_le_bytes([ph[32], ph[33], ph[34], ph[35], ph[36], ph[37], ph[38], ph[39]]) as usize;

        if p_filesz > 0 && p_offset + p_filesz <= KERNEL_ELF.len() {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    KERNEL_ELF[p_offset..].as_ptr(),
                    p_vaddr as *mut u8,
                    p_filesz,
                );
            }
            segments_loaded += 1;
        }
    }

    info!("Qernel loaded: {} segments, entry @ {:#x}", segments_loaded, entry_point);
    info!("Genesis Protocol: BOOTLOADER COMPLETE.");
    info!("Jumping to Qernel _start...");

    // ── Step 5: Exit Boot Services & Jump to Qernel ────────────────
    let boot_info_ref: &'static BootInfo = unsafe { &*boot_info_ptr };

    // Exit boot services — point of no return (consumes system_table)
    let (_runtime, _mmap) = system_table
        .exit_boot_services(MemoryType::LOADER_DATA);

    // Jump to the kernel at its actual entry point!
    type KernelEntry = extern "C" fn(&'static BootInfo) -> !;
    let kernel_main: KernelEntry = unsafe {
        core::mem::transmute(entry_point as *const ())
    };

    kernel_main(boot_info_ref);
}

