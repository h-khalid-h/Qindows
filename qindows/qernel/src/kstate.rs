//! # Kernel State
//!
//! Global kernel state accessible from syscall handlers and interrupt contexts.
//! Uses `spin::Once` for safe one-time initialization and `spin::Mutex` for
//! interior mutability.

use spin::{Mutex, Once};
use crate::silo::SiloManager;
use crate::ipc::IpcManager;
use crate::qaudit::AuditLog;
use crate::syscall_table::SyscallTable;
extern crate nexus;
extern crate synapse;
extern crate chimera;

/// Global kernel state — initialized once during boot, accessible everywhere.
pub struct KernelState {
    /// Silo manager — tracks all active silos
    pub silo_mgr: Mutex<SiloManager>,
    /// IPC manager — tracks all Q-Ring channels
    pub ipc_mgr: Mutex<IpcManager>,
    /// Audit log — hash-chained event log
    pub audit: Mutex<AuditLog>,
    /// Syscall dispatch table with statistics
    pub syscall_table: SyscallTable,
    /// Boot timestamp (ticks since epoch)
    pub boot_timestamp: u64,
    /// Number of active CPU cores
    pub cpu_count: u32,
    /// Q-Nexus global mesh engine
    pub nexus: Mutex<nexus::QNexus>,
    /// Q-Synapse BCI engine
    pub synapse: Mutex<synapse::QSynapse>,
    /// Chimera Virtualization Layer
    pub chimera: Mutex<chimera::ChimeraSilo>,
    /// NVMe Block Device Controller
    pub nvme: Mutex<alloc::boxed::Box<crate::drivers::nvme::NvmeController>>,
    /// VirtIO Network Interface
    pub virtio_net: Mutex<alloc::boxed::Box<crate::drivers::virtio_net::VirtioNet>>,
    /// Intel HD Audio Controller
    pub hda: Mutex<crate::drivers::audio_hda::HdaController>,
    /// System Telemetry Engine
    pub telemetry: Mutex<crate::telemetry::TelemetryEngine>,
    /// Performance Monitoring Counters
    pub pmc: Mutex<crate::pmc::PmcMonitor>,
    /// Power Manager (P-State / C-State control)
    pub power_mgr: Mutex<crate::power_mgmt::PowerManager>,
    /// Power Governor (thermal-aware scheduling)
    pub power_gov: Mutex<crate::power_gov::PowerGovernor>,
    /// Thermal Monitor (zone sensors + trip points)
    pub thermal: Mutex<crate::thermal::ThermalMonitor>,
    /// Security Audit Logger (hash-chained event log)
    pub audit_log: Mutex<crate::qaudit::AuditLog>,
    /// Verified Boot Chain (PCR measurement log)
    pub secure_boot: Mutex<crate::secure_boot::SecureBoot>,
    /// CGroup Resource Control Manager
    pub cgroup_mgr: Mutex<alloc::boxed::Box<crate::cgroup::CGroupManager>>,
    /// IOMMU DMA Remapping Manager
    pub iommu: Mutex<alloc::boxed::Box<crate::iommu::Iommu>>,
    /// ELF Binary Loader
    pub elf_loader: Mutex<alloc::boxed::Box<crate::elf::ElfLoader>>,
    /// Hardware Random Number Generator
    pub rng: Mutex<crate::rng::HardwareRng>,
    /// Wasm Sandbox Manager
    pub sandbox: Mutex<crate::sandbox::SandboxManager>,
    /// GPU Task Scheduler
    pub gpu_sched: Mutex<alloc::boxed::Box<crate::gpu_sched::GpuScheduler>>,
    /// NPU Task Scheduler
    pub npu_sched: Mutex<alloc::boxed::Box<crate::npu_sched::NpuScheduler>>,
    /// Disk I/O Scheduler
    pub disk_sched: Mutex<alloc::boxed::Box<crate::disk_sched::DiskScheduler>>,
    /// Core Dump Manager
    pub dump_mgr: Mutex<alloc::boxed::Box<crate::coredump::DumpManager>>,
    /// Q-Ledger App Distribution
    pub ledger: Mutex<crate::qledger::QLedger>,
    /// Q-Admin Temporal Escalation
    pub admin: Mutex<crate::q_admin::QAdmin>,
    /// Quota Manager
    pub quota: Mutex<crate::qquota::QuotaManager>,
    /// Silo Snapshot Manager
    pub snapshot: Mutex<crate::silo_snapshot::SnapshotManager>,
    /// Hierarchical Timer Wheel
    pub timer_wheel: Mutex<crate::timer_wheel::TimerWheel>,
    /// Device Hotplug Manager
    pub hotplug: Mutex<crate::hotplug::HotplugManager>,
    /// IRQ Balancer
    pub irq_balancer: Mutex<crate::irq_balance::IrqBalancer>,
    /// Page Cache
    pub page_cache: Mutex<alloc::boxed::Box<crate::page_cache::PageCache>>,
    /// CPU Frequency Scaler
    pub cpu_freq: Mutex<crate::cpu_freq::CpuFreqScaler>,
    /// Entropy Pool
    pub entropy_pool: Mutex<crate::entropy_pool::EntropyPool>,
    /// NUMA Topology Manager
    pub numa: Mutex<alloc::boxed::Box<crate::numa::NumaManager>>,
    /// Memory Compressor
    pub mem_compress: Mutex<alloc::boxed::Box<crate::mem_compress::MemCompress>>,
    /// High Precision Event Timer
    pub hpet: Mutex<alloc::boxed::Box<crate::hpet::Hpet>>,
    /// Real-Time Clock
    pub rtc: Mutex<alloc::boxed::Box<crate::rtc::Rtc>>,
    /// Timestamp Counter Manager
    pub tsc: Mutex<alloc::boxed::Box<crate::tsc::TscManager>>,
    /// MSR Operation Statistics
    pub msr_stats: core::sync::atomic::AtomicU64,
    /// PCI Device List
    pub pci_devices: Mutex<alloc::boxed::Box<alloc::vec::Vec<crate::drivers::pci::PciDevice>>>,
    /// MSI Interrupt Controller
    pub msi_manager: Mutex<alloc::boxed::Box<crate::msi::MsiController>>,
    /// SMBIOS Hardware Inventory
    pub smbios_inventory: Mutex<alloc::boxed::Box<crate::smbios::HardwareInventory>>,
    /// USB Host Controller Interface
    pub usb_hci: Mutex<alloc::boxed::Box<crate::usb_hci::UsbHci>>,
    /// VirtIO GPU Driver
    pub virtio_gpu: Mutex<alloc::boxed::Box<crate::virtio_gpu::VirtioGpu>>,
    /// DMA Engine
    pub dma: Mutex<alloc::boxed::Box<crate::dma_engine::DmaEngine>>,
    /// ACPI Parser
    pub acpi: Mutex<alloc::boxed::Box<crate::acpi::AcpiParser>>,
    /// PCM Audio Mixer
    pub pcm: Mutex<alloc::boxed::Box<crate::pcm_audio::PcmMixer>>,
    /// Hot-Swap Engine
    pub hotswap: Mutex<alloc::boxed::Box<crate::hotswap::HotSwapEngine>>,
    /// KProbe Manager
    pub kprobe: Mutex<alloc::boxed::Box<crate::kprobe::KProbeManager>>,
    /// Fault Injection Engine
    pub fault_inject: Mutex<alloc::boxed::Box<crate::fault_inject::FaultInjector>>,
    /// KDump Crash Analyzer
    pub kdump: Mutex<alloc::boxed::Box<crate::kdump::KDump>>,
    /// RCU Concurrency Manager
    pub rcu: Mutex<alloc::boxed::Box<crate::rcu::RcuManager>>,
    /// Genesis Protocol State
    pub genesis: Mutex<alloc::boxed::Box<crate::genesis::GenesisProtocol>>,
    /// NUMA Allocator
    pub numalloc: Mutex<alloc::boxed::Box<crate::memory::numa::NumaAllocator>>,
}

