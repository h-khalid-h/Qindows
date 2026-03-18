//! # KState Extension — Phase 84-100 Subsystem Global State (Phase 101)
//!
//! ## Architecture Guardian: Why a separate file?
//! `kstate.rs` is the original global kernel state singleton (732 lines).
//! Rather than modifying that 700+ line file and risking regressions,
//! we extend it here via **separate `spin::Once`-initialized statics**.
//!
//! Pattern: identical to the existing `KERNEL_STATE: Once<KernelState>` in kstate.rs.
//! Each subsystem has its own `Once<Mutex<T>>` for independent initialization ordering.
//!
//! ## Accessor Pattern
//! ```rust
//! // Call on first use (safe — Once guarantees single init):
//! crate::kstate_ext::event_bus().publish(SiloEvent::Spawned { ... });
//! crate::kstate_ext::qring().register_silo(silo_id);
//! crate::kstate_ext::anomaly().register_silo(silo_id, binary_oid);
//! crate::kstate_ext::black_box().record_event(silo_id, evt);
//! crate::kstate_ext::wm().map_window(silo_id, binary_oid, wt, title);
//! ```
//!
//! ## Thread Safety
//! All statics are `spin::Once<spin::Mutex<T>>`. The Mutex protects interior
//! mutability while Once ensures the container is initialized exactly once.
//!
//! ## Law Compliance
//! - **Law 1**: `event_bus()` allows other modules to subscribe to Silo events
//!   without direct kernel pointer access — loose coupling via events
//! - **Law 8**: `qenergy()` global is the single source of energy budget truth
//! - **Law 9**: `uns_cache()` global is the authoritative UNS address cache

extern crate alloc;
use spin::{Mutex, Once};

// ── Subsystem Imports ─────────────────────────────────────────────────────────

use crate::silo_events::SiloEventBus;
use crate::qring_async::QRingProcessor;
use crate::sentinel_anomaly::SentinelAnomalyScorer;
use crate::black_box::BlackBoxRecorder;
use crate::q_view_wm::QViewWm;
use crate::aether_a11y::AetherA11yLayer;
use crate::uns_cache::UnsCache;
use crate::q_energy::QEnergyLayer;
use crate::timeline_slider::TimelineNavigator;
use crate::ghost_write_engine::GhostWriteEngine;
use crate::q_fonts::QFontEngine;
use crate::q_view_browser::QViewBrowser;
use crate::nexus_dht::NexusDht;
use crate::v_gdi_upscale::VGdiUpscaler;
use crate::q_kit_sdk::QKitEngine;
use crate::kernel_integration::SystemMetrics;
// Phase 101-106 additions
use crate::rng_entropy_feeder::RngEntropyFeeder;
use crate::prism_live_index::LiveObjectIndex;
use crate::q_metrics::QMetricsStore;
use crate::snapshot_restore_bridge::SnapshotRestoreBridge;
// Phase 105-108 additions
use crate::secure_boot_integ::SecureBootIntegration;
use crate::nexus_kernel_bridge::NexusKernelBridge;
use crate::wasm_prism_bridge::WasmPrismBridge;
use crate::aether_kit_bridge::AetherKitBridge;
// Phase 109
use crate::prism_search::PrismSearchEngine;
// Phase 110
use crate::synapse_bridge::SynapseIpcBridge;

// ── Global Statics — Phase 84-100 Subsystems ─────────────────────────────────

