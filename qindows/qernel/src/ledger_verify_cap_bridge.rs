//! # Ledger Verify Cap Bridge (Phase 176)
//!
//! ## Architecture Guardian: The Gap
//! `ledger.rs` provides:
//! - `validate_manifest(manifest: &AppManifest)` → Result<(), LedgerError>
//! - `AppManifest` — full app manifest struct
//! - `ManifestCapability` variants:
//!   - `Network { protocol: String }`
//!   - `Storage { scope: String }`
//!   - `Graphics` (unit)
//!   - `NeuralInput` (unit)
//!   - `MeshCompute` (unit)
//!   - `Device { class: String }`
//!
//! **Missing link**: App manifests were validated structurally but
//! never cross-checked against the Silo's actual CapToken set.
//! A manifest could declare `Network` even without a Net:EXEC cap.
//!
//! This module provides `LedgerVerifyCapBridge`:
//! `verify_and_launch()` — validate manifest + cross-check CapTokens.

extern crate alloc;

use crate::ledger::{AppManifest, validate_manifest, ManifestCapability};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct LedgerCapBridgeStats {
    pub launches_allowed: u64,
    pub launches_denied:  u64,
    pub manifest_errors:  u64,
}

pub struct LedgerVerifyCapBridge {
    pub stats: LedgerCapBridgeStats,
}

impl LedgerVerifyCapBridge {
    pub fn new() -> Self {
        LedgerVerifyCapBridge { stats: LedgerCapBridgeStats::default() }
    }

    /// Validate app manifest + verify CapToken set matches declared caps.
    pub fn verify_and_launch(
        &mut self,
        silo_id: u64,
        manifest: &AppManifest,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        // Step 1: structural manifest validation
        if let Err(e) = validate_manifest(manifest) {
            self.stats.manifest_errors += 1;
            crate::serial_println!("[LEDGER CAP] Silo {} manifest invalid: {:?}", silo_id, e);
            return false;
        }

        // Step 2: verify declared caps match Silo's CapToken set
        for cap in &manifest.capabilities {
            let required = match cap {
                ManifestCapability::Network { .. } => CapType::Network,
                ManifestCapability::Storage { .. } => CapType::Prism,
                ManifestCapability::Graphics       => CapType::Energy, // GPU resources
                ManifestCapability::NeuralInput    => CapType::Synapse,
                ManifestCapability::MeshCompute    => CapType::Energy,
                ManifestCapability::Device { .. }  => CapType::Admin,
            };
            if !forge.check(silo_id, required, CAP_EXEC, 0, tick) {
                self.stats.launches_denied += 1;
                crate::serial_println!(
                    "[LEDGER CAP] Silo {} launch DENIED — manifest declares {:?} but no cap",
                    silo_id, cap
                );
                return false;
            }
        }

        self.stats.launches_allowed += 1;
        true
    }

    /// Just validate manifest structure (no cap check).
    pub fn validate_only(&mut self, manifest: &AppManifest) -> bool {
        match validate_manifest(manifest) {
            Ok(()) => true,
            Err(e) => {
                self.stats.manifest_errors += 1;
                crate::serial_println!("[LEDGER CAP] Manifest error: {:?}", e);
                false
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  LedgerCapBridge: allowed={} denied={} errors={}",
            self.stats.launches_allowed, self.stats.launches_denied, self.stats.manifest_errors
        );
    }
}
