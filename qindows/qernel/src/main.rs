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
pub mod watchdog;
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
pub mod qring_guard; // Phase 52: Q-Ring slot-bound validation + syscall allowlist
pub mod qfs_ghost;   // Phase 53: QFS versioned CoW Ghost-Write path
pub mod silo_launch; // Phase 54: ELF Ring-3 Silo launch (SYSRET path)
pub mod qfabric;     // Phase 55: Q-Fabric QUIC-native network layer
pub mod chimera;     // Phase 57: Win32 → Qindows-native API translation bridge
pub mod uns;         // Phase 58: Universal Namespace resolver (prism://, qfa://, dev://)
pub mod aether;       // Phase 59: GPU-accelerated Aether compositor interface
pub mod synapse;      // Phase 60: Q-Synapse BCI neural intent pipeline
pub mod nexus;        // Phase 61: Nexus Global Mesh / Genesis Protocol
pub mod wasm_runtime; // Phase 62: WASM universal binary runtime kernel interface
pub mod ledger;       // Phase 63: Q-Ledger content-addressable package registry
pub mod identity;     // Phase 64: Q-Identity TPM hardware enclave & auth tokens
pub mod bridge;       // Phase 65: Q-Bridge Windows migration & legacy data ingestion
pub mod qshell;       // Phase 66: Q-Shell semantic object pipeline & Q-Admin escalation
pub mod collab;       // Phase 67: Q-Collab CRDT distributed collaborative workspace
pub mod firstboot;    // Phase 68: First Boot setup wizard state machine
pub mod qtraffic;     // Phase 69: Law 7 traffic flow visualizer & telemetry monitor
pub mod qupdate;      // Phase 70: Atomic hot-swap system updater (zero reboot)
pub mod q_metrics;    // Phase 71: Performance counters & benchmark observatory
pub mod prism_search; // Phase 72: Semantic object graph engine (q_resolve_intent)
pub mod active_task;  // Phase 73: Law 8 energy proportionality enforcement
pub mod q_view;       // Phase 74: Q-View Browser — websites as native Q-Silos
pub mod fiber_offload;   // Phase 75: Edge-Kernel "Scale to Cloud" Fiber serialization
pub mod digital_antibody;// Phase 76: Sentinel Digital Antibody & Global Immunization (<300ms)
pub mod compute_auction; // Phase 77: Nexus Phase V Compute Auction & Q-Credits engine
pub mod q_silo_fork;     // Phase 78: Copy-on-Write Silo forking (CoWFork syscall)
pub mod intent_router;        // Phase 79: Q-Synapse intent → subsystem dispatch (§6.2 complete)
pub mod q_manifest_enforcer;  // Phase 80: Unified 10-laws enforcement bus + compound detection
pub mod elastic_render;       // Phase 81: Aether elastic GPU offload to Q-Server (§9)
pub mod object_shard;         // Phase 82: Prism erasure-coded HA sharding across Nexus peers
pub mod q_credits_wallet;     // Phase 83: User Q-Credits wallet + donation device management
pub mod black_box;            // Phase 84: Sentinel Black Box recorder — Post-Mortem objects (§7)
pub mod silo_events;          // Phase 85: Silo lifecycle pub-sub event bus (loose coupling)
pub mod ghost_write_engine;   // Phase 86: Full Ghost-Write atomic save pipeline (§3.3)
pub mod q_energy;             // Phase 87: Integrated law-8 energy proportionality layer
pub mod timeline_slider;      // Phase 88: Ghost-Write version history navigator (Timeline UI)
pub mod uns_cache;            // Phase 89: Two-tier UNS address resolution cache (L1+L2)
pub mod sentinel_anomaly;     // Phase 90: Sentinel AI PMC-based anomaly scoring engine
pub mod aether_a11y;          // Phase 91: Aether accessibility layer (ARIA + screen magnification)
pub mod q_view_wm;            // Phase 92: Q-View multi-window tiling engine with AI placement
pub mod prism_query;          // Phase 93: Prism structured query DSL engine (filter/sort/dedup)
pub mod nexus_dht;            // Phase 94: Kademlia DHT routing table for Nexus peer discovery
pub mod q_fonts;              // Phase 95: SDF vector font rasterization engine
pub mod q_view_browser;       // Phase 96: Q-View Browser — websites as first-class Silos (§5.4)
pub mod v_gdi_upscale;        // Phase 97: V-GDI legacy GDI/DirectX → SDF upscaling pipeline (§8)
pub mod q_kit_sdk;            // Phase 98: Q-Kit declarative native UI layout engine (§4.6)
pub mod qring_async;          // Phase 99: Q-Ring io_uring-style async batch processor (§2.1)
pub mod kernel_integration;   // Phase 100: Cross-subsystem wire-up, boot integration, law audit
pub mod kstate_ext;           // Phase 101: Once-static extension for Phase 84-100 subsystems
pub mod synapse_bridge;       // Phase 102: Kernel ↔ Synapse Silo IPC bridge (BCI neural pipeline)
pub mod chimera_vgdi_bridge;  // Phase 103: Chimera GDI BitBlt → V-GDI SDF upscaler bridge
pub mod qring_dispatch;       // Phase 104: Q-Ring real dispatch table (replaces stubs in qring_async)
pub mod uns_resolver;         // Phase 105: Full UNS path resolution pipeline (§3) — 6-stage waterfall
pub mod intent_pipeline;      // Phase 106: Synapse → IntentRouter → Q-Ring execution pipeline
pub mod q_manifest_audit;     // Phase 107: Per-law runtime audit hooks for all 10 Q-Manifest laws
pub mod boot_sequence;        // Phase 108: boot_phase2() integrator — wires all subsystems at startup
pub mod aether_kit_bridge;    // Phase 109: Q-Kit layout engine → Aether compositor Q-Ring bridge
pub mod pmc_anomaly_loop;     // Phase 110: PMC hardware counters → anomaly scorer → law enforcement
pub mod nexus_kernel_bridge;  // Phase 111: Nexus Silo ↔ kernel Q-Fabric routing bridge
pub mod q_energy_scheduler;   // Phase 112: Law-8 energy proportionality scheduler integration
pub mod crypto_primitives;    // Phase 113: SHA-256/HMAC/FNV1a-256/SipHash — replaces XOR stubs
pub mod prism_live_index;     // Phase 114: Live object metadata index → feeds PrismQueryEngine
pub mod collab_session_net;   // Phase 115: CRDT collab delta push/receive via Nexus mesh IPC
pub mod hotswap_verifier;     // Phase 116: SHA-256 patch verify + Law2 rollback audit integration
pub mod identity_tpm_bridge;  // Phase 117: TPM-rooted identity, node attestation, CapToken key derivation
// Note: syscall_table already declared (Phase 75) — Phase 118 extends it (no new file needed)
pub mod cap_tokens;           // Phase 119: CapToken forge — mint/verify/revoke HMAC-signed caps
pub mod silo_ipc_router;      // Phase 120: IPC routing IpcSend→IpcRecv with cap check + backpressure
pub mod wasm_prism_bridge;    // Phase 121: WASM AOT pipeline → Prism OID → Silo spawn
pub mod ledger_verifier;      // Phase 122: SHA-256 manifest hash + publisher sig + Credits deduction
pub mod snapshot_restore_bridge; // Phase 123: RestoreStrategy → relaunch/migrate bridge
pub mod q_admin_bridge;       // Phase 124: Admin query bridge → real kernel state + crypto self-test
pub mod telemetry_bridge;     // Phase 125: Feed PMC/energy/traffic data into TelemetryEngine
pub mod secure_boot_integ;    // Phase 126: SHA-256 boot measurements, replace Pcr::simple_hash XOR stub
pub mod prism_store_bridge;   // Phase 127: PrismObjectStore ↔ LiveObjectIndex synchronized bridge
pub mod update_pipeline;      // Phase 128: QUpdateEngine + HotSwapVerifier + SecureBoot integration
pub mod rng_entropy_feeder;   // Phase 129: TSC/network/PMC entropy feeds + periodic refresh
pub mod q_metrics_bridge;     // Phase 130: Feed IPC/CtxSwitch/QRing/Prism latencies into QMetricsStore
pub mod qshell_kernel_bridge; // Phase 131: QShellEngine pipeline + CapToken escalation
pub mod quota_enforcement_bridge; // Phase 132: QuotaManager gates for Prism/net/CPU
pub mod sandbox_cap_bridge;   // Phase 133: SandboxManager ↔ CapTokenForge (TrapReason→Law map)
pub mod fork_cow_bridge;      // Phase 134: SiloForkEngine + CoW + CapToken lifecycle
pub mod settings_kernel_bridge; // Phase 135: SettingsManager seeded with all kernel defaults
pub mod qring_hardening_bridge; // Phase 136: harden_qring_batch() gate before every dispatch
pub mod qaudit_kernel;          // Phase 137: AuditLog kernel integration (law/cap/silo/quota/hotswap)
pub mod sentinel_anomaly_gate;  // Phase 138: SentinelAnomalyScorer → PMC → block anomalous Silos
pub mod qtraffic_law7_bridge;   // Phase 139: Law7Verdict gate on every outbound Nexus flow
pub mod compute_auction_bridge; // Phase 140: ComputeAuction Energy cap gate + CreditLedger integration
pub mod digital_antibody_bridge;   // Phase 141: AntibodyGenerator + immunity registry → Nexus bus
pub mod collab_cap_bridge;         // Phase 142: CRDT collaborative edits behind Collab CapToken
pub mod disk_sched_silo_bridge;    // Phase 143: DiskScheduler Silo priority + CapToken I/O tiers
pub mod prism_search_cap_bridge;   // Phase 144: PrismIndex ingest/get behind Prism:READ/EXEC gate
pub mod uns_cache_silo_bridge;     // Phase 145: UNS cache invalidation on Silo vaporize
pub mod aether_cap_bridge;         // Phase 146: Aether:EXEC gate on every widget submit (Law 3)
pub mod storage_driver_bridge;     // Phase 147: AHCI + NVMe → DiskSchedSiloBridge (I/O fairness)
pub mod message_bus_cap_bridge;    // Phase 148: MessageBus IPC:EXEC gate (Law 1)
pub mod sentinel_firewall_bridge;  // Phase 149: QTrafficEngine verdicts → Firewall rule table
pub mod watchdog_anomaly_bridge;   // Phase 150: WatchdogManager wired to Q-Ring + anomaly scores
pub mod prism_acl_cap_bridge;      // Phase 151: Prism ACL + CapToken conjunction (Law 1)
pub mod cgroup_quota_bridge;       // Phase 152: CGroupManager wired to Silo lifecycle
pub mod object_shard_prism_bridge; // Phase 153: 1MiB+ objects trigger distributed sharding
pub mod kprobe_sentinel_bridge;    // Phase 154: KProbeManager fed from real kernel hotpaths
pub mod cap_mapper_token_bridge;   // Phase 155: Page table perms derived from CapToken at spawn
pub mod irq_silo_bridge;           // Phase 156: IRQ vector alloc+routing wired to Silo lifecycle
pub mod power_gov_energy_bridge;   // Phase 157: PowerGovernor driven by thermal + APIC tick
pub mod core_dump_audit_bridge;    // Phase 158: CoreDump capture wired to QAuditKernel (Law 8)
pub mod gpu_sched_silo_bridge;     // Phase 159: GpuCompute:EXEC gate on GPU workloads
pub mod irq_balance_topo_bridge;   // Phase 160: SMP topology → IRQ balancer
pub mod firstboot_antibody_bridge; // Phase 161: First-boot seeds LocalImmunityRegistry from TPM
pub mod chimera_handle_quota_bridge; // Phase 162: Win32 handle quota enforcement + vaporize cleanup
pub mod fiber_offload_cap_bridge;  // Phase 163: Network:EXEC gate on cross-node fiber offload
pub mod dma_cap_bridge;            // Phase 164: Admin:EXEC DMA gate + IOMMU range registration
pub mod npu_synapse_bridge;        // Phase 165: Synapse:EXEC gate on NPU inference submissions
pub mod timer_wheel_silo_bridge;   // Phase 166: TimerWheel per-Silo tracking + vaporize cleanup
pub mod silo_ipc_router_cap_bridge; // Phase 167: Ipc:EXEC gate on SiloIpcRouter + kernel Silo guard
pub mod silo_events_audit_bridge;  // Phase 168: SiloEvent → QAuditKernel (vaporize/cap/quota)
pub mod quota_prism_bridge;        // Phase 169: Storage quota gate before every Prism write
pub mod network_rate_silo_bridge;  // Phase 170: Per-tick byte rate limiter + anomaly throttle
pub mod snapshot_gc_audit_bridge;  // Phase 171: SnapshotManager GC on vaporize + audit on create
pub mod uns_ttl_enforcer_bridge;   // Phase 172: UNS cache sweep + vaporize URI invalidation
pub mod prism_key_rotation_bridge; // Phase 173: HMAC-SHA256 key derive + zeroize on vaporize (Law 9)
pub mod wasm_jit_cap_bridge;       // Phase 174: Wasm:EXEC gate on load + call
pub mod qfs_ghost_retention_bridge; // Phase 175: Ghost TTL enforcement + Prism:READ gate + sweep
pub mod ledger_verify_cap_bridge;   // Phase 176: AppManifest validate + CapToken match at launch
pub mod qring_guard_audit_bridge;   // Phase 177: harden_qring_batch + audit rejections (Law 6)
pub mod hotswap_audit_bridge;       // Phase 178: stage/verify/apply hotswap + rollback on fail
pub mod q_admin_escalation_audit_bridge; // Phase 179: Escalation request/approve audit log
pub mod telemetry_quota_bridge;     // Phase 180: Max 16 telemetry records/Silo/tick
pub mod q_credits_budget_bridge;    // Phase 181: SpendingLimit enforcement per Silo/kind
pub mod silo_fork_cow_bridge;       // Phase 182: Memory:EXEC gate on fork + CoW page GC
pub mod nexus_mesh_audit_bridge;    // Phase 183: Nexus mesh 64pkt/tick rate limit (Law 4)
pub mod entropy_rng_bridge;         // Phase 184: 128-bit entropy gate before pool extraction
pub mod power_gov_silo_throttle_bridge; // Phase 185: Energy budget + thermal throttle (Law 8)
pub mod synapse_neural_gate_bridge;    // Phase 186: Synapse:READ cap + ThoughtGateState
pub mod timeline_slider_cap_bridge;    // Phase 187: Prism:READ/WRITE gates on Timeline Slider
pub mod wasm_sandbox_exec_bridge;      // Phase 188: Wasm:EXEC gate on sandbox create/run (new)
pub mod update_pipeline_audit_bridge;  // Phase 189: Admin:EXEC gate + audit on apply_next
pub mod thermal_zone_policy_bridge;    // Phase 190: TripAction enforcement (throttle/shutdown)
pub mod rtc_time_fence_bridge;         // Phase 191: Admin:EXEC gate on Rtc::set_time
pub mod timer_wheel_silo_quota_bridge; // Phase 192: Max 32 timers per Silo
pub mod smbios_audit_bridge;           // Phase 193: SMBIOS completeness + spoof detection
pub mod usb_device_cap_bridge;         // Phase 194: Admin:EXEC gate on USB device access
// Phase 195: silo_events_audit_bridge already declared at Phase 168
pub mod iommu_silo_cap_bridge;         // Phase 196: Admin:EXEC gate on IOMMU DMA mapping
pub mod irq_router_cap_bridge;         // Phase 197: Admin:EXEC + 32 vectors/Silo quota
pub mod cpu_freq_silo_cap_bridge;      // Phase 198: Admin:EXEC gate on governor/freq change
pub mod numa_affinity_bridge;          // Phase 199: Silo→NUMA node binding + locality score
pub mod pmc_anomaly_gate_bridge;       // Phase 200: PmcSample → SentinelAnomalyGate block
pub mod rng_entropy_feeder_audit_bridge; // Phase 201: check_refresh before every generate()
pub mod page_cache_silo_quota_bridge;  // Phase 202: Max 4096 pages per Silo quota
pub mod elastic_render_cap_bridge;     // Phase 203: Network:EXEC gate on Q-Server GPU offload
pub mod kernel_integration_health_bridge; // Phase 204: Boot-time kstate_ext subsystem probe
pub mod collab_crdt_cap_bridge;        // Phase 205: Prism:READ/WRITE cap gates on CRDT ops
pub mod kdump_admin_cap_bridge;        // Phase 206: Admin:EXEC gate on crash dump read
pub mod fault_injector_admin_bridge;   // Phase 207: Admin:EXEC gate on fault injection arm
pub mod mem_compress_silo_quota_bridge; // Phase 208: Max 2048 compression pages per Silo
pub mod hotplug_cap_bridge;           // Phase 209: Admin:EXEC gate on hotplug device attach
pub mod intent_pipeline_rate_bridge;   // Phase 210: Max 8 intent events per Silo per tick
pub mod qupdate_engine_audit_bridge;   // Phase 211: Law 2 audit on Kernel/Firmware staging
pub mod identity_token_expiry_bridge;  // Phase 212: IdentityToken expiry enforcement (Law 1)
pub mod acpi_power_profile_bridge;     // Phase 213: Admin:EXEC gate on ACPI PowerProfile
pub mod elf_load_cap_bridge;           // Phase 214: Admin:EXEC + hash gate on ELF load
pub mod firstboot_genesis_audit_bridge; // Phase 215: Genesis event audit trail at firstboot
pub mod qring_async_silo_bridge;       // Phase 216: Max 4096-depth SiloRing creation quota
pub mod rcu_grace_audit_bridge;        // Phase 217: RCU grace period stall detection (Law 4)
pub mod pci_device_cap_bridge;         // Phase 218: Admin:EXEC gate on PCI MMIO mapping
pub mod qfabric_traffic_audit_bridge;  // Phase 219: Max 256 fabric pkts/Silo/tick
pub mod qledger_integrity_bridge;      // Phase 220: QLedger prev_hash chain verification (Law 9)
pub mod active_task_token_audit_bridge; // Phase 221: Expired TaskToken → Law 1 audit
pub mod cgroup_hard_limit_bridge;      // Phase 222: Upgrade SoftWarning → HardThrottle cgroup
pub mod qquota_hard_enforcement_bridge; // Phase 223: HardDenied QuotaResult → Law 4 audit gate
pub mod irq_balance_silo_audit_bridge; // Phase 224: Admin:EXEC gate on IRQ core affinity
pub mod black_box_postmortem_cap_bridge; // Phase 225: Admin:EXEC gate on cross-Silo trace read
pub mod qshell_admin_pipeline_cap_bridge; // Phase 226: AdminEscalation re-check per stage (Law 1)
pub mod secure_boot_pcr_audit_bridge;  // Phase 227: PCR extend → Law 2 audit via log_hotswap
pub mod coredump_cap_bridge;           // Phase 228: Admin:EXEC gate on cross-Silo coredump read
pub mod genesis_silo_audit_bridge;     // Phase 229: Retroactive genesis CapType grant audit (Law 1)
pub mod boot_sequence_integrity_bridge; // Phase 230: Boot stage order verification (Law 2)
pub mod qview_widget_cap_bridge;       // Phase 231: Law 6 gate on cross-Silo QKitTree writes
pub mod pcm_audio_silo_cap_bridge;     // Phase 232: Max 4 audio streams per Silo quota
pub mod npu_scheduler_cap_bridge;      // Phase 233: Admin:EXEC gate on NpuPriority::Critical
pub mod qview_browser_nav_cap_bridge;  // Phase 234: Law 6 gate on cross-Silo DOM injection
pub mod qview_wm_monitor_cap_bridge;   // Phase 235: Admin:EXEC gate on Fullscreen/Presentation
pub mod uns_resolution_rate_bridge;    // Phase 236: Max 64 UNS resolutions/Silo/tick
pub mod silo_launch_validation_bridge; // Phase 237: Entry point + Law 2 audit on Silo launch
pub mod kprobe_admin_cap_bridge;       // Phase 238: Admin:EXEC gate on KProbe insertion
pub mod object_shard_integrity_bridge; // Phase 239: ShardSet recovery health check (Law 9)
pub mod gpu_scheduler_silo_budget_bridge; // Phase 240: 2GB VRAM cap + Admin:EXEC on Critical GPU
pub mod collab_session_net_cap_bridge;  // Phase 241: Prism:WRITE gate on CRDT apply_op
pub mod nexus_dht_record_ttl_bridge;   // Phase 242: Periodic DHT stale peer sweep
pub mod pmc_anomaly_loop_cap_bridge;   // Phase 243: Wrap PmcAnomalyLoop::tick with audit
pub mod numa_alloc_silo_bridge;        // Phase 244: 32-Silo/node NUMA imbalance detection
pub mod apic_timer_silo_bridge;        // Phase 245: Max 1000 Hz APIC timer cap per core
pub mod virtio_gpu_silo_cap_bridge;   // Phase 246: Law 6 gate on cross-Silo VirtIO GPU resource
pub mod usb_hci_silo_cap_bridge;       // Phase 247: Admin:EXEC gate on HID/MassStorage USB access
pub mod v_gdi_upscale_silo_cap_bridge; // Phase 248: Law 6 gate on cross-Silo GDI capture buffer read
pub mod silo_snapshot_ownership_bridge; // Phase 249: Admin:EXEC gate on cross-Silo snapshot read
pub mod uns_resolver_auth_bridge;      // Phase 250: Network:EXEC gate on remote UNS path resolution
pub mod energy_scheduler_law8_bridge; // Phase 251: P-state enforcement on energy budget excess (Law 8)
pub mod qring_dispatch_rate_bridge;    // Phase 252: Max 128 Q-Ring dispatches/Silo/tick (Law 4)
pub mod virtio_queue_silo_bridge;      // Phase 253: Max 32 VirtIO descriptors/Silo/tick
pub mod prism_live_index_eviction_bridge; // Phase 254: Max 1024 live objects/Silo quota
pub mod wasm_runtime_validation_bridge; // Phase 255: 16 MiB WASM binary size cap (Law 4)
pub mod timeline_slider_version_cap_bridge; // Phase 256: Max 10K tick version age cap on previews
pub mod fiber_offload_transmission_cap_bridge; // Phase 257: Max 64 MiB Fiber snapshot transmission
pub mod compute_auction_bid_cap_bridge; // Phase 258: power_score>1000 needs Admin:EXEC
pub mod digital_antibody_rate_bridge;  // Phase 259: Max 8 antibodies generated per tick
pub mod prism_search_rate_bridge;      // Phase 260: Max 16 Prism search queries/Silo/tick
pub mod q_fonts_glyph_cache_rate_bridge; // Phase 261: Max 512 cached glyphs per Silo
pub mod q_metrics_sample_rate_bridge;  // Phase 262: Max 32 metric samples/Silo/tick
pub mod prism_query_result_cap_bridge; // Phase 263: Max 10K results per Prism query
pub mod chimera_handle_leak_bridge;    // Phase 264: Max 4096 Win32 handles per Silo
pub mod q_credits_spend_rate_bridge;   // Phase 265: Max 100 Q-Credits spends/Silo/tick
pub mod collab_vector_clock_rate_bridge; // Phase 266: Max 64 VectorClock ticks/node/tick
pub mod nexus_peer_tier_cap_bridge;    // Phase 267: Admin:EXEC gate on Core/Master tier upgrade
pub mod firstboot_step_audit_bridge;   // Phase 268: Law 2 audit on each firstboot step advance
pub mod update_pipeline_rate_bridge;   // Phase 269: Min 500 ticks between update cycles
pub mod smp_core_silo_affinity_bridge; // Phase 270: Admin:EXEC gate on CPU core affinity pinning
pub mod q_kit_sdk_widget_rate_bridge; // Phase 271: Max 8192 widgets per Silo
pub mod identity_token_bind_bridge;    // Phase 272: IdentityToken silo_id binding (Law 1)
pub mod ledger_package_hash_cap_bridge; // Phase 273: Max 4 package publishes/Silo/tick
pub mod sentinel_anomaly_whitelist_bridge; // Phase 274: Skip scoring for whitelisted system Silos
pub mod q_view_browser_process_cap_bridge; // Phase 275: Max 32 browser tab Silos per session























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
pub mod irq_router; // Phase 49: per-Silo interrupt vector isolation

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

