//! # Firstboot Genesis Bridge (Phase 215)
//!
//! ## Architecture Guardian: The Gap
//! `firstboot.rs` implements first-boot initialization:
//! The kernel runs `genesis.rs` to set up the initial Silo environment.
//!
//! **Missing link**: Firstboot ran with no audit trail. The key genesis
//! events (initial CapToken minting, system Silo creation) were never
//! logged — creating a gap in the tamper-detection chain.
//!
//! This module provides `FirstbootGenesisAuditBridge`:
//! Logs all genesis events to QAuditKernel at firstboot completion.

extern crate alloc;

use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct FirstbootAuditStats {
    pub events_logged: u64,
}

pub struct FirstbootGenesisAuditBridge {
    pub stats: FirstbootAuditStats,
}

impl FirstbootGenesisAuditBridge {
    pub fn new() -> Self {
        FirstbootGenesisAuditBridge { stats: FirstbootAuditStats::default() }
    }

    /// Log genesis events on firstboot completion (Law 2: binary integrity chain).
    pub fn log_genesis_complete(
        &mut self,
        kernel_hash: &[u8; 32],
        node_id: &[u8; 32],
        audit: &mut QAuditKernel,
        tick: u64,
    ) {
        self.stats.events_logged += 1;
        audit.log_hotswap("genesis_complete", kernel_hash, tick);
        crate::serial_println!(
            "[FIRSTBOOT] Genesis complete: kernel_hash={:02x}{:02x}.. node={:02x}{:02x}.. (Law 2 audit)",
            kernel_hash[0], kernel_hash[1], node_id[0], node_id[1]
        );
    }

    /// Log initial Cap minting on firstboot (Law 1: Zero-Ambient Authority audit).
    pub fn log_cap_minted(&mut self, silo_id: u64, audit: &mut QAuditKernel, tick: u64) {
        self.stats.events_logged += 1;
        audit.log_law_violation(1u8, silo_id, tick); // Law 1: tracking cap grant events
        crate::serial_println!("[FIRSTBOOT] Initial cap minted for Silo {}", silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  FirstbootGenesisAuditBridge: events_logged={}", self.stats.events_logged
        );
    }
}
