//! # Identity Token Bind Bridge (Phase 272)
//!
//! ## Architecture Guardian: The Gap
//! `identity.rs` implements `IdentityToken`:
//! - `IdentityToken::is_valid_at(tick: u64)` → bool
//! - `IdentityToken::is_valid()` → bool
//! - `IdentityId::from_seed(seed: &[u8])` → Self
//!
//! **Missing link**: Identity tokens were checked for validity but
//! there was no binding between a token and the Silo that created it.
//! A Silo could share its IdentityToken with other Silos, allowing
//! privilege elevation through credential sharing.
//!
//! This module provides `IdentityTokenBindBridge`:
//! Validates IdentityToken binding — token.silo_id must match caller.

extern crate alloc;

use crate::identity::IdentityToken;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct IdentityBindStats {
    pub valid:          u64,
    pub bind_violation: u64,
    pub expired:        u64,
}

pub struct IdentityTokenBindBridge {
    pub stats: IdentityBindStats,
}

impl IdentityTokenBindBridge {
    pub fn new() -> Self {
        IdentityTokenBindBridge { stats: IdentityBindStats::default() }
    }

    /// Verify token validity and binding — caller_silo must match token.silo_id.
    pub fn verify(
        &mut self,
        token: &IdentityToken,
        caller_silo: u64,
        tick: u64,
        audit: &mut QAuditKernel,
    ) -> bool {
        if !token.is_valid_at(tick) {
            self.stats.expired += 1;
            return false;
        }
        if token.bound_silo != caller_silo {
            self.stats.bind_violation += 1;
            audit.log_law_violation(1u8, caller_silo, tick);
            crate::serial_println!(
                "[IDENTITY] Silo {} using token bound to Silo {} — Law 1 BLOCKED",
                caller_silo, token.bound_silo
            );
            return false;
        }
        self.stats.valid += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IdentityBindBridge: valid={} bind_violation={} expired={}",
            self.stats.valid, self.stats.bind_violation, self.stats.expired
        );
    }
}
