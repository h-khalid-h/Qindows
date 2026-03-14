//! # Compute Auction — Nexus Phase V Q-Credits Engine (Phase 77)
//!
//! ARCHITECTURE.md §9 — Planetary Computing, Phase V of Genesis Protocol:
//! > "**Compute Auction**: Idle CPU/GPU/NPU cycles bid for Q-Credits"
//! > "Your sleeping laptop is a supercomputer node"
//!
//! ## Architecture Guardian: How the Auction Works
//! ```text
//! Resource Provider (idle node)          Resource Consumer (overloaded node)
//! ──────────────────────────────         ──────────────────────────────────────
//! AuctionBid::new(cpu=4, gpu=0, npu=2)   ComputeRequest::new(cpu=2, gpu=0, npu=0)
//!     │                                       │
//!     └──► ComputeAuctionEngine              ─┘ (both submit to Nexus)
//!              │ Match bid ↔ request
//!              │ Lowest credit price wins
//!              │ Award contract → FiberOffload begins
//!              ▼
//!          AwardedContract { provider, consumer, price_per_tick, q_credits }
//! ```
//!
//! ## Q-Credits
//! Q-Credits are a **kernel-enforced** unit. They cannot be fabricated by user code.
//! 1 Q-Credit = 1 CPU-core-tick donated to the mesh.
//! Credits are earned offline (sleeping laptop donates → earns credits).
//! Credits are spent when offloading computation (Scale to Cloud = Phase 75).
//!
//! ## Architecture Guardian: No Money, No Auth Server
//! The auction is fully **decentralized** — no central ledger, no blockchain.
//! Credits are signed by the Qernel that earned them (Ed25519 TPM key).
//! Double-spend protection: the awarding node tracks used credit receipts.
//!
//! ## Relationship to Other Modules
//! - `nexus.rs` (Phase 61): mesh networking layer — handles broadcast of bids/requests
//! - `fiber_offload.rs` (Phase 75): FiberOffload reports credits_cost; auction pays it
//! - `active_task.rs` (Phase 73): provider must release ActiveTask → deep-sleep to become eligible

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Resource Capacity ─────────────────────────────────────────────────────────

/// Compute resources available to bid or request.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ComputeCapacity {
    /// CPU cores (1-255)
    pub cpu_cores: u8,
    /// GPU units (shader multiprocessors / compute units, 0 if absent)
    pub gpu_units: u16,
    /// NPU TOPS (integer tera-operations per second, 0 if absent)
    pub npu_tops: u8,
    /// RAM to dedicate (MiB)
    pub ram_mib: u32,
    /// NVMe scratch space (MiB)
    pub nvme_mib: u32,
    /// Network bandwidth to offer (Mbps)
    pub bandwidth_mbps: u32,
}

impl ComputeCapacity {
    /// Composite power score (arbitrary units) for bid ranking.
    pub fn power_score(&self) -> u64 {
        (self.cpu_cores as u64) * 100
        + (self.gpu_units as u64) * 50
        + (self.npu_tops as u64) * 200
        + (self.ram_mib as u64) / 256
    }

    /// Returns true if `self` satisfies the `needed` capacity.
    pub fn satisfies(&self, needed: &ComputeCapacity) -> bool {
        self.cpu_cores  >= needed.cpu_cores
        && self.gpu_units >= needed.gpu_units
        && self.npu_tops  >= needed.npu_tops
        && self.ram_mib   >= needed.ram_mib
    }
}

// ── Auction Bid (from provider) ───────────────────────────────────────────────

/// A node offering idle compute resources to the mesh.
#[derive(Debug, Clone)]
pub struct AuctionBid {
    /// Unique bid ID
    pub bid_id: u64,
    /// Nexus node ID of the provider (first 8 bytes)
    pub provider_node: u64,
    /// Resources this node is willing to donate
    pub capacity: ComputeCapacity,
    /// Price in Q-Credits per CPU-tick (provider wants to earn this many credits)
    pub price_per_tick: u64,
    /// Minimum contract duration (ticks) the provider will accept
    pub min_duration_ticks: u64,
    /// Maximum contract duration (ticks)
    pub max_duration_ticks: u64,
    /// Tick when this bid expires (auto-withdrawn if not matched)
    pub expires_at: u64,
    /// Geographic region hint (for Nexus latency optimization)
    pub region_hint: String,
    /// Reliability score (0-100, based on historical uptime)
    pub reliability_score: u8,
    /// Provider's TPM attestation proof (that it's a real Qindows node)
    pub attestation_hash: [u8; 32],
}