/// The global kernel state singleton.
static KERNEL: Once<KernelState> = Once::new();

/// Initialize the global kernel state (called once during boot).
pub fn init(
    silo_mgr: SiloManager,
    ipc_mgr: IpcManager,
    audit: AuditLog,
    boot_timestamp: u64,
) {
    KERNEL.call_once(|| KernelState {
        silo_mgr: Mutex::new(silo_mgr),
        ipc_mgr: Mutex::new(ipc_mgr),
        audit: Mutex::new(audit),
        syscall_table: SyscallTable::new(),
        boot_timestamp,
        cpu_count: 1, // SMP will update this
        nexus: Mutex::new(nexus::QNexus {
            peers: alloc::vec::Vec::new(),
            local_identity: nexus::PeerIdentity {
                node_id: [0; 32],
                alias: alloc::string::String::from("Node-01"),
                capabilities: nexus::HardwareProfile {
                    cpu_cores: 4,
                    gpu_units: 0,
                    has_npu: false,
                    ram_mb: 256,
                    bandwidth_mbps: 1000,
                },
                availability: 1.0,
                reputation: 100,
            },
            offloaded_tasks: alloc::vec::Vec::new(),
            credits_earned: 0,
            fibers_processed: 0,
        }),
        synapse: Mutex::new(synapse::QSynapse::new()),
        chimera: Mutex::new(chimera::ChimeraSilo::new(0)),
        nvme: Mutex::new(alloc::boxed::Box::new(crate::drivers::nvme::NvmeController::init(0xFE00_0000))),
        virtio_net: Mutex::new(alloc::boxed::Box::new(crate::drivers::virtio_net::VirtioNet::init(0xFE80_0000))),
        hda: Mutex::new(crate::drivers::audio_hda::HdaController::init(0xFEB0_0000)),
        telemetry: Mutex::new(crate::telemetry::TelemetryEngine::new()),
        pmc: Mutex::new(crate::pmc::PmcMonitor::new()),
        power_mgr: Mutex::new(crate::power_mgmt::PowerManager::new(4)),
        power_gov: Mutex::new({
            let mut gov = crate::power_gov::PowerGovernor::new();
            gov.add_core(0, crate::power_gov::CoreType::Performance, 4500, 800);
            gov.add_core(1, crate::power_gov::CoreType::Performance, 4500, 800);
            gov.add_core(2, crate::power_gov::CoreType::Efficiency, 3200, 400);
            gov.add_core(3, crate::power_gov::CoreType::Efficiency, 3200, 400);
            gov.thermals.push(crate::power_gov::ThermalZone {
                id: 0, name: "cpu-pkg",
                temp_c10: 450, trip_passive: 850, trip_critical: 1050,
            });
            gov
        }),
        thermal: Mutex::new({
            let mut tm = crate::thermal::ThermalMonitor::new();
            tm.add_zone(0, "cpu-package", crate::thermal::ZoneType::Cpu, 5000);
            tm.add_zone(1, "gpu", crate::thermal::ZoneType::Gpu, 3000);
            tm.add_trip(0, 85_000, crate::thermal::TripAction::Passive);
            tm.add_trip(0, 95_000, crate::thermal::TripAction::Active);
            tm.add_trip(0, 105_000, crate::thermal::TripAction::Critical);
            tm.add_trip(1, 90_000, crate::thermal::TripAction::Passive);
            tm.add_trip(1, 100_000, crate::thermal::TripAction::Critical);
            tm
        }),
        audit_log: Mutex::new({
            let mut log = crate::qaudit::AuditLog::new(1024);
            log.log(
                crate::qaudit::Severity::Info,
                crate::qaudit::AuditCategory::SystemBoot,
                None, "qernel", "boot", true, "Qindows kernel initialized",
                0,
            );
            log
        }),
        secure_boot: Mutex::new({
            let mut sb = crate::secure_boot::SecureBoot::new(
                crate::secure_boot::BootPolicy::Audit
            );
            // Register trusted boot components
            let bl_hash = crate::secure_boot::Pcr::simple_hash(b"qindows-bootloader-v1.0");
            sb.add_trusted(crate::secure_boot::BootComponent::Bootloader, bl_hash, "qindows-bootloader", 1);
            let kern_hash = crate::secure_boot::Pcr::simple_hash(b"qernel-v1.0-genesis");
            sb.add_trusted(crate::secure_boot::BootComponent::Kernel, kern_hash, "qernel", 1);
            // Measure bootloader and kernel now
            sb.measure(crate::secure_boot::BootComponent::Bootloader, b"qindows-bootloader-v1.0", "UEFI bootloader", 0);
            sb.measure(crate::secure_boot::BootComponent::Kernel, b"qernel-v1.0-genesis", "Qernel kernel", 0);
            sb.lock_boot_pcrs();
            sb
        }),
        cgroup_mgr: Mutex::new(alloc::boxed::Box::new({
            let mut mgr = crate::cgroup::CGroupManager::new();
            let root_id = mgr.create("system", 0, None);
            mgr.set_limit(root_id, crate::cgroup::Resource::Memory, 256 * 1024 * 1024, 192 * 1024 * 1024, crate::cgroup::Enforcement::Throttle);
            mgr.set_limit(root_id, crate::cgroup::Resource::CpuTime, 1_000_000, 800_000, crate::cgroup::Enforcement::Throttle);
            let shell_id = mgr.create("q-shell", 3, Some(root_id));
            mgr.set_limit(shell_id, crate::cgroup::Resource::Memory, 64 * 1024 * 1024, 48 * 1024 * 1024, crate::cgroup::Enforcement::Notify);
            mgr
        })),
        iommu: Mutex::new(alloc::boxed::Box::new(crate::iommu::Iommu::new())),
        elf_loader: Mutex::new(alloc::boxed::Box::new(crate::elf::ElfLoader::new())),
        rng: Mutex::new(crate::rng::HardwareRng::init()),
        sandbox: Mutex::new(crate::sandbox::SandboxManager::new()),
        gpu_sched: Mutex::new(alloc::boxed::Box::new({
            let mut g = crate::gpu_sched::GpuScheduler::new(4 * 1024 * 1024 * 1024); // 4 GiB VRAM
            g.set_budget(1, 2 * 1024 * 1024 * 1024, 64); // system: 2 GiB, 64 tasks
            g.set_budget(3, 1 * 1024 * 1024 * 1024, 32); // q-shell silo: 1 GiB, 32 tasks
            g
        })),
        npu_sched: Mutex::new(alloc::boxed::Box::new({
            let mut n = crate::npu_sched::NpuScheduler::new(512 * 1024 * 1024); // 512 MiB cache
            n.add_core(0);
            n.add_core(1);
            n
        })),
        disk_sched: Mutex::new(alloc::boxed::Box::new({
            let mut d = crate::disk_sched::DiskScheduler::new();
            d.set_share(1, 100); // system: weight 100
            d.set_share(3, 50); // q-shell: weight 50
            d
        })),
        dump_mgr: Mutex::new(alloc::boxed::Box::new(crate::coredump::DumpManager::new(Default::default()))),
        ledger: Mutex::new(crate::qledger::QLedger::new()),
        admin: Mutex::new(crate::q_admin::QAdmin::new()),
        quota: Mutex::new({
            let mut q = crate::qquota::QuotaManager::new();
            q.create_silo(1, None);
            q.set(1, crate::qquota::Resource::MemoryBytes, 4 * 1024 * 1024 * 1024, 8 * 1024 * 1024 * 1024);
            q.create_silo(3, Some(1));
            q.set(3, crate::qquota::Resource::MemoryBytes, 512 * 1024 * 1024, 1024 * 1024 * 1024);
            q
        }),
        snapshot: Mutex::new(crate::silo_snapshot::SnapshotManager::new()),
        timer_wheel: Mutex::new(crate::timer_wheel::TimerWheel::new(1_000_000)), // 1ms ticks
        hotplug: Mutex::new(crate::hotplug::HotplugManager::new()),
        irq_balancer: Mutex::new({
            let mut b = crate::irq_balance::IrqBalancer::new(4); // 4 cores
            b.set_silo_affinity(1, alloc::vec![0, 1, 2, 3]);
            b.set_silo_affinity(3, alloc::vec![2, 3]);
            b
        }),
        page_cache: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: page_cache...");
            let mut pc = crate::page_cache::PageCache::new(65536); // 64K pages = 256MB
            pc.set_pool(1, 32768);
            pc.set_pool(3, 8192);
            pc
        })),
        cpu_freq: Mutex::new({
            crate::serial_println!("INIT: cpu_freq...");
            crate::cpu_freq::CpuFreqScaler::new(
                4, alloc::vec![800_000, 1_200_000, 2_000_000, 3_000_000, 4_000_000]
            )
        }),
        entropy_pool: Mutex::new({
            crate::serial_println!("INIT: entropy_pool...");
            let mut ep = crate::entropy_pool::EntropyPool::new();
            // Seed with boot-time entropy
            ep.mix(crate::entropy_pool::EntropySample {
                source: crate::entropy_pool::EntropySource::Hardware,
                data: [0x42; 32], // Boot seed
                entropy_bits: 256,
                timestamp: 0,
            });
            ep
        }),
        numa: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: numa...");
            let mut nm = crate::numa::NumaManager::new();
            nm.add_node(0, alloc::vec![0, 1], 4 * 1024 * 1024 * 1024); // 4GiB
            nm.add_node(1, alloc::vec![2, 3], 4 * 1024 * 1024 * 1024); // 4GiB
            nm.set_distance(0, 1, 20);
            nm.set_distance(1, 0, 20);
            nm
        })),
        mem_compress: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: mem_compress...");
            let mut mc = crate::mem_compress::MemCompress::new(512 * 1024 * 1024); // 512MB zpool
            mc.set_budget(1, 8192);
            mc.set_budget(3, 4096);
            mc
        })),
        hpet: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: hpet...");
            // Fake ACPI HPET at 0xFED00000, 100ns period, 3 timers
            crate::hpet::Hpet::new(0xFED0_0000, 100_000, 3)
        })),
        rtc: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: rtc...");
            crate::rtc::Rtc::new()
        })),
        tsc: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: tsc...");
            let mut t = crate::tsc::TscManager::new();
            t.set_reliability(crate::tsc::TscReliability::Invariant);
            t.calibrate(1_000_000_000, 3_000_000_000, 4); // 3 GHz
            t
        })),
        msr_stats: core::sync::atomic::AtomicU64::new(0),
        pci_devices: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: pci enumeration...");
            crate::drivers::pci::enumerate()
        })),
        msi_manager: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: msi manager...");
            crate::msi::MsiController::new(0x20, 256)
        })),
        smbios_inventory: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: smbios parser...");
            let mut parser = crate::smbios::SmbiosParser::new();
            unsafe { parser.parse_tables(0xF0000, 0x10000, (3, 0)); }
            parser.inventory
        })),
        usb_hci: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: usb host controller...");
            crate::usb_hci::UsbHci::new()
        })),
        virtio_gpu: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: virtio gpu...");
            crate::virtio_gpu::VirtioGpu::new(crate::virtio::VirtioDevice::new(0, 0, 0))
        })),
        dma: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: dma engine...");
            crate::dma_engine::DmaEngine::new()
        })),
        acpi: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: acpi parser...");
            let mut parser = crate::acpi::AcpiParser::new();
            // We parse a fake RSDP at 0xF0000 for QEMU emulation testing
            unsafe { parser.parse_rsdp(0xF0000); }
            parser
        })),
        pcm: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: pcm audio mixer...");
            crate::pcm_audio::PcmMixer::new(48000, 2)
        })),
        hotswap: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: hotswap engine...");
            crate::hotswap::HotSwapEngine::new()
        })),
        kprobe: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: kprobe manager...");
            crate::kprobe::KProbeManager::new()
        })),
        fault_inject: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: fault injector...");
            crate::fault_inject::FaultInjector::new()
        })),
        kdump: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: kdump analyzer...");
            crate::kdump::KDump::new()
        })),
        rcu: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: rcu manager...");
            crate::rcu::RcuManager::new()
        })),
        genesis: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: genesis protocol...");
            crate::genesis::GenesisProtocol::default()
        })),

        numalloc: Mutex::new(alloc::boxed::Box::new({
            crate::serial_println!("INIT: numa allocator...");
            crate::memory::numa::NumaAllocator::new()
        })),
    });
    crate::serial_println!("INIT: kstate init complete!");
}

