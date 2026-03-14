//! # QAudit Kernel Integration (Phase 137)
//!
//! ## Architecture Guardian: The Gap
//! `qaudit.rs` implements `AuditLog`:
//! - `log()` — appends cryptographically chained audit events
//! - `verify_chain()` — verifies HMAC chain integrity
//! - `query_category()` / `query_silo()` / `query_severity()` — audit queries
//!
//! **Missing link**: Nothing called `AuditLog::log()` from kernel events.
//! Law violations, Silo vaporizations, CapToken denials, and Q-Ring
//! hardening blocks all happened silently with no audit trail.
//!
//! This module provides `QAuditKernel`:
//! 1. `log_law_violation()` — records Law-N audits
//! 2. `log_cap_denial()` — records Law 1 capability denials
//! 3. `log_silo_vaporize()` — records Silo lifecycle events
//! 4. `log_quota_hard_limit()` — records resource enforcement
//! 5. `log_hotswap()` — records binary swap events (Law 2)
//! 6. `verify_and_report()` — calls verify_chain() and reports

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::qaudit::{AuditLog, AuditCategory, Severity};

// ── Integration Statistics ────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AuditKernelStats {
    pub law_violations:   u64,
    pub cap_denials:      u64,
    pub silo_events:      u64,
    pub quota_events:     u64,
    pub hotswap_events:   u64,
    pub verify_failures:  u64,
    pub verify_ok:        u64,
}

// ── QAudit Kernel Integration ─────────────────────────────────────────────────

/// Routes kernel events to the cryptographically-chained AuditLog.
pub struct QAuditKernel {
    pub log:   AuditLog,
    pub stats: AuditKernelStats,
}

impl QAuditKernel {
    pub fn new() -> Self {
        QAuditKernel {
            log:   AuditLog::new(4096), // 4096-event ring
            stats: AuditKernelStats::default(),
        }
    }

    /// Record a Q-Manifest Law violation.
    pub fn log_law_violation(&mut self, law: u8, silo_id: u64, tick: u64) {
        self.stats.law_violations += 1;
        self.log.log(
            Severity::Critical,
            AuditCategory::PolicyChange,
            Some(silo_id),
            &alloc::format!("Silo {}", silo_id),
            &alloc::format!("Law-{} violated", law),
            false,
            "",
            tick,
        );
    }

    /// Record a CapToken deny event (Law 1).
    pub fn log_cap_denial(&mut self, silo_id: u64, cap_type: &str, tick: u64) {
        self.stats.cap_denials += 1;
        self.log.log(
            Severity::Alert,
            AuditCategory::Authorization,
            Some(silo_id),
            &alloc::format!("Silo {}", silo_id),
            &alloc::format!("Cap denied: {}", cap_type),
            false,
            "",
            tick,
        );
    }

    /// Record Silo lifecycle event.
    pub fn log_silo_vaporize(&mut self, silo_id: u64, reason: &str, tick: u64) {
        self.stats.silo_events += 1;
        self.log.log(
            Severity::Warning,
            AuditCategory::SiloLifecycle,
            Some(silo_id),
            &alloc::format!("Silo {}", silo_id),
            "vaporized",
            true,
            reason,
            tick,
        );
    }

    /// Record a quota hard-limit enforcement event.
    pub fn log_quota_hard_limit(&mut self, silo_id: u64, resource: &str, tick: u64) {
        self.stats.quota_events += 1;
        self.log.log(
            Severity::Alert,
            AuditCategory::PolicyChange,
            Some(silo_id),
            &alloc::format!("Silo {}", silo_id),
            &alloc::format!("Quota hard-limit: {}", resource),
            false,
            "",
            tick,
        );
    }

    /// Record a binary hotswap event (Law 2).
    pub fn log_hotswap(&mut self, module_name: &str, old_hash: &[u8; 32], tick: u64) {
        self.stats.hotswap_events += 1;
        self.log.log(
            Severity::Warning,
            AuditCategory::Integrity,
            None,
            module_name,
            "hotswap",
            true,
            &alloc::format!("old={:02x}{:02x}..", old_hash[0], old_hash[1]),
            tick,
        );
    }

    /// Record a Q-Ring hardening block.
    pub fn log_qring_block(&mut self, silo_id: u64, rejected: usize, tick: u64) {
        self.log.log(
            Severity::Alert,
            AuditCategory::Authorization,
            Some(silo_id),
            &alloc::format!("Silo {}", silo_id),
            "qring_block",
            false,
            &alloc::format!("{} entries rejected", rejected),
            tick,
        );
    }

    /// Verify audit chain integrity; returns false if chain is broken.
    pub fn verify_and_report(&mut self) -> bool {
        let ok = self.log.verify_chain();
        if ok {
            self.stats.verify_ok += 1;
            crate::serial_println!("[QAUDIT] Chain integrity ✓ ({} events)", self.log.events.len());
        } else {
            self.stats.verify_failures += 1;
            crate::serial_println!("[QAUDIT] Chain integrity ✗ — TAMPER DETECTED!");
        }
        ok
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QAuditKernel: law={} cap={} silo={} quota={} hotswap={} ok={} fail={}",
            self.stats.law_violations, self.stats.cap_denials, self.stats.silo_events,
            self.stats.quota_events, self.stats.hotswap_events,
            self.stats.verify_ok, self.stats.verify_failures
        );
    }
}