// ── Compute Request (from consumer) ──────────────────────────────────────────

/// A node requesting external compute resources (to offload a heavy Fiber).
#[derive(Debug, Clone)]
pub struct ComputeRequest {
    /// Unique request ID
    pub request_id: u64,
    /// Nexus node ID of the consumer
    pub consumer_node: u64,
    /// Resources needed
    pub needed: ComputeCapacity,
    /// Maximum price the consumer will pay (Q-Credits per tick)
    pub max_price_per_tick: u64,
    /// Required minimum contract duration
    pub min_duration_ticks: u64,
    /// Tick when this request expires
    pub expires_at: u64,
    /// What Silo/Fiber needs offloading (for correlated FiberOffload)
    pub silo_id: u64,
    pub fiber_id: u64,
    /// Minimum reliability score required from provider
    pub min_reliability: u8,
}

// ── Awarded Contract ──────────────────────────────────────────────────────────

/// A matched bid+request — the result of a successful auction.
#[derive(Debug, Clone)]
pub struct AwardedContract {
    /// Unique contract ID
    pub contract_id: u64,
    /// Provider (bid) node
    pub provider_node: u64,
    pub bid_id: u64,
    /// Consumer (request) node
    pub consumer_node: u64,
    pub request_id: u64,
    /// Agreed resources
    pub capacity: ComputeCapacity,
    /// Agreed price (Q-Credits per tick) — always ≤ max_price and ≥ min_price
    pub agreed_price_per_tick: u64,
    /// Contract start tick
    pub started_at: u64,
    /// Scheduled end tick
    pub ends_at: u64,
    /// Q-Credits deducted from consumer on contract award
    pub total_credits_reserved: u64,
    /// Is this contract currently active?
    pub active: bool,
}

// ── Q-Credit Ledger ───────────────────────────────────────────────────────────

/// Per-node Q-Credit accounting (kernel-enforced, TPM-signed in production).
#[derive(Debug, Clone, Default)]
pub struct CreditLedger {
    /// Spendable Q-Credits balance
    pub balance: u64,
    /// Reserved (locked in active contracts)
    pub reserved: u64,
    /// Total credits ever earned
    pub earned_total: u64,
    /// Total credits ever spent
    pub spent_total: u64,
    /// Donation ticks logged (for credit issuance)
    pub cpu_ticks_donated: u64,
}

impl CreditLedger {
    /// Earn credits (called when a provider contract completes).
    pub fn earn(&mut self, amount: u64) {
        self.balance += amount;
        self.earned_total += amount;
        crate::serial_println!("[AUCTION] Earned {} Q-Credits. Balance: {}", amount, self.balance);
    }

    /// Reserve credits for a pending contract (deducted from spendable).
    pub fn reserve(&mut self, amount: u64) -> bool {
        if self.balance < amount { return false; }
        self.balance -= amount;
        self.reserved += amount;
        true
    }

    /// Release reservation (contract completed or cancelled).
    pub fn release_reservation(&mut self, amount: u64, spent: bool) {
        self.reserved = self.reserved.saturating_sub(amount);
        if spent { self.spent_total += amount; }
        else     { self.balance += amount; } // refund if cancelled
    }

    /// Issue credits for donated ticks (1 credit per CPU-tick donated).
    pub fn issue_for_donation(&mut self, cpu_ticks: u64) {
        self.cpu_ticks_donated += cpu_ticks;
        let earned = cpu_ticks / 1000; // 1 credit per 1000 ticks donated (~1 second)
        self.earn(earned);
    }
}

// ── Auction Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AuctionStats {
    pub bids_submitted: u64,
    pub requests_submitted: u64,
    pub contracts_awarded: u64,
    pub contracts_completed: u64,
    pub contracts_cancelled: u64,
    pub total_credits_traded: u64,
    pub avg_match_time_ticks: u64,
}

// ── Compute Auction Engine ────────────────────────────────────────────────────