/// Get a reference to the global kernel state.
///
/// # Panics
/// Panics if called before `init()`.
pub fn state() -> &'static KernelState {
    KERNEL.get().expect("Kernel state not initialized")
}

/// Convenience: lock the silo manager.
pub fn silos() -> spin::MutexGuard<'static, SiloManager> {
    state().silo_mgr.lock()
}

/// Convenience: lock the IPC manager.
pub fn ipc() -> spin::MutexGuard<'static, IpcManager> {
    state().ipc_mgr.lock()
}

/// Convenience: lock the audit log.
pub fn audit() -> spin::MutexGuard<'static, AuditLog> {
    state().audit.lock()
}

/// Convenience: lock the Nexus mesh engine.
pub fn nexus() -> spin::MutexGuard<'static, nexus::QNexus> {
    state().nexus.lock()
}

/// Convenience: lock the Synapse BCI engine.
pub fn synapse() -> spin::MutexGuard<'static, synapse::QSynapse> {
    state().synapse.lock()
}

/// Convenience: lock the Chimera Translation Layer.
pub fn chimera() -> spin::MutexGuard<'static, chimera::ChimeraSilo> {
    state().chimera.lock()
}

/// Convenience: lock the NVMe controller.
pub fn nvme() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::drivers::nvme::NvmeController>> {
    state().nvme.lock()
}

