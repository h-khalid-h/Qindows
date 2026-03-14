//! # Secure Boot PCR Audit Bridge (Phase 227)
//!
//! ## Architecture Guardian: The Gap
//! `secure_boot.rs` implements the measured boot chain:
//! - `Pcr { index, digest }` — Platform Configuration Register
//! - `Pcr::extend(digest)` → bool — extends PCR with new measurement
//! - `BootPolicy` — RequireSigned, AllowSelfSigned, Skip
//! - `BootComponent` — Kernel, Bootloader, Firmware, Driver, Application
//!
//! **Missing link**: PCR extension was never audited. A compromised boot
//! stage could extend a PCR with a fake measurement, breaking the trust chain.
//!
//! This module provides `SecureBootPcrAuditBridge`:
//! Logs every PCR extension to QAuditKernel (Law 2 — binary integrity).

extern crate alloc;

use crate::secure_boot::{Pcr, BootComponent};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct SecureBootPcrStats {
    pub extensions: u64,
    pub failed:     u64,
}

pub struct SecureBootPcrAuditBridge {
    pub stats: SecureBootPcrStats,
}

impl SecureBootPcrAuditBridge {
    pub fn new() -> Self {
        SecureBootPcrAuditBridge { stats: SecureBootPcrStats::default() }
    }

    /// Extend a PCR and log the extension event (Law 2).
    pub fn extend_with_audit(
        &mut self,
        pcr: &mut Pcr,
        component: &BootComponent,
        digest: &[u8; 32],
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        let ok = pcr.extend(digest);
        if ok {
            self.stats.extensions += 1;
            audit.log_hotswap("pcr_extend", digest, tick);
            crate::serial_println!("[SECURE BOOT] PCR[{}] extended for {:?}", pcr.index, component);
        } else {
            self.stats.failed += 1;
            crate::serial_println!("[SECURE BOOT] PCR[{}] extend FAILED (locked?)", pcr.index);
        }
        ok
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SecureBootPcrBridge: extensions={} failed={}", self.stats.extensions, self.stats.failed
        );
    }
}
