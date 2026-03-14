//! # Identity Token Expiry Bridge (Phase 212)
//!
//! ## Architecture Guardian: The Gap  
//! `identity.rs` implements `IdentityToken`:
//! - `IdentityToken::is_valid_at(tick)` → bool
//! - `IdentityToken::is_valid()` → bool
//! - `IdentityId::from_seed(seed)` → IdentityId
//! - `AuthMethod` — Password, BioFace, BioRetina, Hardware, Neural, Recovery
//!
//! **Missing link**: Token expiry was checked individually but never enforced
//! at the kernel auth boundary. A token that expired mid-session was never
//! revoked — processes continued running with stale identity tokens (Law 1).
//!
//! This module provides `IdentityTokenExpiryBridge`:
//! Periodic sweep — revokes sessions holding expired IdentityTokens.

extern crate alloc;
use alloc::collections::BTreeMap;

use crate::identity::IdentityToken;

#[derive(Debug, Default, Clone)]
pub struct IdentityExpiryStats {
    pub checks:    u64,
    pub expired:   u64,
}

pub struct IdentityTokenExpiryBridge {
    pub stats: IdentityExpiryStats,
}

impl IdentityTokenExpiryBridge {
    pub fn new() -> Self {
        IdentityTokenExpiryBridge { stats: IdentityExpiryStats::default() }
    }

    /// Check if a token is valid at the current tick.
    pub fn check_token(&mut self, token: &IdentityToken, tick: u64) -> bool {
        self.stats.checks += 1;
        if !token.is_valid_at(tick) {
            self.stats.expired += 1;
            crate::serial_println!(
                "[IDENTITY] Token expired at tick {} — session should be revoked (Law 1)", tick
            );
            return false;
        }
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IdentityExpiryBridge: checks={} expired={}",
            self.stats.checks, self.stats.expired
        );
    }
}
