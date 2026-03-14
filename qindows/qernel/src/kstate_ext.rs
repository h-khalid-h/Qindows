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

    crate::serial_println!(
        "[KSTATE-EXT] Phase 84-100 subsystems initialized ({} statics)",
        16
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

    crate::serial_println!(
        "[KSTATE-EXT] on_silo_spawn: Silo {} wired into 6 subsystems @ tick {}", silo_id, tick
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