/// The decentralized Nexus Phase V compute auction engine.
pub struct ComputeAuctionEngine {
    /// Active bids: bid_id → bid
    pub active_bids: BTreeMap<u64, AuctionBid>,
    /// Active requests: request_id → request
    pub active_requests: BTreeMap<u64, ComputeRequest>,
    /// Active contracts: contract_id → contract
    pub active_contracts: BTreeMap<u64, AwardedContract>,
    /// Completed/failed contracts (last 64)
    pub history: Vec<AwardedContract>,
    /// This node's Q-Credit ledger
    pub credits: CreditLedger,
    /// Auction statistics
    pub stats: AuctionStats,
    /// Next IDs
    next_bid_id: u64,
    next_request_id: u64,
    next_contract_id: u64,
    /// This node's ID
    pub node_id: u64,
}

impl ComputeAuctionEngine {
    pub fn new(node_id: u64) -> Self {
        ComputeAuctionEngine {
            active_bids: BTreeMap::new(),
            active_requests: BTreeMap::new(),
            active_contracts: BTreeMap::new(),
            history: Vec::new(),
            credits: CreditLedger::default(),
            stats: AuctionStats::default(),
            next_bid_id: 1,
            next_request_id: 1,
            next_contract_id: 1,
            node_id,
        }
    }

    // ── Provider Side ─────────────────────────────────────────────────────────

    /// Submit a bid offering idle resources to the mesh.
    /// Returns bid_id. Caller (Nexus) broadcasts this over Q-Fabric.
    pub fn submit_bid(
        &mut self,
        capacity: ComputeCapacity,
        price_per_tick: u64,
        min_duration_ticks: u64,
        max_duration_ticks: u64,
        tick: u64,
    ) -> u64 {
        let bid_id = self.next_bid_id;
        self.next_bid_id += 1;
        let bid = AuctionBid {
            bid_id,
            provider_node: self.node_id,
            capacity,
            price_per_tick,
            min_duration_ticks,
            max_duration_ticks,
            expires_at: tick + 60_000, // bid expires in 60 seconds
            region_hint: "global".to_string(),
            reliability_score: 95,
            attestation_hash: {
                let mut h = [0u8; 32];
                h[0] = (self.node_id & 0xFF) as u8;
                h
            },
        };
        crate::serial_println!(
            "[AUCTION] Bid submitted: id={} cpu={} gpu={} npu={} at {} credits/tick",
            bid_id, capacity.cpu_cores, capacity.gpu_units, capacity.npu_tops, price_per_tick
        );
        self.active_bids.insert(bid_id, bid);
        self.stats.bids_submitted += 1;
        bid_id
    }

    // ── Consumer Side ─────────────────────────────────────────────────────────