/// Convenience: lock the VirtIO-Net device.
pub fn virtio_net() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::drivers::virtio_net::VirtioNet>> {
    state().virtio_net.lock()
}

/// Convenience: lock the HDA audio controller.
pub fn hda() -> spin::MutexGuard<'static, crate::drivers::audio_hda::HdaController> {
    state().hda.lock()
}

/// Convenience: lock the Telemetry Engine.
pub fn telemetry() -> spin::MutexGuard<'static, crate::telemetry::TelemetryEngine> {
    state().telemetry.lock()
}

/// Convenience: lock the PMC Monitor.
pub fn pmc() -> spin::MutexGuard<'static, crate::pmc::PmcMonitor> {
    state().pmc.lock()
}

/// Convenience: lock the Power Manager.
pub fn power_mgr() -> spin::MutexGuard<'static, crate::power_mgmt::PowerManager> {
    state().power_mgr.lock()
}

/// Convenience: lock the Power Governor.
pub fn power_gov() -> spin::MutexGuard<'static, crate::power_gov::PowerGovernor> {
    state().power_gov.lock()
}

/// Convenience: lock the Thermal Monitor.
pub fn thermal() -> spin::MutexGuard<'static, crate::thermal::ThermalMonitor> {
    state().thermal.lock()
}