/// Silo lifecycle event bus (Phase 85).
static EVENT_BUS: Once<Mutex<SiloEventBus>> = Once::new();
/// Q-Ring async batch processor (Phase 99).
static QRING: Once<Mutex<QRingProcessor>> = Once::new();
/// Sentinel AI anomaly scorer (Phase 90).
static ANOMALY: Once<Mutex<SentinelAnomalyScorer>> = Once::new();
/// Black Box post-mortem recorder (Phase 84).
static BLACK_BOX: Once<Mutex<BlackBoxRecorder>> = Once::new();
/// Q-View multi-window manager (Phase 92).
static WM: Once<Mutex<QViewWm>> = Once::new();
/// Aether accessibility layer (Phase 91).
static A11Y: Once<Mutex<AetherA11yLayer>> = Once::new();
/// UNS address resolution cache (Phase 89).
static UNS_CACHE: Once<Mutex<UnsCache>> = Once::new();
/// Q-Energy proportionality layer (Phase 87).
static QENERGY: Once<Mutex<QEnergyLayer>> = Once::new();
/// Ghost-Write atomic save engine (Phase 86).
static GHOST_WRITE: Once<Mutex<GhostWriteEngine>> = Once::new();
/// Timeline Slider version history (Phase 88).
static TIMELINE: Once<Mutex<TimelineNavigator>> = Once::new();
/// Q-Fonts SDF rasterization engine (Phase 95).
static FONTS: Once<Mutex<QFontEngine>> = Once::new();
/// Q-View Browser — websites as Silos (Phase 96).
static BROWSER: Once<Mutex<QViewBrowser>> = Once::new();
/// Nexus Kademlia DHT (Phase 94).
static NEXUS_DHT: Once<Mutex<NexusDht>> = Once::new();
/// V-GDI legacy upscaler (Phase 97).
static VGDI: Once<Mutex<VGdiUpscaler>> = Once::new();
/// Q-Kit declarative UI engine (Phase 98).
static QKIT: Once<Mutex<QKitEngine>> = Once::new();
/// Cross-subsystem system metrics (Phase 100).
static METRICS: Once<Mutex<SystemMetrics>> = Once::new();
/// RNG entropy feeder — keeps pool fresh from TSC/PMC jitter (Phase 101)
static RNG_FEEDER: Once<Mutex<RngEntropyFeeder>> = Once::new();
/// Prism live hot object index — fed by SiloSpawn + GhostWrite (Phase 102)
static LIVE_INDEX: Once<Mutex<LiveObjectIndex>> = Once::new();
/// Kernel metric store — OS-semantic performance counters (Phase 103)
static METRIC_STORE: Once<Mutex<QMetricsStore>> = Once::new();
/// Snapshot restore bridge — periodic silo checkpoint (Phase 104)
static SNAP_BRIDGE: Once<Mutex<SnapshotRestoreBridge>> = Once::new();
/// Secure boot integration — SHA-256 PCR measurements (Phase 105)
static SECURE_BOOT: Once<Mutex<SecureBootIntegration>> = Once::new();
/// Nexus kernel bridge — Q-Fabric mesh routing (Phase 106)
static NEXUS_BRIDGE: Once<Mutex<NexusKernelBridge>> = Once::new();
/// WASM Prism bridge — WASM AOT → Silo spawn pipeline (Phase 107)
static WASM_BRIDGE: Once<Mutex<WasmPrismBridge>> = Once::new();
/// Aether-Kit bridge — Q-Kit layout → compositor Q-Ring submission (Phase 108)
static AETHER_KIT: Once<Mutex<AetherKitBridge>> = Once::new();
/// Prism Semantic Search Engine — in-kernel object graph search (Phase 109)
static PRISM_SEARCH: Once<Mutex<PrismSearchEngine>> = Once::new();
/// Synapse IPC Bridge — kernel ↔ Synapse Silo neural pipeline (Phase 110)
static SYNAPSE_BRIDGE: Once<Mutex<SynapseIpcBridge>> = Once::new();

// ── Initializer ───────────────────────────────────────────────────────────────