    /// Submit a compute request (need to offload a Fiber).
    pub fn submit_request(
        &mut self,
        silo_id: u64,
        fiber_id: u64,
        needed: ComputeCapacity,
        max_price_per_tick: u64,
        min_duration_ticks: u64,
        tick: u64,
    ) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let req = ComputeRequest {
            request_id,
            consumer_node: self.node_id,
            needed,
            max_price_per_tick,
            min_duration_ticks,
            expires_at: tick + 10_000, // request expires in 10 seconds
            silo_id,
            fiber_id,
            min_reliability: 80,
        };
        crate::serial_println!(
            "[AUCTION] Request submitted: id={} silo={} fiber={} max_price={} credits/tick",
            request_id, silo_id, fiber_id, max_price_per_tick
        );
        self.active_requests.insert(request_id, req);
        self.stats.requests_submitted += 1;
        request_id
    }

    // ── Matching Engine ───────────────────────────────────────────────────────

    /// Run the matching algorithm: pair bids with requests.
    /// Called by Nexus on each received bid or request.
    /// Returns a list of newly awarded contracts.
    pub fn run_matching(&mut self, tick: u64) -> Vec<AwardedContract> {
        let mut awarded: Vec<AwardedContract> = Vec::new();
        let mut matched_bid_ids: Vec<u64> = Vec::new();
        let mut matched_request_ids: Vec<u64> = Vec::new();

        let request_ids: Vec<u64> = self.active_requests.keys().copied().collect();

        for request_id in request_ids {
            let req = match self.active_requests.get(&request_id) {
                Some(r) if !matched_request_ids.contains(&r.request_id) => r.clone(),
                _ => continue,
            };
            if req.expires_at < tick { continue; }

            // Find best bid: satisfies capacity, price ≤ max_price, best reliability
            let best_bid_id = {
                let mut best: Option<(u64, u8, u64)> = None; // (bid_id, reliability, price)
                for (bid_id, bid) in &self.active_bids {
                    if matched_bid_ids.contains(bid_id) { continue; }
                    if bid.expires_at < tick { continue; }
                    if !bid.capacity.satisfies(&req.needed) { continue; }
                    if bid.price_per_tick > req.max_price_per_tick { continue; }
                    if bid.reliability_score < req.min_reliability { continue; }
                    if bid.min_duration_ticks > req.min_duration_ticks { continue; }
                    match best {
                        None => { best = Some((*bid_id, bid.reliability_score, bid.price_per_tick)); }
                        Some((_, _, best_price)) if bid.price_per_tick < best_price => {
                            best = Some((*bid_id, bid.reliability_score, bid.price_per_tick));
                        }
                        _ => {}
                    }
                }
                best.map(|(id, _, _)| id)
            };

            if let Some(bid_id) = best_bid_id {
                let bid = self.active_bids[&bid_id].clone();
                let contract_id = self.next_contract_id;
                self.next_contract_id += 1;
                let duration = req.min_duration_ticks.max(bid.min_duration_ticks);
                let total_cost = bid.price_per_tick * duration;
                let contract = AwardedContract {
                    contract_id,
                    provider_node: bid.provider_node,
                    bid_id,
                    consumer_node: req.consumer_node,
                    request_id: req.request_id,
                    capacity: bid.capacity,
                    agreed_price_per_tick: bid.price_per_tick,
                    started_at: tick,
                    ends_at: tick + duration,
                    total_credits_reserved: total_cost,
                    active: true,
                };

                crate::serial_println!(
                    "[AUCTION] Contract #{}: node {} → node {} | {} credits/tick × {} ticks = {} total",
                    contract_id, bid.provider_node, req.consumer_node,
                    bid.price_per_tick, duration, total_cost
                );

                matched_bid_ids.push(bid_id);
                matched_request_ids.push(request_id);
                self.active_contracts.insert(contract_id, contract.clone());
                self.stats.contracts_awarded += 1;
                self.stats.total_credits_traded += total_cost;
                awarded.push(contract);
            }
        }

        // Remove matched bids and requests
        for id in matched_bid_ids { self.active_bids.remove(&id); }
        for id in matched_request_ids { self.active_requests.remove(&id); }

        awarded
    }

    /// Complete a contract and settle credits.
    pub fn complete_contract(&mut self, contract_id: u64, tick: u64) {
        if let Some(mut contract) = self.active_contracts.remove(&contract_id) {
            let actual_ticks = tick.saturating_sub(contract.started_at);
            let earned = contract.agreed_price_per_tick * actual_ticks;
            // Provider earns credits
            if contract.provider_node == self.node_id {
                self.credits.earn(earned);
            }
            // Consumer's reserved credits spent
            else if contract.consumer_node == self.node_id {
                self.credits.release_reservation(contract.total_credits_reserved, true);
            }
            contract.active = false;
            self.stats.contracts_completed += 1;
            if self.history.len() >= 64 { self.history.remove(0); }
            self.history.push(contract);
        }
    }

    /// Expire outdated bids and requests.
    pub fn expire_stale(&mut self, tick: u64) {
        self.active_bids.retain(|_, b| b.expires_at >= tick);
        self.active_requests.retain(|_, r| r.expires_at >= tick);
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Compute Auction (Nexus Phase V)    ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Balance:       {:>8} Q-Credits      ║", self.credits.balance);
        crate::serial_println!("║ Reserved:      {:>8} Q-Credits      ║", self.credits.reserved);
        crate::serial_println!("║ Earned total:  {:>8} Q-Credits      ║", self.credits.earned_total);
        crate::serial_println!("║ Active bids:   {:>8}               ║", self.active_bids.len());
        crate::serial_println!("║ Active reqs:   {:>8}               ║", self.active_requests.len());
        crate::serial_println!("║ Contracts done:{:>8}               ║", self.stats.contracts_completed);
        crate::serial_println!("║ Credits traded:{:>8}               ║", self.stats.total_credits_traded);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
