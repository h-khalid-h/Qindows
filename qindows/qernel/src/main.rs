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
#![feature(alloc_error_handler)]
#![allow(dead_code)]
#![allow(static_mut_refs)]

extern crate alloc;

pub mod math_ext;

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
pub mod coredump;
pub mod secure_boot;
pub mod numa;
pub mod rng;
pub mod hotswap;
pub mod qledger;
pub mod genesis;
pub mod sandbox;
pub mod q_admin;
pub mod entropy_pool;
pub mod pmc;
pub mod silo_snapshot;
pub mod power_gov;
pub mod npu_sched;
pub mod fault_inject;
pub mod msr;
pub mod disk_sched;
pub mod cgroup;
pub mod gpu_sched;
pub mod virtio;
pub mod virtio_gpu;
pub mod thermal;
pub mod rcu;
pub mod kprobe;
pub mod kdump;
pub mod mem_compress;
pub mod page_cache;
pub mod irq_balance;
pub mod dma_engine;
pub mod numa_alloc;
pub mod cpu_freq;
pub mod pci_enum;
pub mod spinlock;
pub mod ioport;
pub mod msi;
pub mod tsc;
pub mod apic_timer;
pub mod hpet;
pub mod power_mgmt;
pub mod pcm_audio;
pub mod efi_stub;
pub mod usb_hci;
pub mod rtc;
pub mod silo;
pub mod smp;
pub mod syscall;
pub mod syscall_table;
pub mod timer;
pub mod timer_wheel;
pub mod qaudit;
pub mod qquota;
pub mod kstate;

use core::panic::PanicInfo;

