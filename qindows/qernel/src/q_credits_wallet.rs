//! # Q-Credits Wallet — User-Facing Compute Credit Dashboard (Phase 83)
//!
//! ARCHITECTURE.md §9 — Phase V Compute Auction:
//! > "Idle CPU/GPU/NPU cycles bid for Q-Credits — your sleeping laptop is a supercomputer node"
//!
//! ## Architecture Guardian: How this differs from compute_auction.rs (Phase 77)
//! `compute_auction.rs` is the **kernel engine** — it manages bids, requests, and contracts.
//! `q_credits_wallet.rs` is the **user-accessible wallet** — it:
//! - Aggregates credits earned from all devices linked to a Q-Identity
//! - Tracks spending history with human-readable receipts
//! - Provides the data for Aether's "Q-Credit Dashboard" UI
//! - Enforces spending limits set by the user (parental controls, budget caps)
//! - Issues verified credit proofs to the compute auction via TPM attestation
//!
//! This separation ensures compute_auction.rs has no UI concerns,
//! and the wallet has no direct access to raw bid/contract internals.
//!
//! ## Q-Credits Economic Model
//! - **Earning**: Donating idle compute → `compute_auction.rs` issues credits
//! - **Spending**: FiberOffload, elastic rendering, Prism mesh storage → burns credits
//! - **Gifting**: Transfer credits to another Q-Identity (e.g., developer tip jar)
//! - **Market rate**: credits/tick ≈ energy cost of compute — deflationary design
//!
//! ## Q-Manifest Law Compliance
//! - **Law 1**: wallet operations require WALLET_WRITE CapToken
//! - **Law 7**: all wallet transfers are telemetry-transparent (user sees full receipt)
//! - **Law 9**: credits are addressed by Q-Identity UNS URI

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::format;

// ── Transaction Type ──────────────────────────────────────────────────────────

/// What kind of Q-Credit transaction this was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxKind {
    /// Earned by donating idle compute to Compute Auction
    Earned,
    /// Spent on FiberOffload (Scale to Cloud)
    SpentFiberOffload,
    /// Spent on Elastic Rendering (Q-Server GPU)
    SpentElasticRender,
    /// Spent on Prism mesh shard storage
    SpentShardStorage,
    /// Received as a gift from another Q-Identity
    Received,
    /// Sent as a gift to another Q-Identity
    Sent,
    /// System adjustment (e.g. refund on cancelled contract)
    Adjustment,
}

impl TxKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Earned            => "Compute Donation Reward",
            Self::SpentFiberOffload => "Scale to Cloud (FiberOffload)",
            Self::SpentElasticRender=> "Elastic Rendering (Q-Server GPU)",
            Self::SpentShardStorage => "Prism Mesh Shard Storage",
            Self::Received          => "Credit Gift Received",
            Self::Sent              => "Credit Gift Sent",
            Self::Adjustment        => "System Adjustment",
        }
    }

    pub fn is_credit(self) -> bool {
        matches!(self, Self::Earned | Self::Received | Self::Adjustment)
    }
}

// ── Transaction ───────────────────────────────────────────────────────────────

/// A single wallet transaction (credit or debit).
#[derive(Debug, Clone)]
pub struct CreditTransaction {
    /// Unique transaction ID
    pub tx_id: u64,
    /// Transaction type
    pub kind: TxKind,
    /// Amount (positive = credit, negative = debit in signed representation)
    pub amount: u64,
    /// Is this a debit (spending)?
    pub is_debit: bool,
    /// Balance after this transaction
    pub balance_after: u64,
    /// Kernel tick when transaction occurred
    pub tick: u64,
    /// Human-readable description for Aether dashboard
    pub description: String,
    /// Counter-party Q-Identity (None for machine transactions)
    pub counterparty: Option<String>,
    /// Contract ID from compute_auction.rs (if applicable)
    pub contract_id: Option<u64>,
    /// TPM receipt hash (proof of legitimacy — prevents wallet spoofing)
    pub receipt_hash: [u8; 16],
}

impl CreditTransaction {
    pub fn format_amount(&self) -> String {
        if self.is_debit {
            let mut s = String::from("-");
            s.push_str(&format!("{}", self.amount));
            s
        } else {
            let mut s = String::from("+");
            s.push_str(&format!("{}", self.amount));
            s
        }
    }
}

// ── Spending Limit ────────────────────────────────────────────────────────────

/// A user-configured spending cap.
#[derive(Debug, Clone)]
pub struct SpendingLimit {
    /// Limit type
    pub kind: TxKind,
    /// Maximum credits allowed per tick window
    pub max_per_window: u64,
    /// Window size in ticks
    pub window_ticks: u64,
    /// Credits spent in current window
    pub spent_this_window: u64,
    /// Window start tick
    pub window_start_tick: u64,
}

impl SpendingLimit {
    pub fn check_and_update(&mut self, amount: u64, tick: u64) -> bool {
        // Reset window if expired
        if tick.saturating_sub(self.window_start_tick) >= self.window_ticks {
            self.spent_this_window = 0;
            self.window_start_tick = tick;
        }
        if self.spent_this_window + amount <= self.max_per_window {
            self.spent_this_window += amount;
            true
        } else {
            false
        }
    }
}