/// Convenience: lock the Audit Logger.
pub fn audit_log() -> spin::MutexGuard<'static, crate::qaudit::AuditLog> {
    state().audit_log.lock()
}

/// Convenience: lock the Secure Boot engine.
pub fn secure_boot() -> spin::MutexGuard<'static, crate::secure_boot::SecureBoot> {
    state().secure_boot.lock()
}

/// Convenience: lock the CGroup Manager.
pub fn cgroup_mgr() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::cgroup::CGroupManager>> {
    state().cgroup_mgr.lock()
}

/// Convenience: lock the IOMMU Manager.
pub fn iommu() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::iommu::Iommu>> {
    state().iommu.lock()
}

/// Convenience: lock the ELF Loader.
pub fn elf_loader() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::elf::ElfLoader>> {
    state().elf_loader.lock()
}

/// Convenience: lock the Hardware RNG.
pub fn rng() -> spin::MutexGuard<'static, crate::rng::HardwareRng> {
    state().rng.lock()
}

/// Convenience: lock the Sandbox Manager.
pub fn sandbox() -> spin::MutexGuard<'static, crate::sandbox::SandboxManager> {
    state().sandbox.lock()
}

/// Convenience: lock the GPU Scheduler.
pub fn gpu_sched() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::gpu_sched::GpuScheduler>> {
    state().gpu_sched.lock()
}

