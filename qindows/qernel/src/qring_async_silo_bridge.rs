//! # QRing Async Silo Bridge (Phase 216)
//!
//! ## Architecture Guardian: The Gap
//! `qring_async.rs` implements async Q-Ring I/O for Silos:
//! - `SiloRing { silo_id, sq/cq entries }` — per-Silo submission/completion queues
//! - `SqEntry { opcode: SqOpcode, ... }` — submission queue entry
//! - `SqOpcode` — ReadV, WriteV, Send, Recv, Accept, Fsync, ...
//!
//! **Missing link**: SiloRing was created without a quota on SqEntry depth.
//! A Silo could create an infinite-depth ring, exhausting kernel memory.
//!
//! This module provides `QRingAsyncSiloBridge`:
//! Max ring depth of 4096 entries per Silo.

extern crate alloc;

use crate::qring_async::SiloRing;

const MAX_RING_DEPTH: usize = 4096;

#[derive(Debug, Default, Clone)]
pub struct QRingAsyncStats {
    pub rings_created:  u64,
    pub depth_capped:   u64,
}

pub struct QRingAsyncSiloBridge {
    pub stats: QRingAsyncStats,
}

impl QRingAsyncSiloBridge {
    pub fn new() -> Self {
        QRingAsyncSiloBridge { stats: QRingAsyncStats::default() }
    }

    /// Create a SiloRing with depth capped at MAX_RING_DEPTH (4096).
    pub fn create_ring(&mut self, silo_id: u64, requested_depth: usize) -> SiloRing {
        self.stats.rings_created += 1;
        let depth = if requested_depth > MAX_RING_DEPTH {
            self.stats.depth_capped += 1;
            crate::serial_println!(
                "[QRING ASYNC] Silo {} requested depth {}, capped to {}", silo_id, requested_depth, MAX_RING_DEPTH
            );
            MAX_RING_DEPTH
        } else { requested_depth };
        SiloRing::new(silo_id, depth)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QRingAsyncBridge: rings={} capped={}",
            self.stats.rings_created, self.stats.depth_capped
        );
    }
}