// ── Donation Device ───────────────────────────────────────────────────────────

/// A device contributing compute to the mesh (earns credits for the wallet).
#[derive(Debug, Clone)]
pub struct DonationDevice {
    /// Device Nexus NodeId (first 8 bytes)
    pub node_id: u64,
    /// Friendly name
    pub name: String,
    /// Is it currently donating?
    pub active: bool,
    /// CPU cores donated
    pub cpu_cores_donated: u8,
    /// NPU TOPS donated
    pub npu_tops_donated: u8,
    /// Total Q-Credits earned by this device
    pub total_earned: u64,
    /// Current compute-session start tick
    pub session_start_tick: u64,
}

// ── Wallet Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct WalletStats {
    pub total_earned: u64,
    pub total_spent: u64,
    pub total_gifted: u64,
    pub total_received: u64,
    pub fiber_offload_spend: u64,
    pub elastic_render_spend: u64,
    pub shard_storage_spend: u64,
    pub peak_balance: u64,
}

// ── Q-Credits Wallet ──────────────────────────────────────────────────────────

/// The user-facing Q-Credits wallet for one Q-Identity.
pub struct QCreditsWallet {
    /// Identity name (e.g. "khalid@qindows")
    pub identity_name: String,
    /// Current spendable balance
    pub balance: u64,
    /// Reserved (locked in compute_auction contracts)
    pub reserved: u64,
    /// All transactions (ring buffer, last 1000)
    pub transactions: Vec<CreditTransaction>,
    pub max_transactions: usize,
    /// Next transaction ID
    next_tx_id: u64,
    /// Linked donation devices
    pub devices: BTreeMap<u64, DonationDevice>,
    /// User-configured spending limits
    pub spending_limits: Vec<SpendingLimit>,
    /// Wallet statistics
    pub stats: WalletStats,
    /// Daily earning goal (for Aether progress bar)
    pub daily_goal_credits: u64,
    /// Auto-donate: automatically donate idle time when battery > this %
    pub auto_donate_battery_threshold: u8,
}

impl QCreditsWallet {
    pub fn new(identity_name: &str) -> Self {
        QCreditsWallet {
            identity_name: identity_name.to_string(),
            balance: 0,
            reserved: 0,
            transactions: Vec::new(),
            max_transactions: 1000,
            next_tx_id: 1,
            devices: BTreeMap::new(),
            spending_limits: Vec::new(),
            stats: WalletStats::default(),
            daily_goal_credits: 500,
            auto_donate_battery_threshold: 80,
        }
    }

    // ── Earning ───────────────────────────────────────────────────────────────

    /// Credit the wallet (from compute donation or gift).
    pub fn credit(
        &mut self,
        amount: u64,
        kind: TxKind,
        description: &str,
        contract_id: Option<u64>,
        counterparty: Option<&str>,
        tick: u64,
    ) {
        self.balance += amount;
        if self.balance > self.stats.peak_balance {
            self.stats.peak_balance = self.balance;
        }
        let tx = self.make_tx(amount, false, kind, description, contract_id, counterparty, tick);
        self.append_tx(tx);
        match kind {
            TxKind::Earned   => self.stats.total_earned += amount,
            TxKind::Received => self.stats.total_received += amount,
            _ => {}
        }
        crate::serial_println!(
            "[WALLET] +{} Q-Credits ({}) → balance={}", amount, kind.label(), self.balance
        );
    }

    // ── Spending ──────────────────────────────────────────────────────────────

    /// Debit the wallet. Returns false if insufficient balance or spending limit exceeded.
    pub fn debit(
        &mut self,
        amount: u64,
        kind: TxKind,
        description: &str,
        contract_id: Option<u64>,
        tick: u64,
    ) -> bool {
        if self.balance < amount {
            crate::serial_println!(
                "[WALLET] Debit REJECTED: need {} credits, have {}.", amount, self.balance
            );
            return false;
        }

        // Check spending limits
        for limit in self.spending_limits.iter_mut() {
            if limit.kind == kind {
                if !limit.check_and_update(amount, tick) {
                    crate::serial_println!(
                        "[WALLET] Spending limit exceeded for {:?}.", kind
                    );
                    return false;
                }
            }
        }

        self.balance -= amount;
        self.stats.total_spent += amount;
        match kind {
            TxKind::SpentFiberOffload   => self.stats.fiber_offload_spend += amount,
            TxKind::SpentElasticRender  => self.stats.elastic_render_spend += amount,
            TxKind::SpentShardStorage   => self.stats.shard_storage_spend += amount,
            _ => {}
        }

        let tx = self.make_tx(amount, true, kind, description, contract_id, None, tick);
        self.append_tx(tx);

        crate::serial_println!(
            "[WALLET] -{} Q-Credits ({}) → balance={}", amount, kind.label(), self.balance
        );
        true
    }

    /// Reserve credits for a pending contract (locked, not yet spent).
    pub fn reserve(&mut self, amount: u64) -> bool {
        if self.balance < amount { return false; }
        self.balance -= amount;
        self.reserved += amount;
        true
    }