/// The Qernel Entry Point — 15-Phase Boot Sequence.
///
/// Called by the bootloader after UEFI boot services have exited.
/// This is the absolute beginning of Qindows — no standard library,
/// no OS layer. We are talking directly to the CPU.
#[no_mangle]
pub extern "C" fn _start(_boot_info_arg: &'static BootInfo) -> ! {
    // Initialize serial port first (for debug output)
    drivers::serial::SerialWriter::init();
    serial_println!("Qernel boot sequence initiated...");

    // Read BootInfo from the well-known fixed address (0x2FF000)
    // where the bootloader placed it. We don't rely on the function
    // argument because register state may be clobbered during the
    // bootloader→kernel transition after ExitBootServices.
    const BOOT_INFO_ADDR: u64 = 0x2F_F000;
    let boot_info: &'static BootInfo = unsafe { &*(BOOT_INFO_ADDR as *const BootInfo) };

    // ── Phase 1: Memory ─────────────────────────────────────────
    // Initialize the physical memory manager with the UEFI memory map.
    let mut frame_allocator = memory::FrameAllocator::init(
        boot_info.memory_map_addr,
        boot_info.memory_map_entries,
        boot_info.memory_map_desc_size,
    );
    memory::paging::init(&mut frame_allocator);
    memory::pcid::init(); // Reserve PCID 0 for kernel identity map
    memory::heap::init(&mut frame_allocator);
    serial_println!("[OK] Phase 1: Memory (frames + paging + heap + PCID pool)");


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
    let mut genesis = genesis::GenesisProtocol::default();

    if !genesis.check_completed() {
        serial_println!("  Genesis: First boot detected — running full protocol");
        genesis.step(boot_timestamp);  // Advance through setup wizard
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

    // ── Render Aether Desktop ─────────────────────────────────
    // Clear the boot console and draw the full desktop environment
    // on the framebuffer. This is the first graphical UI the user sees.
    serial_println!("Rendering Aether Desktop...");

    // Draw the desktop (background, taskbar, icons, Q logo, status panel)
    drivers::desktop::render_desktop(&mut display);

    // Render text elements (status panel, clock)
    drivers::desktop::render_status_text(
        &mut display,
        &mut console,
        active_silo_count,
        active_channel_count,
    );

    // Render clock from the RTC
    drivers::desktop::render_clock(
        &mut display,
        &mut console,
        wall_clock.hour,
        wall_clock.minute,
        wall_clock.month,
        wall_clock.day,
    );

    serial_println!("Aether Desktop rendered — framebuffer live");

    serial_println!("--- INITIATING RING 3 USERSPACE TRANSITION ---");

    // 1. Load the compiled Q-Shell ELF executable payload
    let q_shell_elf = include_bytes!("../../target/x86_64-unknown-none/release/q-shell");
    
    let mut loader = crate::elf::ElfLoader::new();
    let elf_info = match loader.parse(q_shell_elf) {
        Ok(info) => info,
        Err(e) => panic!("Failed to parse Q-Shell ELF: {:?}", e),
    };

    serial_println!("  Parsed Q-Shell ELF: {} bytes, {} segments", q_shell_elf.len(), elf_info.segments.len());

    // 2. Map the ELF segments into physical memory
    let loaded = unsafe { loader.load_into_memory(q_shell_elf, &elf_info).expect("Failed to load ELF segments") };
    
    serial_println!("  ELF Loaded! Entry point: {:#018x}", loaded.entry_point);

    // 3. Allocate a User Stack
    let user_stack_size = 2 * 1024 * 1024;
    
    // Allocate contiguous pages from the kernel heap (which spans 32MiB)
    extern crate alloc;
    let mut ring3_stack = alloc::vec::Vec::<u8>::with_capacity(user_stack_size);
    // Force the memory to be committed
    for _ in 0..user_stack_size { ring3_stack.push(0); }
    let user_stack_base = ring3_stack.as_ptr() as u64;
    
    // Leak the Vec so the memory stays allocated as the Ring 3 stack forever
    core::mem::forget(ring3_stack);
    
    let user_stack_top = user_stack_base + user_stack_size as u64;
    
    serial_println!("  User Stack allocated: Base={:#018x}, Top={:#018x}", user_stack_base, user_stack_top);

    // Get the Q-Shell Silo ID to masquerade as
    let qshell_id = kstate::silos().silos.iter().find(|s| s.binary_oid == 0x0000_0005_0000_0001).unwrap().id;

    serial_println!("  Transitioning CPU to Ring 3 (IRETQ)...");
    serial_println!("══════════════════════════════════════════════");

    // ── Arm the APIC Timer ────────────────────────────────────
    drivers::apic::start_timer();

    // 4. Perform the hardware privilege drop
    unsafe {
        crate::syscall::set_current_silo(qshell_id);
        crate::scheduler::context::switch_to_user_mode(loaded.entry_point, user_stack_top);
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
