//! # UNS Resolver Auth Bridge (Phase 250)
//!
//! ## Architecture Guardian: The Gap
//! `uns_resolver.rs` implements `UnsResolver`:
//! - `UnsPath::parse(path: &str)` → Option<Self>
//! - `UnsPath::path_hash()` → [u8; 32]
//! - `UnsPath::is_remote()` → bool
//! - `ResolveResult` — resolved node, scheme, resource type
//!
//! **Missing link**: Remote UNS path resolution (`is_remote() == true`)
//! had no capability gate. A Silo could resolve paths on any remote node
//! in the Nexus mesh, probing topology and leaking node addresses.
//!
//! This module provides `UnsResolverAuthBridge`:
//! Network:EXEC cap required for remote (cross-mesh) path resolution.

extern crate alloc;

use crate::uns_resolver::UnsPath;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct UnsResolverAuthStats {
    pub local_resolved:  u64,
    pub remote_allowed:  u64,
    pub remote_denied:   u64,
}

pub struct UnsResolverAuthBridge {
    pub stats: UnsResolverAuthStats,
}

impl UnsResolverAuthBridge {
    pub fn new() -> Self {
        UnsResolverAuthBridge { stats: UnsResolverAuthStats::default() }
    }

    /// Authorize a UNS path resolution. Remote paths need Network:EXEC cap.
    pub fn authorize_resolve(
        &mut self,
        silo_id: u64,
        path: &UnsPath,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !path.is_remote() {
            self.stats.local_resolved += 1;
            return true;
        }
        if !forge.check(silo_id, CapType::Network, CAP_EXEC, 0, tick) {
            self.stats.remote_denied += 1;
            crate::serial_println!(
                "[UNS RESOLVER] Silo {} remote path resolve denied — no Network:EXEC cap", silo_id
            );
            return false;
        }
        self.stats.remote_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  UnsResolverBridge: local={} remote_allowed={} denied={}",
            self.stats.local_resolved, self.stats.remote_allowed, self.stats.remote_denied
        );
    }
}
