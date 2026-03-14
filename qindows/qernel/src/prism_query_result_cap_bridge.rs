//! # Prism Query Result Cap Bridge (Phase 263)
//!
//! ## Architecture Guardian: The Gap
//! `prism_query.rs` implements `PrismQuery`:
//! - `PrismQuery { filters: Vec<QueryFilter>, sort: SortOrder, ... }`
//! - `QueryFilter::matches(meta: &ObjectMeta)` → bool
//!
//! **Missing link**: Query result set size was unbounded. A query
//! across all objects in the Prism index could return millions of
//! `ObjectMeta` records, causing allocation pressure.
//!
//! This module provides `PrismQueryResultCapBridge`:
//! Max 10000 results per query enforced before result materialization.

extern crate alloc;

const MAX_QUERY_RESULTS: u64 = 10_000;

#[derive(Debug, Default, Clone)]
pub struct QueryResultCapStats {
    pub queries_ok:      u64,
    pub queries_capped:  u64,
}

pub struct PrismQueryResultCapBridge {
    pub stats: QueryResultCapStats,
}

impl PrismQueryResultCapBridge {
    pub fn new() -> Self {
        PrismQueryResultCapBridge { stats: QueryResultCapStats::default() }
    }

    /// Check result count — deny materializaion if exceeds cap.
    pub fn check_result_count(&mut self, result_count: u64, silo_id: u64) -> u64 {
        if result_count > MAX_QUERY_RESULTS {
            self.stats.queries_capped += 1;
            crate::serial_println!(
                "[PRISM QUERY] Silo {} results {} capped to {}", silo_id, result_count, MAX_QUERY_RESULTS
            );
            MAX_QUERY_RESULTS
        } else {
            self.stats.queries_ok += 1;
            result_count
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PrismQueryResultCap: ok={} capped={}", self.stats.queries_ok, self.stats.queries_capped
        );
    }
}