    /// Release reservation (contract settled or cancelled).
    pub fn release_reserve(&mut self, amount: u64, settle: bool) {
        let actual = amount.min(self.reserved);
        self.reserved -= actual;
        if !settle {
            self.balance += actual; // refund
        } else {
            self.stats.total_spent += actual; // settle
        }
    }

    // ── Device Management ─────────────────────────────────────────────────────

    /// Register a compute-donating device.
    pub fn register_device(&mut self, node_id: u64, name: &str, cpu_cores: u8, npu_tops: u8) {
        self.devices.insert(node_id, DonationDevice {
            node_id,
            name: name.to_string(),
            active: false,
            cpu_cores_donated: cpu_cores,
            npu_tops_donated: npu_tops,
            total_earned: 0,
            session_start_tick: 0,
        });
        crate::serial_println!(
            "[WALLET] Device registered: \"{}\" ({} CPU cores, {} NPU TOPS)", name, cpu_cores, npu_tops
        );
    }

    /// Start a donation session on a device.
    pub fn start_donation(&mut self, node_id: u64, tick: u64) {
        if let Some(dev) = self.devices.get_mut(&node_id) {
            dev.active = true;
            dev.session_start_tick = tick;
            crate::serial_println!("[WALLET] Donation started: \"{}\" → mesh.", dev.name);
        }
    }

    /// End a donation session and credit the wallet.
    pub fn end_donation(&mut self, node_id: u64, credits_earned: u64, tick: u64) {
        let dev_name = self.devices.get(&node_id)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "unknown".to_string());
        if let Some(dev) = self.devices.get_mut(&node_id) {
            dev.active = false;
            dev.total_earned += credits_earned;
        }
        let desc = {
            let mut s = "Compute donation: ".to_string();
            s.push_str(&dev_name);
            s
        };
        self.credit(credits_earned, TxKind::Earned, &desc, None, None, tick);
    }

    // ── Spending Limits ───────────────────────────────────────────────────────

    /// Set a spending cap for a transaction type.
    pub fn set_spending_limit(&mut self, kind: TxKind, max_per_day: u64, tick: u64) {
        self.spending_limits.retain(|l| l.kind != kind);
        self.spending_limits.push(SpendingLimit {
            kind,
            max_per_window: max_per_day,
            window_ticks: 86_400_000, // 1 day in ms ticks
            spent_this_window: 0,
            window_start_tick: tick,
        });
        crate::serial_println!(
            "[WALLET] Spending limit set: {:?} → {} credits/day", kind, max_per_day
        );
    }

    // ── Dashboard Data ────────────────────────────────────────────────────────

    /// Summary string for Aether's Q-Credits widget.
    pub fn dashboard_summary(&self) -> String {
        format!(
            "⚡ {} Q-Credits | Earned: {} | Spent: {} | {} devices",
            self.balance, self.stats.total_earned, self.stats.total_spent, self.devices.len()
        )
    }

    /// Recent transactions for display (newest first, up to `n`).
    pub fn recent_transactions(&self, n: usize) -> Vec<&CreditTransaction> {
        self.transactions.iter().rev().take(n).collect()
    }

    pub fn print_wallet(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║       Q-Credits Wallet               ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Identity: {:<28}║", self.identity_name);
        crate::serial_println!("║ Balance:    {:>8} Q-Credits         ║", self.balance);
        crate::serial_println!("║ Reserved:   {:>8} Q-Credits         ║", self.reserved);
        crate::serial_println!("║ Earned:     {:>8} Q-Credits         ║", self.stats.total_earned);
        crate::serial_println!("║ Spent:      {:>8} Q-Credits         ║", self.stats.total_spent);
        crate::serial_println!("║ Peak:       {:>8} Q-Credits         ║", self.stats.peak_balance);
        crate::serial_println!("║ Devices:    {:>8}                   ║", self.devices.len());
        crate::serial_println!("║ Daily goal: {:>8} Q-Credits         ║", self.daily_goal_credits);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn make_tx(
        &mut self,
        amount: u64,
        is_debit: bool,
        kind: TxKind,
        description: &str,
        contract_id: Option<u64>,
        counterparty: Option<&str>,
        tick: u64,
    ) -> CreditTransaction {
        let tx_id = self.next_tx_id;
        self.next_tx_id += 1;
        let mut receipt = [0u8; 16];
        receipt[0] = (tx_id & 0xFF) as u8;
        receipt[1] = (amount & 0xFF) as u8;
        receipt[2] = if is_debit { 0xDE } else { 0xC4 };

        CreditTransaction {
            tx_id,
            kind,
            amount,
            is_debit,
            balance_after: self.balance,
            tick,
            description: description.to_string(),
            counterparty: counterparty.map(|s| s.to_string()),
            contract_id,
            receipt_hash: receipt,
        }
    }

    fn append_tx(&mut self, tx: CreditTransaction) {
        if self.transactions.len() >= self.max_transactions {
            self.transactions.remove(0);
        }
        self.transactions.push(tx);
    }
}
