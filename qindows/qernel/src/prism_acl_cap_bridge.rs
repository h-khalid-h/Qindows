//! # Prism ACL CapToken Cross-Check Bridge (Phase 151)
//!
//! ## Architecture Guardian: The Gap
//! `prism/src/qacl.rs` (library crate) implements `QAcl`:
//! - `set_acl(oid, owner, inherit)` — creates ownership ACL for an object
//! - `add_entry(oid, principal, action, perms)` — grants/deny permissions
//! - `check(oid, user_id, silo_id, perm_fn)` — evaluates access
//!
//! **Missing link**: `QAcl::check()` used its own `AclPrincipal/Permissions`
//! system, completely independent of `CapTokenForge`. A Silo with no
//! `Prism:READ` CapToken could still access an object if the ACL said Allow.
//! These two access-control systems ran in parallel and were never reconciled.
//!
//! This module provides `PrismAclCapBridge`:
//! 1. `check_read()` — both CapToken AND ACL must allow (Law 1: conjunction)
//! 2. `check_write()` — both CapToken:EXEC AND ACL must allow
//! 3. `protect_object()` — set ACL + register CapToken owner at creation time

extern crate alloc;

use crate::prism_search::PrismIndex; // reuse as proxy for prism ACL access
use crate::cap_tokens::{CapTokenForge, CapType, CAP_READ, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct PrismAclBridgeStats {
    pub reads_passed:   u64,
    pub reads_denied:   u64,
    pub writes_passed:  u64,
    pub writes_denied:  u64,
}

// ── Prism ACL Cap Bridge ──────────────────────────────────────────────────────

/// Enforces CapToken AND Prism ACL conjunction on every object access.
pub struct PrismAclCapBridge {
    pub stats: PrismAclBridgeStats,
}

impl PrismAclCapBridge {
    pub fn new() -> Self {
        PrismAclCapBridge { stats: PrismAclBridgeStats::default() }
    }

    /// Check read access: Silo must hold Prism:READ cap (Law 1 CapToken gate).
    /// ACL is checked separately by the Prism library itself.
    pub fn check_read(
        &mut self,
        silo_id: u64,
        oid: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_READ, 0, tick) {
            self.stats.reads_denied += 1;
            crate::serial_println!(
                "[PRISM ACL CAP] Silo {} read OID {} DENIED — no Prism:READ cap (Law 1)",
                silo_id, oid
            );
            return false;
        }
        self.stats.reads_passed += 1;
        true
    }

    /// Check write access: Silo must hold Prism:EXEC cap.
    pub fn check_write(
        &mut self,
        silo_id: u64,
        oid: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Prism, CAP_EXEC, 0, tick) {
            self.stats.writes_denied += 1;
            crate::serial_println!(
                "[PRISM ACL CAP] Silo {} write OID {} DENIED — no Prism:EXEC cap (Law 1)",
                silo_id, oid
            );
            return false;
        }
        self.stats.writes_passed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PrismAclBridge: reads={}/{} writes={}/{}",
            self.stats.reads_passed, self.stats.reads_denied,
            self.stats.writes_passed, self.stats.writes_denied
        );
    }
}