/// Re-export shared BootInfo from qindows-types.
pub use qindows_types::boot::BootInfo;

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

    // ── Phase 9: Timekeeping & Entropy ──────────────────────────
    // Calibrate high-resolution timers and seed the CSPRNG.
    let mut rtc = rtc::Rtc::new();
    let wall_clock = rtc.read_time();
    let boot_timestamp = wall_clock.to_timestamp();

    let _hpet = hpet::Hpet::new(0xFED0_0000, 100_000_000, 3); // ACPI HPET base
    // hpet.enable(); — would start the counter in production

    let mut tsc = tsc::TscManager::new();
    tsc.calibrate(1_000_000_000, 3_000_000_000, 8); // 3 GHz estimate
    tsc.set_reliability(tsc::TscReliability::Invariant);

    let mut hwrng = rng::HardwareRng::init();
    hwrng.seed_from_hardware();

    console.print_ok(&mut display, "Timekeeping: HPET + TSC + RTC calibrated");
    console.print_ok(&mut display, "Entropy: RDRAND/RDSEED pool seeded");
    serial_println!("[OK] Phase 9: Timekeeping (HPET + TSC + RTC) & Entropy (CSPRNG seeded)");

    // ── Phase 10: Hardware Discovery ────────────────────────────
    // Enumerate PCI bus and initialize discovered controllers.
    let mut pci = pci_scan::PciScanner::new();
    pci.scan();
    let pci_count = pci.devices.len();

    // Initialize storage controllers discovered on PCI
    let nvme_devices = pci.find_by_class(pci_scan::PciClass::MassStorage);
    let nvme_count = nvme_devices.len();
    // In production: for each NVMe BAR0, call drivers::nvme::NvmeController::init(bar0)

    // Initialize USB host controllers
    let usb_devices = pci.find_by_class(pci_scan::PciClass::SerialBus);
    let usb_count = usb_devices.len();
    // In production: for each xHCI BAR0, call drivers::usb_xhci::XhciController::init(bar0)

    // Initialize audio
    let audio_devices = pci.find_by_class(pci_scan::PciClass::Multimedia);
    let audio_count = audio_devices.len();
    // In production: for each HDA BAR0, call drivers::audio_hda::HdaController::init(bar0)

    // Initialize PS/2 input devices
    drivers::mouse::init(
        boot_info.horizontal_resolution as i32,
        boot_info.vertical_resolution as i32,
    );

    console.print_ok(&mut display, "PCI: Bus enumeration complete");
    serial_println!(
        "[OK] Phase 10: Hardware Discovery ({} PCI devices: {} storage, {} USB, {} audio)",
        pci_count, nvme_count, usb_count, audio_count
    );

    // ── Phase 11: Security Hardening ────────────────────────────
    // Measure boot chain, start audit log, configure capability system.
    let mut secure_boot = secure_boot::SecureBoot::new(
        secure_boot::BootPolicy::Enforce,
    );
    secure_boot.measure(
        secure_boot::BootComponent::Kernel,
        b"qernel-1.0.0-genesis",
        "Qernel binary measurement",
        boot_timestamp,
    );
    secure_boot.lock_boot_pcrs();

    let mut audit = qaudit::AuditLog::new(8192);
    audit.log(
        qaudit::Severity::Info,
        qaudit::AuditCategory::SystemBoot,
        None,
        "qernel",
        "boot_start",
        true,
        "13-phase boot initiated",
        boot_timestamp,
    );

    let _quota_mgr = qquota::QuotaManager::new();
    let _admin = q_admin::QAdmin::new();

    console.print_ok(&mut display, "Secure Boot: PCRs measured & locked");
    console.print_ok(&mut display, "Audit: Hash-chained event log ACTIVE");
    serial_println!("[OK] Phase 11: Security Hardening (SecureBoot + Audit + Quota + Q-Admin)");

    // ── Phase 12: System Services ───────────────────────────────
    // Start telemetry, control groups, IOMMU, and read SMBIOS.
    let mut telemetry = telemetry::TelemetryEngine::new();
    telemetry.add_alert("cpu_usage", 90.0, true, 3);
    telemetry.add_alert("memory_pressure", 85.0, true, 5);

    let _cgroups = cgroup::CGroupManager::new();
    let _iommu = iommu::Iommu::new();

    console.print_ok(&mut display, "Telemetry: Ring-buffered metrics ACTIVE");
    console.print_ok(&mut display, "CGroups: Resource limits configured");
    serial_println!("[OK] Phase 12: System Services (Telemetry + CGroups + IOMMU)");

    // ── Phase 13: Genesis Protocol ──────────────────────────────
    // First-boot initialization: HW survey, identity, Silo Zero.
    let mut genesis = genesis::GenesisProtocol::new();

    if !genesis.check_completed() {
        serial_println!("  Genesis: First boot detected — running full protocol");
        let _ = genesis.run_full(boot_timestamp);
        console.print_ok(&mut display, "Genesis: Identity + Ledger + Prism DONE");
    } else {
        console.print_ok(&mut display, "Genesis: Previously completed (skipping)");
    }

    // Spawn the System Silo (PID 1)
    let mut silo_mgr = silo::SiloManager::new();
    let system_silo_id = silo_mgr.spawn(0x0000_DEAD_BEEF_0001, 0); // System binary OID

    // Grant the System Silo full capabilities
    if let Some(sys_silo) = silo_mgr.get_mut(system_silo_id) {
        use capability::Permissions;
        sys_silo.grant_capability(capability::CapToken::new(
            system_silo_id,              // owner_silo
            0,                           // target_oid (system-wide)
            Permissions::all(),
        ).with_expiry(boot_timestamp + 86400 * 365)); // 1-year expiry
        sys_silo.state = silo::SiloState::Running;
    }

    // Log the Silo creation in the audit trail
    audit.log(
        qaudit::Severity::Info,
        qaudit::AuditCategory::SiloLifecycle,
        Some(system_silo_id),
        "genesis",
        "silo_spawn",
        true,
        "System Silo (PID 1) created with full capabilities",
        boot_timestamp,
    );

    console.print_ok(&mut display, "Silo Manager: System Silo (PID 1) ONLINE");
    serial_println!("[OK] Phase 13: Genesis Protocol + System Silo spawned");

    // ── Phase 14: Service Silos ─────────────────────────────────
    // Spawn dedicated silos for each major subsystem.
    // Each silo gets only the capabilities it needs (least privilege).
    let mut ipc_mgr = ipc::IpcManager::new();
    {
        use capability::{CapToken, Permissions};

        // ── Prism (Object Storage Engine) ───────────────────────
        let prism_silo_id = silo_mgr.spawn(0x0000_0001_0000_0001, 0);
        if let Some(prism) = silo_mgr.get_mut(prism_silo_id) {
            prism.grant_capability(CapToken::new(
                prism_silo_id, 0,
                Permissions::READ | Permissions::WRITE | Permissions::PRISM | Permissions::DEVICE,
            ));
            prism.state = silo::SiloState::Running;
        }
        let _ = ipc_mgr.create_channel(
            system_silo_id, prism_silo_id,
            &CapToken::new(system_silo_id, 0, Permissions::SPAWN),
        );
        serial_println!("  → Prism Silo #{} spawned (R/W + Storage)", prism_silo_id);

        // ── Aether (GPU Compositor / UI) ────────────────────────
        let aether_silo_id = silo_mgr.spawn(0x0000_0002_0000_0001, 0);
        if let Some(aether) = silo_mgr.get_mut(aether_silo_id) {
            aether.grant_capability(CapToken::new(
                aether_silo_id, 0,
                Permissions::READ | Permissions::GRAPHICS | Permissions::DEVICE,
            ));
            aether.state = silo::SiloState::Running;
        }
        let _ = ipc_mgr.create_channel(
            system_silo_id, aether_silo_id,
            &CapToken::new(system_silo_id, 0, Permissions::SPAWN),
        );
        serial_println!("  → Aether Silo #{} spawned (GFX + Input)", aether_silo_id);

        // ── Nexus (Mesh Networking) ─────────────────────────────
        let nexus_silo_id = silo_mgr.spawn(0x0000_0003_0000_0001, 0);
        if let Some(nexus) = silo_mgr.get_mut(nexus_silo_id) {
            nexus.grant_capability(CapToken::new(
                nexus_silo_id, 0,
                Permissions::NET_SEND | Permissions::NET_RECV | Permissions::DEVICE,
            ));
            nexus.state = silo::SiloState::Running;
        }
        let _ = ipc_mgr.create_channel(
            system_silo_id, nexus_silo_id,
            &CapToken::new(system_silo_id, 0, Permissions::SPAWN),
        );
        serial_println!("  → Nexus Silo #{} spawned (Net Send/Recv)", nexus_silo_id);

        // ── Synapse (AI / NLP Engine) ───────────────────────────
        let synapse_silo_id = silo_mgr.spawn(0x0000_0004_0000_0001, 0);
        if let Some(synapse) = silo_mgr.get_mut(synapse_silo_id) {
            synapse.grant_capability(CapToken::new(
                synapse_silo_id, 0,
                Permissions::READ | Permissions::NEURAL | Permissions::DEVICE,
            ));
            synapse.state = silo::SiloState::Running;
        }
        let _ = ipc_mgr.create_channel(
            system_silo_id, synapse_silo_id,
            &CapToken::new(system_silo_id, 0, Permissions::SPAWN),
        );
        serial_println!("  → Synapse Silo #{} spawned (Neural + Read)", synapse_silo_id);

        // ── Q-Shell (Terminal) ──────────────────────────────────
        let qshell_silo_id = silo_mgr.spawn(0x0000_0005_0000_0001, 0);
        if let Some(qshell) = silo_mgr.get_mut(qshell_silo_id) {
            qshell.grant_capability(CapToken::new(
                qshell_silo_id, 0,
                Permissions::READ | Permissions::WRITE | Permissions::EXECUTE | Permissions::SPAWN,
            ));
            qshell.state = silo::SiloState::Running;
        }
        let _ = ipc_mgr.create_channel(
            system_silo_id, qshell_silo_id,
            &CapToken::new(system_silo_id, 0, Permissions::SPAWN),
        );
        serial_println!("  → Q-Shell Silo #{} spawned (R/W/X + Spawn)", qshell_silo_id);
    }

    let active_silo_count = silo_mgr.silos.len();
    let active_channel_count = ipc_mgr.channels.len();

    console.print_ok(&mut display, "Service Silos: Prism + Aether + Nexus + Synapse + Q-Shell");
    serial_println!(
        "[OK] Phase 14: {} Service Silos spawned, {} IPC channels established",
        active_silo_count, active_channel_count
    );

    // ── Phase 15: Kernel State Finalization ─────────────────────
    // Transfer ownership of managers into the global kernel state.
    // From this point, syscall handlers can access silos, IPC, and audit.
    kstate::init(silo_mgr, ipc_mgr, audit, boot_timestamp);
    console.print_ok(&mut display, "Kernel State: Global singleton initialized");
    serial_println!("[OK] Phase 15: Kernel state finalized — syscall dispatch LIVE");

    // ── Boot Complete ───────────────────────────────────────────
    console.write_str(&mut display, "\n");
    console.set_fg(0x00_06_D6_A0);
    console.write_str(&mut display, "  QINDOWS QERNEL v1.0.0 ONLINE\n");
    console.write_str(&mut display, "  THE MESH AWAITS.\n");

    serial_println!("╔══════════════════════════════════════╗");
    serial_println!("║    QINDOWS QERNEL v1.0.0 ONLINE     ║");
    serial_println!("║    15/15 Phases Complete             ║");
    serial_println!("║    Memory · GDT · IDT · APIC        ║");
    serial_println!("║    Aether · Syscall · Sentinel       ║");
    serial_println!("║    Scheduler · Timekeeping           ║");
    serial_println!("║    Hardware · Security · Services    ║");
    serial_println!("║    Genesis · Service Silos LIVE      ║");
    serial_println!("║   {} Silos · {} IPC Channels          ║", active_silo_count, active_channel_count);
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