/// Convenience: lock the NPU Scheduler.
pub fn npu_sched() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::npu_sched::NpuScheduler>> {
    state().npu_sched.lock()
}

/// Convenience: lock the Disk Scheduler.
pub fn disk_sched() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::disk_sched::DiskScheduler>> {
    state().disk_sched.lock()
}

/// Convenience: lock the Core Dump Manager.
pub fn dump_mgr() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::coredump::DumpManager>> {
    state().dump_mgr.lock()
}

/// Convenience: lock the Q-Ledger.
pub fn ledger() -> spin::MutexGuard<'static, crate::qledger::QLedger> {
    state().ledger.lock()
}

/// Convenience: lock the Q-Admin Manager.
pub fn admin() -> spin::MutexGuard<'static, crate::q_admin::QAdmin> {
    state().admin.lock()
}

/// Convenience: lock the Quota Manager.
pub fn quota() -> spin::MutexGuard<'static, crate::qquota::QuotaManager> {
    state().quota.lock()
}

/// Convenience: lock the Snapshot Manager.
pub fn snapshot() -> spin::MutexGuard<'static, crate::silo_snapshot::SnapshotManager> {
    state().snapshot.lock()
}

/// Convenience: lock the Timer Wheel.
pub fn timer_wheel() -> spin::MutexGuard<'static, crate::timer_wheel::TimerWheel> {
    state().timer_wheel.lock()
}