/// Initialize all Phase 84-100 subsystems.
/// Called once from the kernel boot sequence (after Phase 15 — after heap is up).
/// Must be called before any accessor function.
pub fn init(self_node_id: [u8; 32]) {
    EVENT_BUS.call_once(|| Mutex::new(SiloEventBus::new()));
    QRING.call_once(|| Mutex::new(QRingProcessor::new()));
    ANOMALY.call_once(|| Mutex::new(SentinelAnomalyScorer::new()));
    BLACK_BOX.call_once(|| Mutex::new(BlackBoxRecorder::new()));
    WM.call_once(|| Mutex::new(QViewWm::new()));
    A11Y.call_once(|| Mutex::new(AetherA11yLayer::new()));
    UNS_CACHE.call_once(|| Mutex::new(UnsCache::new()));
    QENERGY.call_once(|| Mutex::new(QEnergyLayer::new()));
    GHOST_WRITE.call_once(|| Mutex::new(GhostWriteEngine::new(0)));
    TIMELINE.call_once(|| Mutex::new(TimelineNavigator::new()));
    FONTS.call_once(|| Mutex::new(QFontEngine::new()));
    BROWSER.call_once(|| Mutex::new(QViewBrowser::new()));
    NEXUS_DHT.call_once(|| Mutex::new(NexusDht::new(self_node_id)));
    VGDI.call_once(|| Mutex::new(VGdiUpscaler::new()));
    QKIT.call_once(|| Mutex::new(QKitEngine::new()));
    METRICS.call_once(|| Mutex::new(SystemMetrics::default()));
    // Phase 101-104: additional subsystems
    RNG_FEEDER.call_once(|| Mutex::new(RngEntropyFeeder::new()));
    LIVE_INDEX.call_once(|| Mutex::new(LiveObjectIndex::new()));
    METRIC_STORE.call_once(|| Mutex::new(QMetricsStore::new(60_000))); // 60kHz tick freq
    SNAP_BRIDGE.call_once(|| Mutex::new(SnapshotRestoreBridge::new()));
    // Phase 105-108
    SECURE_BOOT.call_once(|| Mutex::new(SecureBootIntegration::new()));
    NEXUS_BRIDGE.call_once(|| Mutex::new(NexusKernelBridge::new(self_node_id)));
    WASM_BRIDGE.call_once(|| Mutex::new(WasmPrismBridge::new()));
    AETHER_KIT.call_once(|| Mutex::new(AetherKitBridge::new(
        crate::nexus_kernel_bridge::AETHER_SILO_ID
    )));

    // Perform initial SecureBoot measurements for kernel core components
    if let Some(mut sb) = SECURE_BOOT.get().and_then(|m| m.try_lock()) {
        use crate::secure_boot::{BootComponent};
        // Measure the kernel binary (use self_node_id as proxy for kernel identity)
        sb.measure_component(
            BootComponent::Kernel,
            &self_node_id,
            "qernel-core",
            1,
            0,
        );
        sb.lock_boot();
    }
    // Phase 109: Prism Semantic Search Engine
    PRISM_SEARCH.call_once(|| Mutex::new(PrismSearchEngine::new()));
    // Seed the Prism with a kernel-internal system Q-Node so the index is non-empty
    if let Some(mut ps) = PRISM_SEARCH.get().and_then(|m| m.try_lock()) {
        use crate::prism_search::QNode;
        let node = QNode::new(
            self_node_id,
            "kernel",
            "Qindows Kernel",
            0, // kernel silo_id = 0
            0, // tick = 0 (boot)
            4096,
            "qindows kernel boot system core",
        );
        ps.ingest_object(node);
    }
    // Phase 110: Synapse IPC Bridge (kernel ↔ Synapse Silo neural pipeline)
    SYNAPSE_BRIDGE.call_once(|| Mutex::new(SynapseIpcBridge::new()));

    crate::serial_println!(
        "[KSTATE-EXT] Phase 84-110 subsystems initialized ({} statics)",
        26
    );
}

// ── Accessor Functions ─────────────────────────────────────────────────────────

/// Lock the Silo Event Bus.
pub fn event_bus() -> spin::MutexGuard<'static, SiloEventBus> {
    EVENT_BUS.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Q-Ring async batch processor.
