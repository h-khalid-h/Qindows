//! # Compute Auction Bid Cap Bridge (Phase 258)
//!
//! ## Architecture Guardian: The Gap
//! `compute_auction.rs` implements `AuctionBid`:
//! - `AuctionBid { bid_id, provider_node, capacity: ComputeCapacity, price_per_tick, ... }`
//! - `ComputeCapacity::power_score()` → u64 — compute power level
//! - `ComputeCapacity::satisfies(needed)` → bool
//!
//! **Missing link**: `AuctionBid` capacity had no power_score cap.
//! A node could submit a bid declaring enormous capacity, triggering
//! expensive satisfies() traversal on every consumer request.
//!
//! This module provides `ComputeAuctionBidCapBridge`:
//! Max 1000 compute power-score per bid + Admin:EXEC for high bids.

extern crate alloc;

use crate::compute_auction::AuctionBid;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

const MAX_CAPACITY_WITHOUT_CAP: u64 = 1000;

#[derive(Debug, Default, Clone)]
pub struct AuctionBidCapStats {
    pub bids_allowed: u64,
    pub bids_denied:  u64,
}

pub struct ComputeAuctionBidCapBridge {
    pub stats: AuctionBidCapStats,
}

impl ComputeAuctionBidCapBridge {
    pub fn new() -> Self {
        ComputeAuctionBidCapBridge { stats: AuctionBidCapStats::default() }
    }

    /// Authorize a bid — large power_score bids require Admin:EXEC cap.
    pub fn authorize_bid(
        &mut self,
        bid: &AuctionBid,
        provider_silo_id: u64, // passed separately since AuctionBid uses provider_node
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        let score = bid.capacity.power_score();
        if score > MAX_CAPACITY_WITHOUT_CAP {
            if !forge.check(provider_silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
                self.stats.bids_denied += 1;
                crate::serial_println!(
                    "[AUCTION] Provider {} bid power_score {} denied — Admin:EXEC required",
                    bid.provider_node, score
                );
                return false;
            }
        }
        self.stats.bids_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AuctionBidCapBridge: allowed={} denied={}",
            self.stats.bids_allowed, self.stats.bids_denied
        );
    }
}