/// Convenience: lock the Hotplug Manager.
pub fn hotplug() -> spin::MutexGuard<'static, crate::hotplug::HotplugManager> {
    state().hotplug.lock()
}

/// Convenience: lock the IRQ Balancer.
pub fn irq_balancer() -> spin::MutexGuard<'static, crate::irq_balance::IrqBalancer> {
    state().irq_balancer.lock()
}

/// Convenience: lock the Page Cache.
pub fn page_cache() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::page_cache::PageCache>> {
    state().page_cache.lock()
}

/// Convenience: lock the CPU Frequency Scaler.
pub fn cpu_freq() -> spin::MutexGuard<'static, crate::cpu_freq::CpuFreqScaler> {
    state().cpu_freq.lock()
}

/// Convenience: lock the Entropy Pool.
pub fn entropy_pool() -> spin::MutexGuard<'static, crate::entropy_pool::EntropyPool> {
    state().entropy_pool.lock()
}

/// Convenience: lock the NUMA Manager.
pub fn numa() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::numa::NumaManager>> {
    state().numa.lock()
}

/// Convenience: lock the Memory Compressor.
pub fn mem_compress() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::mem_compress::MemCompress>> {
    state().mem_compress.lock()
}

/// Convenience: lock the HPET.
pub fn hpet() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::hpet::Hpet>> {
    state().hpet.lock()
}

/// Convenience: lock the RTC.
pub fn rtc() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::rtc::Rtc>> {
    state().rtc.lock()
}