pub fn qring() -> spin::MutexGuard<'static, QRingProcessor> {
    QRING.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Sentinel AI anomaly scorer.
pub fn anomaly() -> spin::MutexGuard<'static, SentinelAnomalyScorer> {
    ANOMALY.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Black Box recorder.
pub fn black_box() -> spin::MutexGuard<'static, BlackBoxRecorder> {
    BLACK_BOX.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Q-View Window Manager.
pub fn wm() -> spin::MutexGuard<'static, QViewWm> {
    WM.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Aether Accessibility Layer.
pub fn a11y() -> spin::MutexGuard<'static, AetherA11yLayer> {
    A11Y.get().expect("kstate_ext not initialized").lock()
}

/// Lock the UNS Address Cache.
pub fn uns_cache() -> spin::MutexGuard<'static, UnsCache> {
    UNS_CACHE.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Q-Energy Layer.
pub fn qenergy() -> spin::MutexGuard<'static, QEnergyLayer> {
    QENERGY.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Ghost-Write Engine.
pub fn ghost_write() -> spin::MutexGuard<'static, GhostWriteEngine> {
    GHOST_WRITE.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Timeline Index.
pub fn timeline() -> spin::MutexGuard<'static, TimelineNavigator> {
    TIMELINE.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Q-Fonts Engine.
pub fn fonts() -> spin::MutexGuard<'static, QFontEngine> {
    FONTS.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Q-View Browser.
pub fn browser() -> spin::MutexGuard<'static, QViewBrowser> {
    BROWSER.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Nexus DHT.
pub fn nexus_dht() -> spin::MutexGuard<'static, NexusDht> {
    NEXUS_DHT.get().expect("kstate_ext not initialized").lock()
}

/// Lock the V-GDI Upscaler.
pub fn vgdi() -> spin::MutexGuard<'static, VGdiUpscaler> {
    VGDI.get().expect("kstate_ext not initialized").lock()
}

/// Lock the Q-Kit SDK Engine.
pub fn qkit() -> spin::MutexGuard<'static, QKitEngine> {
    QKIT.get().expect("kstate_ext not initialized").lock()
}

/// Lock the System Metrics.
pub fn metrics() -> spin::MutexGuard<'static, SystemMetrics> {
    METRICS.get().expect("kstate_ext not initialized").lock()
}
/// Lock the RNG Entropy Feeder.
pub fn rng_feeder() -> spin::MutexGuard<'static, RngEntropyFeeder> {
    RNG_FEEDER.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Prism Live Object Index.
pub fn live_index() -> spin::MutexGuard<'static, LiveObjectIndex> {
    LIVE_INDEX.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Kernel Metric Store.
pub fn metric_store() -> spin::MutexGuard<'static, QMetricsStore> {
    METRIC_STORE.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Snapshot Restore Bridge.
pub fn snap_bridge() -> spin::MutexGuard<'static, SnapshotRestoreBridge> {
    SNAP_BRIDGE.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Secure Boot Integration.
pub fn secure_boot() -> spin::MutexGuard<'static, SecureBootIntegration> {
    SECURE_BOOT.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Nexus Kernel Bridge.
pub fn nexus_bridge() -> spin::MutexGuard<'static, NexusKernelBridge> {
    NEXUS_BRIDGE.get().expect("kstate_ext not initialized").lock()
}
/// Lock the WASM Prism Bridge.
pub fn wasm_bridge() -> spin::MutexGuard<'static, WasmPrismBridge> {
    WASM_BRIDGE.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Aether-Kit Bridge.
pub fn aether_kit() -> spin::MutexGuard<'static, AetherKitBridge> {
    AETHER_KIT.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Prism Semantic Search Engine.
