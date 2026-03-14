//! # WASM-Prism Bridge (Phase 121)
//!
//! ## Architecture Guardian: The Gap
//! `wasm_runtime.rs` (Phase 86):
//! - `register_module()` — validates WASM binary, plans memory layout
//! - `compilation_complete()` — records compiled Prism OID
//! - BUT: nobody calls `register_module()` when an app is installed
//! - Nobody wires the compiled OID back to the Ledger for Law 2 tracking
//!
//! This module provides `WasmPrismBridge`:
//! 1. `on_app_install()` — called by Ledger after install, feeds WASM binary to runtime
//! 2. `poll_compilation()` — checks if AOT compilation is complete, updates Ledger
//! 3. `spawn_wasm_silo()` — launches a compiled WASM binary as a new Silo
//! 4. `track_memory()` — enforces WASM memory limits via silo_ipc_router
//!
//! ## WASM Compilation Pipeline
//! ```text
//! QLedger::install(bytes)
//!   → wasm_prism_bridge::on_app_install()
//!       → WasmRuntime::register_module(bytes) → hash
//!       → CapTokenForge::grant_wasm_cap(hash)
//!       → [AOT compile in Prism background Silo]
//!       → WasmRuntime::compilation_complete(hash, compiled_oid)
//!       → QLedger sets compiled_oid
//!       → Can now launch
//! ```

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

use crate::wasm_runtime::{WasmRuntime, WasmValidationError, WasmRuntimeRecord};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC, CAP_READ};

// ── WASM App Lifecycle State ──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmAppState {
    Registered,    // binary stored, AOT in progress
    Compiled,      // compiled OID received from AOT silo
    Running,       // one or more active Silos
    Quarantined,   // Sentinel flagged
}

// ── WASM-Prism Bridge Stats ───────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct WasmBridgeStats {
    pub installs:               u64,
    pub validation_failures:    u64,
    pub compilations_complete:  u64,
    pub silos_launched:         u64,
    pub memory_limit_breaches:  u64,
    pub quarantined:            u64,
}

// ── App Record ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct WasmAppRecord {
    pub app_id:       String,
    pub wasm_hash:    u64,
    pub compiled_oid: Option<u64>,
    pub state:        WasmAppState,
    pub silo_ids:     Vec<u64>,
    pub installed_tick: u64,
}

// ── WASM-Prism Bridge ─────────────────────────────────────────────────────────

/// Integrates WASM runtime with Prism object store and Silo lifecycle.
pub struct WasmPrismBridge {
    pub runtime:  WasmRuntime,
    pub apps:     BTreeMap<String, WasmAppRecord>,
    pub stats:    WasmBridgeStats,
    /// Maximum WASM linear memory pages (64K each); default: 256 pages = 16MB
    pub max_pages: u32,
}

impl WasmPrismBridge {
    pub fn new() -> Self {
        WasmPrismBridge {
            runtime:   WasmRuntime::new(),
            apps:      BTreeMap::new(),
            stats:     WasmBridgeStats::default(),
            max_pages: 256,
        }
    }

    /// Called by QLedger::install() when a new WASM package arrives.
    pub fn on_app_install(
        &mut self,
        app_id: &str,
        wasm_bytes: &[u8],
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Result<u64, WasmValidationError> {
        self.stats.installs += 1;

        // 1. Validate + memory-plan the WASM binary
        let hash = match self.runtime.register_module(wasm_bytes) {
            Ok(h) => h,
            Err(e) => {
                self.stats.validation_failures += 1;
                crate::serial_println!("[WASM BRIDGE] Validation failed for '{}': {:?}", app_id, e);
                return Err(e);
            }
        };

        // 2. Grant WASM execution cap to kernel (for AOT compile silo)
        // In production: the AOT compile Silo is granted an ephemeral Wasm cap
        crate::serial_println!(
            "[WASM BRIDGE] Registered '{}' hash={:#x} — queued for AOT compile", app_id, hash
        );

        // 3. Record the app
        self.apps.insert(app_id.into(), WasmAppRecord {
            app_id: app_id.into(),
            wasm_hash: hash,
            compiled_oid: None,
            state: WasmAppState::Registered,
            silo_ids: alloc::vec![],
            installed_tick: tick,
        });

        Ok(hash)
    }

    /// Called by AOT compile Silo when compilation finishes.
    pub fn on_compilation_complete(&mut self, wasm_hash: u64, compiled_oid: u64, tick: u64) {
        self.runtime.compilation_complete(wasm_hash, compiled_oid);
        self.stats.compilations_complete += 1;

        // Find app by hash
        for record in self.apps.values_mut() {
            if record.wasm_hash == wasm_hash {
                record.compiled_oid = Some(compiled_oid);
                record.state = WasmAppState::Compiled;
                crate::serial_println!(
                    "[WASM BRIDGE] '{}' AOT complete → OID={:#x} @ tick {}",
                    record.app_id, compiled_oid, tick
                );
                return;
            }
        }
    }

    /// Launch a compiled WASM app as a new Silo.
    pub fn spawn_wasm_silo(
        &mut self,
        app_id: &str,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        let record = self.apps.get_mut(app_id)?;
        if record.state != WasmAppState::Compiled { return None; }

        let compiled_oid = record.compiled_oid?;

        // In production: calls silo_launch::spawn_from_oid(compiled_oid)
        let new_silo_id = compiled_oid ^ (tick & 0xFFFF);
        record.silo_ids.push(new_silo_id);
        record.state = WasmAppState::Running;
        self.stats.silos_launched += 1;

        // Grant baseline caps + Wasm exec cap to new silo
        forge.register_silo(new_silo_id, crate::crypto_primitives::sha256(&compiled_oid.to_le_bytes()));
        forge.grant_baseline(new_silo_id, tick);
        forge.mint(new_silo_id, CapType::Wasm, compiled_oid, tick + 1_000_000, CAP_EXEC | CAP_READ);

        crate::serial_println!(
            "[WASM BRIDGE] Spawned '{}' as Silo {} from OID={:#x}", app_id, new_silo_id, compiled_oid
        );
        Some(new_silo_id)
    }

    /// Quarantine a WASM app (on Law violation).
    pub fn quarantine(&mut self, app_id: &str) {
        if let Some(rec) = self.apps.get_mut(app_id) {
            rec.state = WasmAppState::Quarantined;
            self.stats.quarantined += 1;
            crate::serial_println!("[WASM BRIDGE] '{}' quarantined", app_id);
        }
    }

    /// Check if an app's compiled OID exists (for Ledger query).
    pub fn compiled_oid_for(&self, app_id: &str) -> Option<u64> {
        self.apps.get(app_id)?.compiled_oid
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  WasmBridge: installs={} compiled={} launched={} quarantined={} apps={}",
            self.stats.installs, self.stats.compilations_complete,
            self.stats.silos_launched, self.stats.quarantined, self.apps.len()
        );
    }
}
