//! # Compute Auction CapToken Bridge (Phase 140)
//!
//! ## Architecture Guardian: The Gap
//! `compute_auction.rs` implements:
//! - `AuctionBid` — provider bid with capacity/price
//! - `AwardedContract` — matched bid+request contract
//! - `CreditLedger` — per-node credit accounting with earn/reserve/release
//!
//! **Missing link**: The `CreditLedger` tracked credits but was never connected
//! to CapTokenForge for Law 8 enforcement. Also: the auction never checked
//! whether the bidding node had the `Energy` cap.
//!
//! This module provides `ComputeAuctionBridge`:
//! 1. `submit_bid_with_cap_check()` — verifies Energy cap before accepting bid
//! 2. `award_and_debit()` — records contract award, decrements reserved credits
//! 3. `donate_and_earn()` — credits ledger when donating CPU cycles (Law 8)

extern crate alloc;

use crate::compute_auction::{AuctionBid, AwardedContract, CreditLedger};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AuctionBridgeStats {
    pub bids_submitted:   u64,
    pub bids_cap_denied:  u64,
    pub contracts_awarded: u64,
    pub credits_donated:  u64,
}

// ── Compute Auction Bridge ────────────────────────────────────────────────────

/// Bridges ComputeAuction to CapTokenForge and CreditLedger.
pub struct ComputeAuctionBridge {
    pub ledger: CreditLedger,
    pub stats:  AuctionBridgeStats,
}

impl ComputeAuctionBridge {
    pub fn new() -> Self {
        ComputeAuctionBridge {
            ledger: CreditLedger::default(),
            stats:  AuctionBridgeStats::default(),
        }
    }

    /// Submit a bid after verifying the provider node has Energy cap (Law 8).
    /// `silo_id` = the Silo on this node that owns the energy cap.
    pub fn submit_bid_with_cap_check(
        &mut self,
        bid: &AuctionBid,
        silo_id: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        self.stats.bids_submitted += 1;

        // Law 8: Silo that initiated the bid must have Energy cap
        if !forge.check(silo_id, CapType::Energy, CAP_EXEC, 0, tick) {
            self.stats.bids_cap_denied += 1;
            crate::serial_println!(
                "[AUCTION BRIDGE] Bid {} denied — Silo {} has no Energy cap",
                bid.bid_id, silo_id
            );
            return false;
        }

        // Reserve credits for the potential contract cost
        let credits_needed = bid.price_per_tick.saturating_mul(bid.min_duration_ticks);
        let reserved = self.ledger.reserve(credits_needed);
        if !reserved {
            crate::serial_println!(
                "[AUCTION BRIDGE] Bid {} denied — insufficient credits (need {})",
                bid.bid_id, credits_needed
            );
            return false;
        }

        crate::serial_println!(
            "[AUCTION BRIDGE] Bid {} accepted from node {} ({} credits/tick reserved {})",
            bid.bid_id, bid.provider_node, bid.price_per_tick, credits_needed
        );
        true
    }

    /// Award a contract: mark reserved credits as spent.
    pub fn award_and_debit(&mut self, contract: &AwardedContract) {
        self.stats.contracts_awarded += 1;
        // Release the reservation as spent
        self.ledger.release_reservation(contract.total_credits_reserved, true);
        crate::serial_println!(
            "[AUCTION BRIDGE] Contract {} awarded — {} credits spent",
            contract.contract_id, contract.total_credits_reserved
        );
    }

    /// Earn credits by donating CPU cycles to the mesh (Law 8).
    pub fn donate_and_earn(&mut self, silo_id: u64, cpu_ticks: u64) {
        self.ledger.issue_for_donation(cpu_ticks);
        self.stats.credits_donated += cpu_ticks;
        crate::serial_println!(
            "[AUCTION BRIDGE] Silo {} donated {}t CPU — credits earned", silo_id, cpu_ticks
        );
    }

    /// Cancel a bid: release reserved credits.
    pub fn cancel_bid(&mut self, credits_reserved: u64) {
        self.ledger.release_reservation(credits_reserved, false);
    }

    pub fn credit_balance(&self) -> u64 {
        self.ledger.balance
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AuctionBridge: bids={} denied={} awarded={} donated={}t balance={}",
            self.stats.bids_submitted, self.stats.bids_cap_denied,
            self.stats.contracts_awarded, self.stats.credits_donated, self.credit_balance()
        );
    }
}