/// Convenience: lock the TSC Manager.
pub fn tsc() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::tsc::TscManager>> {
    state().tsc.lock()
}

/// Convenience: track MSR accesses.
pub fn record_msr_access() {
    state().msr_stats.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

pub fn msr_stats() -> u64 {
    state().msr_stats.load(core::sync::atomic::Ordering::Relaxed)
}

/// Convenience: lock the PCI Device List.
pub fn pci_devices() -> spin::MutexGuard<'static, alloc::boxed::Box<alloc::vec::Vec<crate::drivers::pci::PciDevice>>> {
    state().pci_devices.lock()
}

/// Convenience: lock the MSI Interrupt Controller.
pub fn msi_manager() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::msi::MsiController>> {
    state().msi_manager.lock()
}

/// Convenience: lock the SMBIOS Hardware Inventory.
pub fn smbios() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::smbios::HardwareInventory>> {
    state().smbios_inventory.lock()
}

/// Convenience: lock the USB HCI.
pub fn usb_hci() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::usb_hci::UsbHci>> {
    state().usb_hci.lock()
}

/// Convenience: lock the VirtIO GPU Driver.
pub fn virtio_gpu() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::virtio_gpu::VirtioGpu>> {
    state().virtio_gpu.lock()
}

/// Convenience: lock the DMA Engine.
pub fn dma() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::dma_engine::DmaEngine>> {
    state().dma.lock()
}

/// Convenience: lock the ACPI Parser.
pub fn acpi() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::acpi::AcpiParser>> {
    state().acpi.lock()
}


/// Convenience: lock the KProbe Manager.
pub fn kprobe() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::kprobe::KProbeManager>> {
    state().kprobe.lock()
}

/// Convenience: lock the Fault Injection Engine.
pub fn fault_inject() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::fault_inject::FaultInjector>> {
    state().fault_inject.lock()
}

/// Convenience: lock the KDump Analyzer.
pub fn kdump() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::kdump::KDump>> {
    state().kdump.lock()
}

/// Convenience: lock the RCU Manager.
pub fn rcu() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::rcu::RcuManager>> {
    state().rcu.lock()
}

/// Convenience: lock the Genesis Protocol.
pub fn genesis() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::genesis::GenesisProtocol>> {
    state().genesis.lock()
}

/// Convenience: lock the NUMA Allocator.
pub fn numalloc() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::memory::numa::NumaAllocator>> {
    state().numalloc.lock()
}

/// Convenience: lock the PCM Audio Mixer.
pub fn pcm() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::pcm_audio::PcmMixer>> {
    state().pcm.lock()
}

/// Convenience: lock the Hot-Swap Engine.
pub fn hotswap() -> spin::MutexGuard<'static, alloc::boxed::Box<crate::hotswap::HotSwapEngine>> {
    state().hotswap.lock()
}

/// Global monotonic tick counter — incremented by the APIC timer IRQ (~1 per ms).
///
/// Used by the Sentinel to compute silo block durations for Law III enforcement.
static GLOBAL_TICK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Boot-complete flag — set to 1 after Phase 15 (kernel state finalized).
///
/// The APIC timer_handler reads this flag before accessing any global state
/// (SCHEDULERS, etc.) to prevent premature preemption during early boot when
/// heap allocators and spinlocks are still being initialized.
pub static BOOT_COMPLETE: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(false);

/// Signal that kernel boot is complete (Phase 15 done). After this point,
/// the APIC timer may trigger preemptive scheduling.
#[inline(always)]
pub fn signal_boot_complete() {
    BOOT_COMPLETE.store(true, core::sync::atomic::Ordering::Release);
}

/// Increment the global tick counter (called from the APIC timer IRQ handler).
#[inline(always)]
pub fn tick() {
    GLOBAL_TICK.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

/// Read the current global tick count (approximate milliseconds since boot).
#[inline(always)]
pub fn global_tick() -> u64 {
    GLOBAL_TICK.load(core::sync::atomic::Ordering::Relaxed)
}