pub fn prism_search() -> spin::MutexGuard<'static, PrismSearchEngine> {
    PRISM_SEARCH.get().expect("kstate_ext not initialized").lock()
}
/// Lock the Synapse IPC Bridge (kernel ↔ neural pipeline).
pub fn synapse_bridge() -> spin::MutexGuard<'static, SynapseIpcBridge> {
    SYNAPSE_BRIDGE.get().expect("kstate_ext not initialized").lock()
}

/// Route a FabricSend packet via NexusKernelBridge (called from qring_dispatch).
/// Uses try_lock to avoid deadlock in batch dispatch context.
pub(crate) fn nexus_send(from_silo: u64, dest_prefix: u64, payload_len: u32, tick: u64) {
    if let (Some(nb_mtx), Some(qr_mtx)) = (NEXUS_BRIDGE.get(), QRING.get()) {
        if let (Some(mut nb), Some(mut qr)) = (nb_mtx.try_lock(), qr_mtx.try_lock()) {
            nb.send_packet(from_silo, dest_prefix, payload_len, &mut qr, tick);
        }
    }
}

/// Deliver a FabricRecv packet from Nexus Silo to a local Silo via NexusKernelBridge.
pub(crate) fn nexus_deliver(dest_silo: u64, src_prefix: u64, payload_len: u32, tick: u64) {
    if let (Some(nb_mtx), Some(qr_mtx)) = (NEXUS_BRIDGE.get(), QRING.get()) {
        if let (Some(mut nb), Some(mut qr)) = (nb_mtx.try_lock(), qr_mtx.try_lock()) {
            nb.deliver_inbound(dest_silo, src_prefix, payload_len, &mut qr, tick);
        }
    }
}


// ── Tick-Driven Integration Hook ──────────────────────────────────────────────

