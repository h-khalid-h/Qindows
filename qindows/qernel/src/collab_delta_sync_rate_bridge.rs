#![no_std]
extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crate::collab::{CollabSession, CrdtOp};
use crate::nexus::NodeId;

/// Bridge for Phase 300: Collab Session Delta Sync Rate Bridge
/// Limits how often a peer can request a full CRDT operation delta sync.
pub struct CollabDeltaSyncRateBridge<'a> {
    target: &'a CollabSession,
    sync_counts: BTreeMap<NodeId, u32>,
    last_tick: u64,
}

impl<'a> CollabDeltaSyncRateBridge<'a> {
    pub fn new(target: &'a CollabSession) -> Self {
        Self {
            target,
            sync_counts: BTreeMap::new(),
            last_tick: 0,
        }
    }

    pub fn build_catchup(
        &mut self,
        peer: NodeId,
        local_node: NodeId,
        tick: u64,
    ) -> Option<Vec<CrdtOp>> {
        if tick > self.last_tick {
            self.sync_counts.clear();
            self.last_tick = tick;
        }

        let count = self.sync_counts.entry(peer).or_insert(0);
        if *count >= 16 {
            crate::serial_println!(
                "[COLLAB SYNC] Peer {:08x} exceeded 16 delta syncs/tick. Dropping catchup request.",
                peer.short_hex()
            );
            return None; // Deny sync due to rate limit violation
        }

        *count += 1;
        Some(self.target.build_catchup(peer, local_node))
    }
}
