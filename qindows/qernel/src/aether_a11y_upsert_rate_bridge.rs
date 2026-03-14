#![no_std]
extern crate alloc;

use alloc::collections::BTreeMap;
use crate::aether_a11y::{AetherA11yLayer, A11yNode};

/// Bridge for Phase 295: Aether A11y Node Upsert Rate Bridge
/// Restricts each Silo to a maximum of 512 accessibility node updates per tick to prevent DoS.
pub struct AetherA11yUpsertRateBridge<'a> {
    target: &'a mut AetherA11yLayer,
    upsert_counts: BTreeMap<u64, u32>,
    last_tick: u64,
}

impl<'a> AetherA11yUpsertRateBridge<'a> {
    pub fn new(target: &'a mut AetherA11yLayer) -> Self {
        Self {
            target,
            upsert_counts: BTreeMap::new(),
            last_tick: 0,
        }
    }

    pub fn upsert_node(&mut self, silo_id: u64, node: A11yNode, tick: u64) -> bool {
        if tick > self.last_tick {
            self.upsert_counts.clear();
            self.last_tick = tick;
        }

        let count = self.upsert_counts.entry(silo_id).or_insert(0);
        if *count >= 512 {
            crate::serial_println!(
                "[AETHER A11Y] Silo {} exceeded 512 A11y node upserts/tick. Dropping upsert request.", silo_id
            );
            return false;
        }

        *count += 1;
        self.target.upsert_node(silo_id, node);
        true
    }
}