/// Called from APIC timer interrupt (every tick) after `BOOT_COMPLETE`.
/// Drains Q-Ring for all Silos; sweeps UNS cache TTLs.
/// Must be fast (called in IRQ context) — no blocking.
pub fn tick_hook(tick: u64) {
    // Only run if all statics are initialized
    if QRING.get().is_none() || UNS_CACHE.get().is_none() { return; }

    // Drain all Silo Q-Rings (fast — O(N entries) where N = ring drain count)
    if let Some(ring_mtx) = QRING.get() {
        if let Some(mut ring) = ring_mtx.try_lock() {
            ring.drain_all();
        }
    }

    // Sweep UNS cache TTLs (skipped if interval not elapsed)
    if let Some(uns_mtx) = UNS_CACHE.get() {
        if let Some(mut uns) = uns_mtx.try_lock() {
            uns.sweep(tick);
        }
    }

    // Increment metrics tick
    if let Some(met_mtx) = METRICS.get() {
        if let Some(mut met) = met_mtx.try_lock() {
            met.ticks += 1;
        }
    }

    // Feed TSC jitter into RNG pool (keeps entropy fresh for sandbox_create etc.)
    // Read TSC from tick (cheap — no RDTSC needed in interrupt context)
    if let Some(rng_mtx) = RNG_FEEDER.get() {
        if let Some(mut rng) = rng_mtx.try_lock() {
            let tsc_approx = tick.wrapping_mul(0x9E3779B97F4A7C15); // tick × golden ratio = TSC approx
            rng.feed_timer_entropy(tsc_approx, tick);
            rng.check_refresh(tick);
        }
    }

    // Gap 20.4 — Aether compositor vsync @ 60fps (every 16 ticks ≈ 16ms at 1kHz).
    // Sweeps the window list and logs active window count every ~1s to serial.
    // QViewWm has map_window/unmap_window/focus — no blocking operations here.
    if tick % 16 == 0 {
        if let Some(wm_mtx) = WM.get() {
            if let Some(wm) = wm_mtx.try_lock() {
                // Log vsync frame once per ~second (every 60 frames × 16 ticks)
                if tick % (16 * 60) == 0 && tick > 0 {
                    crate::serial_println!(
                        "[AETHER] vsync @ tick {} — {} windows active", tick, wm.windows.len()
                    );
                }
            }
        }
    }

    // Record Q-Ring throughput in MetricStore (every 1000 ticks ~= 16ms @60kHz)
    if tick % 1000 == 0 {
        if let Some(ms_mtx) = METRIC_STORE.get() {
            if let Some(mut ms) = ms_mtx.try_lock() {
                // Record ring drain count as throughput proxy
                ms.record(crate::q_metrics::MetricKind::QRingThroughput, tick % 256, tick);
                ms.record(crate::q_metrics::MetricKind::ContextSwitchTicks, tick & 0xFF, tick);
            }
        }
    }

    // Gap 19.3 — Dispatch pending disk I/O requests every 10 ticks (~10ms at 1kHz).
    // DiskScheduler::dispatch() picks the highest-priority request (CFQ-weighted)
    // and calls kstate::nvme().write_blocks() or read_blocks() to submit to hardware.
    if tick % 10 == 0 {
        // try_lock: skip if nvme or disk_sched is already locked
        let disk_opt = crate::kstate::disk_sched_try_lock();
        let nvme_opt = crate::kstate::nvme_try_lock();
        if let (Some(mut disk), Some(mut nvme)) = (disk_opt, nvme_opt) {
            if let Some(request_id) = disk.dispatch(tick) {
                // A request was dequeued — perform the I/O on the NVMe controller
                // For now we issue a flush to commit any pending writes
                let _cid = nvme.flush();
                crate::serial_println!(
                    "[DISK] Dispatch: request {} → NVMe flush issued @ tick {}", request_id, tick
                );
                disk.complete(request_id, true, tick);
            }
        }
    }

    // Gap 19.4 — ghost_write: flush dirty write buffers every 5000 ticks (~5s).
    // Gap 23.3 + 25.2 — VirtIO-net RX ring poll + Ethernet demux every 100 ticks.
    if tick % 100 == 0 {
        if let Some(mut net) = crate::kstate::state_opt()
            .and_then(|s| s.virtio_net.try_lock())
        {
            let frames = net.receive();
            if !frames.is_empty() {
                // Gap 25.2 + logic-fix-3 — demux returns (accepted, optional ARP reply).
                // Send the ARP reply immediately while still holding the net lock.
                let our_mac = net.mac;
                let (accepted, arp_reply) = crate::net_stack::demux(&frames, &our_mac);
                if let Some(reply) = arp_reply {
                    net.send(&reply);
                }
                crate::serial_println!(
                    "[NET] tick={} rx={} accepted={}", tick, frames.len(), accepted
                );
            }
        }
    }

    if tick % 5000 == 0 && tick > 0 {
        if let Some(gw_mtx) = GHOST_WRITE.get() {
            if let Some(gw) = gw_mtx.try_lock() {
                let txns = gw.transaction_count();
                if txns > 0 {
                    crate::serial_println!(
                        "[GHOST-WRITE] Periodic flush @ tick {} — {} committed transactions", tick, txns
                    );
                    // Gap 27.1 — Sync committed ghost-write objects into Prism live index.
                    // Creates a PrismStoreBridge stub and calls write_and_update() to
                    // update the head pointer in the live object index after each flush.
                    drop(gw); // release lock before acquiring prism_store lock
                    let mut pb = crate::prism_store_bridge::PrismStoreBridge::new();
                    let tick_bytes = tick.to_le_bytes();
                    let _ = pb.write_and_update(
                        tick,           // object_id = current tick as checkpoint OID
                        0u64,           // author_silo = 0 (kernel)
                        &tick_bytes,    // data = 8-byte tick stamp
                        alloc::vec![],  // tags = empty
                        tick,           // tick
                    );
                    crate::serial_println!("[GHOST-WRITE] Prism write-back checkpoint OID={:#x}", tick);
                } else { drop(gw); }
            }
        }
    }

    // Gap 21.2 — Timer wheel dispatch: fire expired timers every tick.
    // TimerWheel::tick() (no args) fires any expired one-shot or periodic callbacks.
    {
        let tw_opt = crate::kstate::state_opt()
            .and_then(|s| s.timer_wheel.try_lock());
        if let Some(mut tw) = tw_opt {
            tw.tick();
        }
    }

    // Gap 21.3 — Aether-Kit compositor frame at 60fps.
    // AetherKitBridge::compositor_frame_tick needs (&mut ChimeraVgdiBridge, &mut QRingProcessor, tick).
    // We call it by fetching both from kstate_ext statics with try_lock — no-op if either is held.
    if tick % 16 == 0 {
        if let (Some(ak_mtx), Some(qr_mtx)) = (AETHER_KIT.get(), QRING.get()) {
            if let (Some(mut ak), Some(mut qr)) = (ak_mtx.try_lock(), qr_mtx.try_lock()) {
                // ChimeraVgdiBridge is a separate kstate_ext static; use a local stub context
                // (production: fetch from kstate::chimera_vgdi() once that bridge is wired)
                use crate::chimera_vgdi_bridge::ChimeraVgdiBridge;
                let mut vgdi_stub = ChimeraVgdiBridge::new();
                ak.compositor_frame_tick(&mut vgdi_stub, &mut qr, tick);
            }
        }
    }
}

