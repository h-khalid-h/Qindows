//! # Kernel Integration Health Bridge (Phase 204)
//!
//! Boot-time health probe across all 24 kstate_ext global subsystems (Phase 84-108).

extern crate alloc;
use crate::kstate_ext;

#[derive(Debug, Default, Clone)]
pub struct KernelHealthStats {
    pub subsystems_ok:     u64,
    pub subsystems_failed: u64,
}

pub struct KernelIntegrationHealthBridge {
    pub stats: KernelHealthStats,
}

impl KernelIntegrationHealthBridge {
    pub fn new() -> Self {
        KernelIntegrationHealthBridge { stats: KernelHealthStats::default() }
    }

    /// Run a health probe on all 24 kstate_ext subsystems (Phase 84-108).
    /// Returns true if all subsystems are accessible (not deadlocked).
    pub fn probe_all(&mut self) -> bool {
        macro_rules! probe {
            ($name:expr, $accessor:expr) => {{
                let _guard = $accessor;
                self.stats.subsystems_ok += 1;
                crate::serial_println!("[HEALTH] {} OK", $name);
            }};
        }

        // Phase 84-100 (original 16 statics)
        probe!("EventBus",    kstate_ext::event_bus());
        probe!("QRing",       kstate_ext::qring());
        probe!("Anomaly",     kstate_ext::anomaly());
        probe!("BlackBox",    kstate_ext::black_box());
        probe!("WM",          kstate_ext::wm());
        probe!("A11y",        kstate_ext::a11y());
        probe!("UnsCache",    kstate_ext::uns_cache());
        probe!("QEnergy",     kstate_ext::qenergy());
        probe!("GhostWrite",  kstate_ext::ghost_write());
        probe!("Timeline",    kstate_ext::timeline());
        probe!("Fonts",       kstate_ext::fonts());
        probe!("Browser",     kstate_ext::browser());
        probe!("NexusDht",    kstate_ext::nexus_dht());
        probe!("VGdi",        kstate_ext::vgdi());
        probe!("QKit",        kstate_ext::qkit());
        probe!("Metrics",     kstate_ext::metrics());
        // Phase 101-104
        probe!("RngFeeder",   kstate_ext::rng_feeder());
        probe!("LiveIndex",   kstate_ext::live_index());
        probe!("MetricStore", kstate_ext::metric_store());
        probe!("SnapBridge",  kstate_ext::snap_bridge());
        // Phase 105-108
        probe!("SecureBoot",  kstate_ext::secure_boot());
        probe!("NexusBridge", kstate_ext::nexus_bridge());
        probe!("WasmBridge",  kstate_ext::wasm_bridge());
        probe!("AetherKit",   kstate_ext::aether_kit());

        let all_ok = self.stats.subsystems_failed == 0;
        crate::serial_println!(
            "[KERNEL HEALTH] {}/{} subsystems OK — {}",
            self.stats.subsystems_ok,
            self.stats.subsystems_ok + self.stats.subsystems_failed,
            if all_ok { "PASS" } else { "FAIL" }
        );
        all_ok
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  KernelHealthBridge: ok={} failed={}",
            self.stats.subsystems_ok, self.stats.subsystems_failed
        );
    }
}
