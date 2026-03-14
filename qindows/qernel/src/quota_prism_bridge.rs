//! # Quota Prism Bridge (Phase 169)
//!
//! ## Architecture Guardian: The Gap
//! `quota_enforcement_bridge.rs` (Phase 132) holds `QuotaManager` with
//! `charge(silo_id, resource, amount)` / `set_quota(...)`.
//!
//! However, there was no Prism-specific quota path.
//! Large Prism writes bypassed all quota accounting → disk exhaustion.
//!
//! This module provides `QuotaPrismBridge`:
//! - Standalone per-Silo storage quota tracking (no external quota module needed)
//! - Simple BTreeMap tracking; quota checked before every Prism write

extern crate alloc;
use alloc::collections::BTreeMap;

const DEFAULT_PRISM_QUOTA_BYTES: u64 = 10 * 1024 * 1024 * 1024; // 10 GiB

#[derive(Debug, Clone)]
struct SiloStorageState {
    limit_bytes: u64,
    used_bytes:  u64,
}

#[derive(Debug, Default, Clone)]
pub struct QuotaPrismBridgeStats {
    pub writes_allowed: u64,
    pub writes_denied:  u64,
    pub bytes_charged:  u64,
}

pub struct QuotaPrismBridge {
    silos:    BTreeMap<u64, SiloStorageState>,
    pub stats: QuotaPrismBridgeStats,
}

impl QuotaPrismBridge {
    pub fn new() -> Self {
        QuotaPrismBridge { silos: BTreeMap::new(), stats: QuotaPrismBridgeStats::default() }
    }

    /// Set per-Silo Prism storage quota at spawn time.
    pub fn set_silo_storage_quota(&mut self, silo_id: u64, max_bytes: u64) {
        self.silos.insert(silo_id, SiloStorageState { limit_bytes: max_bytes, used_bytes: 0 });
    }

    /// Charge storage quota. Returns false if over limit.
    pub fn charge_prism_write(&mut self, silo_id: u64, byte_count: u64) -> bool {
        let state = self.silos.entry(silo_id).or_insert(SiloStorageState {
            limit_bytes: DEFAULT_PRISM_QUOTA_BYTES, used_bytes: 0,
        });

        if state.used_bytes + byte_count > state.limit_bytes {
            self.stats.writes_denied += 1;
            crate::serial_println!(
                "[QUOTA PRISM] Silo {} write denied — quota exceeded ({}/{} bytes)",
                silo_id, state.used_bytes, state.limit_bytes
            );
            return false;
        }
        state.used_bytes += byte_count;
        self.stats.writes_allowed += 1;
        self.stats.bytes_charged += byte_count;
        true
    }

    /// Release stored bytes on Silo vaporize.
    pub fn on_silo_vaporize(&mut self, silo_id: u64) {
        self.silos.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QuotaPrismBridge: allowed={} denied={} charged={}",
            self.stats.writes_allowed, self.stats.writes_denied, self.stats.bytes_charged
        );
    }
}