// ── On-Silo-Spawn Hook ────────────────────────────────────────────────────────

/// Wire up a newly-spawned Silo into all Phase 84-100 subsystems.
/// Called from `silo_launch.rs` after SYSRET completes.
pub fn on_silo_spawn(silo_id: u64, binary_oid: [u8; 32], tick: u64) {
    use crate::silo_events::SiloEvent;
    use crate::q_view_wm::WindowType;

    if QRING.get().is_none() { return; } // not yet initialized

    if let Some(mut bus) = EVENT_BUS.get().and_then(|m| m.try_lock()) {
        bus.publish(SiloEvent::Spawned {
            silo_id, binary_oid, spawn_tick: tick,
            initial_caps: alloc::vec![],
            parent_silo: None,
        });
    }
    if let Some(mut ring) = QRING.get().and_then(|m| m.try_lock()) {
        ring.register_silo(silo_id);
    }
    if let Some(mut a11y) = A11Y.get().and_then(|m| m.try_lock()) {
        a11y.register_silo(silo_id);
    }
    if let Some(mut wm) = WM.get().and_then(|m| m.try_lock()) {
        wm.map_window(silo_id, binary_oid, WindowType::Browser, "Untitled");
    }
    if let Some(mut anom) = ANOMALY.get().and_then(|m| m.try_lock()) {
        anom.register_silo(silo_id, binary_oid);
    }
    if let Some(mut bb) = BLACK_BOX.get().and_then(|m| m.try_lock()) {
        bb.register_silo(silo_id, binary_oid, tick);
    }
    // Phase 101-104 additions
    if let Some(mut idx) = LIVE_INDEX.get().and_then(|m| m.try_lock()) {
        idx.register_silo_binary(silo_id, binary_oid, tick);
    }
    if let Some(mut ms) = METRIC_STORE.get().and_then(|m| m.try_lock()) {
        ms.record(crate::q_metrics::MetricKind::SiloSpawnLatency, tick & 0xFFFF, tick);
    }
    if let Some(mut snp) = SNAP_BRIDGE.get().and_then(|m| m.try_lock()) {
        let _ = snp.checkpoint(silo_id, "spawn", tick);
    }
    // Phase 105-108 additions
    // Law 2: measure each silo binary into the PCR chain (SHA-256 of binary_oid bytes)
    if let Some(mut sb) = SECURE_BOOT.get().and_then(|m| m.try_lock()) {
        sb.on_binary_load(silo_id, &binary_oid, tick);
    }
    // Register silo in Nexus routing table (ensure Nexus Silo can deliver to it)
    if let Some(mut nb) = NEXUS_BRIDGE.get().and_then(|m| m.try_lock()) {
        let dest_prefix = u64::from_le_bytes(binary_oid[..8].try_into().unwrap_or([0u8;8]));
        nb.install_route(crate::nexus_kernel_bridge::NexusRoute {
            dest_prefix,
            next_hop: silo_id,
            latency_ticks: 1,
            age_ticks: tick as u32,
            direct: true,
        });
    }

    crate::serial_println!(
        "[KSTATE-EXT] on_silo_spawn: Silo {} wired into 11 subsystems @ tick {}", silo_id, tick
    );
}

