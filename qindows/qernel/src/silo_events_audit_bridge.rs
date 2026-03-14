//! # Silo Events Audit Bridge (Phase 168)
//!
//! ## Architecture Guardian: The Gap
//! `silo_events.rs` `SiloEvent` variants:
//! - Spawned { silo_id, binary_oid, spawn_tick, initial_caps, parent_silo }
//! - Vaporized { silo_id, tick, cause: VaporizeCause, post_mortem_oid }
//! - Suspended { silo_id, tick, reason: SuspendReason }
//! - Resumed { silo_id, tick, suspended_for_ticks }
//! - Migrated { silo_id, fiber_id, server_node_id, tick }
//! - Recalled { silo_id, fiber_id, tick, ... }
//!
//! `SiloEvent::silo_id()` → u64, `SiloEvent::name()` → &str
//!
//! **Missing link**: SiloEvents were never forwarded to QAuditKernel.
//! Vaporize events occurred silently with no cryptographic audit trail.
//!
//! This module provides `SiloEventsAuditBridge` to wire events to audit.

extern crate alloc;
use alloc::format;

use crate::silo_events::{SiloEvent, VaporizeCause};
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct SiloEventBridgeStats {
    pub events_logged:  u64,
    pub vaporize_count: u64,
}

pub struct SiloEventsAuditBridge {
    pub stats: SiloEventBridgeStats,
}

impl SiloEventsAuditBridge {
    pub fn new() -> Self {
        SiloEventsAuditBridge { stats: SiloEventBridgeStats::default() }
    }

    /// Emit a SiloEvent to the audit log.
    pub fn emit(&mut self, event: &SiloEvent, audit: &mut QAuditKernel, tick: u64) {
        self.stats.events_logged += 1;
        let silo_id = event.silo_id();

        match event {
            SiloEvent::Vaporized { silo_id, cause, .. } => {
                self.stats.vaporize_count += 1;
                let reason = format!("vaporize: {}", cause.label());
                audit.log_silo_vaporize(*silo_id, &reason, tick);
            }
            SiloEvent::Suspended { silo_id, reason, .. } => {
                crate::serial_println!("[EVENT] Silo {} suspended: {:?}", silo_id, reason);
            }
            SiloEvent::Migrated { silo_id, server_node_id, .. } => {
                crate::serial_println!("[EVENT] Silo {} migrated → node {:x}", silo_id, server_node_id);
            }
            _ => {
                crate::serial_println!("[EVENT] Silo {} {}", silo_id, event.name());
            }
        }
    }

    /// Process a batch of buffered events.
    pub fn emit_batch(&mut self, events: &[SiloEvent], audit: &mut QAuditKernel, tick: u64) {
        for event in events {
            self.emit(event, audit, tick);
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SiloEventBridge: logged={} vaporize={}",
            self.stats.events_logged, self.stats.vaporize_count
        );
    }
}
