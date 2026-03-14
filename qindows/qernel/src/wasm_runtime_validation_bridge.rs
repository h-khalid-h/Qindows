//! # WASM Runtime Validation Bridge (Phase 255)
//!
//! ## Architecture Guardian: The Gap
//! `wasm_runtime.rs` implements WASM module validation:
//! - `validate_wasm_binary(bytes: &[u8])` → Result<WasmModuleDesc, WasmValidationError>
//! - `resolve_wasm_import(module, name)` → Option<u64>
//! - `WasmValidationError` — MissingMagic, UnsupportedVersion, InvalidSection, ...
//!
//! **Missing link**: `validate_wasm_binary()` was called but the module
//! size was uncapped. A malicious Silo could submit a 1 GB WASM binary,
//! exhausting kernel heap during validation (before exec actually starts).
//!
//! This module provides `WasmRuntimeValidationBridge`:
//! Max 16 MB WASM binary per Silo. Returns error on oversize before parsing.

extern crate alloc;

use crate::wasm_runtime::{validate_wasm_binary, WasmModuleDesc, WasmValidationError};
use crate::qaudit_kernel::QAuditKernel;

const MAX_WASM_SIZE_BYTES: usize = 16 * 1024 * 1024; // 16 MiB

#[derive(Debug, Default, Clone)]
pub struct WasmValidationStats {
    pub validated_ok:  u64,
    pub size_rejected: u64,
    pub invalid:       u64,
}

pub struct WasmRuntimeValidationBridge {
    pub stats: WasmValidationStats,
}

impl WasmRuntimeValidationBridge {
    pub fn new() -> Self {
        WasmRuntimeValidationBridge { stats: WasmValidationStats::default() }
    }

    /// Validate a WASM binary with size cap enforcement.
    pub fn validate(
        &mut self,
        bytes: &[u8],
        silo_id: u64,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> Result<WasmModuleDesc, WasmValidationError> {
        if bytes.len() > MAX_WASM_SIZE_BYTES {
            self.stats.size_rejected += 1;
            audit.log_law_violation(4u8, silo_id, tick); // Law 4: resource fairness
            crate::serial_println!(
                "[WASM] Silo {} binary {} bytes exceeds {} MiB cap — Law 4",
                silo_id, bytes.len(), MAX_WASM_SIZE_BYTES / (1024*1024)
            );
            return Err(WasmValidationError::BinaryTooLarge {
                size: bytes.len() as u64,
                limit: MAX_WASM_SIZE_BYTES as u64,
            });
        }
        match validate_wasm_binary(bytes) {
            Ok(desc) => { self.stats.validated_ok += 1; Ok(desc) }
            Err(e) => { self.stats.invalid += 1; Err(e) }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  WasmValidationBridge: ok={} size_rejected={} invalid={}",
            self.stats.validated_ok, self.stats.size_rejected, self.stats.invalid
        );
    }
}