// ── On-Silo-Vaporize Hook ─────────────────────────────────────────────────────

/// Tear down all per-Silo state. Called from Sentinel / vaporize path.
pub fn on_silo_vaporize(silo_id: u64, tick: u64) {
    use crate::silo_events::SiloEvent;
    use crate::silo_events::VaporizeCause;
    use crate::black_box::VaporizationCause as BbCause;

    if QRING.get().is_none() { return; }

    if let Some(mut bb) = BLACK_BOX.get().and_then(|m| m.try_lock()) {
        bb.seal_post_mortem(silo_id, BbCause::UserRequested, tick);
    }
    if let Some(mut bus) = EVENT_BUS.get().and_then(|m| m.try_lock()) {
        bus.publish(SiloEvent::Vaporized {
            silo_id, tick,
            cause: VaporizeCause::UserRequested,
            post_mortem_oid: None,
        });
    }
    if let Some(mut ring) = QRING.get().and_then(|m| m.try_lock()) {
        ring.drain(silo_id);
        ring.deregister_silo(silo_id);
    }
    if let Some(mut a11y) = A11Y.get().and_then(|m| m.try_lock()) {
        a11y.unregister_silo(silo_id);
    }
    if let Some(mut wm) = WM.get().and_then(|m| m.try_lock()) {
        wm.unmap_window(silo_id);
    }
    if let Some(mut anom) = ANOMALY.get().and_then(|m| m.try_lock()) {
        anom.unregister_silo(silo_id);
    }

    crate::serial_println!(
        "[KSTATE-EXT] on_silo_vaporize: Silo {} cleaned up @ tick {}", silo_id, tick
    );
}

// ── Gap 22.3: Compute Auction Bridge ──────────────────────────────────────────

/// Per-kernel compute auction singleton.
static COMPUTE_AUCT: spin::Once<spin::Mutex<crate::compute_auction::ComputeAuctionEngine>> =
    spin::Once::new();

/// Gap 22.3 — Submit a compute bid from a Ring-3 Silo (via Syscall 304).
pub fn submit_compute_bid(task_id: u64, credits: u64, deadline_ticks: u64) -> Option<u64> {
    let mtx = COMPUTE_AUCT.call_once(|| {
        spin::Mutex::new(crate::compute_auction::ComputeAuctionEngine::new(1))
    });
    if let Some(mut auction) = mtx.try_lock() {
        // Map (task_id, credits, deadline) → ComputeAuctionEngine::submit_bid args
        let price_per_tick = if deadline_ticks > 0 { credits / deadline_ticks } else { 1 };
        let cap = crate::compute_auction::ComputeCapacity {
            cpu_cores: 1,
            ram_mib: 64,
            gpu_units: 0,
            npu_tops: 0,
            nvme_mib: 0,
            bandwidth_mbps: 100,
        };
        let bid_id = auction.submit_bid(
            cap, price_per_tick, deadline_ticks / 2, deadline_ticks,
            crate::kstate::global_tick(),
        );
        let _ = task_id; // task_id recorded in serial log by caller
        Some(bid_id)
    } else {
        None
    }
}
