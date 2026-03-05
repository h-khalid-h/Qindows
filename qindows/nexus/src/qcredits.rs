//! # Q-Credits — Mesh Compute Billing
//!
//! Every Qindows device can contribute idle CPU/GPU/NPU cycles to the
//! Global Mesh (Section 11.1). Q-Credits are the accounting unit that
//! tracks compute contributions and consumption.
//!
//! How it works:
//! - You **earn** Q-Credits by lending idle cycles to the mesh
//! - You **spend** Q-Credits when offloading heavy tasks (Edge-Kernel)
//! - Credits are tracked per-device with tamper-proof signed receipts
//! - Anti-abuse: Sentinel monitors for fraudulent credit claims

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Transaction type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxType {
    /// Earned credits (contributed compute)
    Earned,
    /// Spent credits (consumed remote compute)
    Spent,
    /// Transferred to another device
    Transfer,
    /// Bonus (mesh participation reward)
    Bonus,
    /// Penalty (fraudulent claim deduction)
    Penalty,
}

/// A credit transaction.
#[derive(Debug, Clone)]
pub struct CreditTx {
    /// Transaction ID
    pub id: u64,
    /// Type
    pub tx_type: TxType,
    /// Amount (millicredits for precision)
    pub amount: u64,
    /// Device that earned/spent
    pub device_id: [u8; 32],
    /// Counterparty device (who we computed for / who computed for us)
    pub counterparty: Option<[u8; 32]>,
    /// Timestamp
    pub timestamp: u64,
    /// Description
    pub description: String,
    /// Signed receipt hash
    pub receipt_hash: [u8; 32],
}

/// Compute resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    CpuCycles,
    GpuUnits,
    NpuInference,
    StorageGb,
    BandwidthMb,
}

/// Rate card — how many millicredits per unit of compute.
#[derive(Debug, Clone)]
pub struct RateCard {
    pub cpu_per_second: u64,     // millicredits per CPU-second
    pub gpu_per_second: u64,     // millicredits per GPU-second
    pub npu_per_inference: u64,  // millicredits per NPU inference
    pub storage_per_gb_hour: u64,
    pub bandwidth_per_mb: u64,
}

impl Default for RateCard {
    fn default() -> Self {
        RateCard {
            cpu_per_second: 10,
            gpu_per_second: 50,
            npu_per_inference: 100,
            storage_per_gb_hour: 5,
            bandwidth_per_mb: 2,
        }
    }
}

/// Device credit account.
#[derive(Debug, Clone)]
pub struct CreditAccount {
    /// Device ID
    pub device_id: [u8; 32],
    /// Device name
    pub name: String,
    /// Balance (millicredits)
    pub balance: i64,
    /// Total earned
    pub total_earned: u64,
    /// Total spent
    pub total_spent: u64,
    /// Is this device trusted?
    pub trusted: bool,
    /// Last activity
    pub last_active: u64,
}

/// Q-Credits statistics.
#[derive(Debug, Clone, Default)]
pub struct CreditStats {
    pub transactions: u64,
    pub credits_earned: u64,
    pub credits_spent: u64,
    pub credits_transferred: u64,
    pub penalties_applied: u64,
}

/// The Q-Credits Manager.
pub struct QCredits {
    /// Device accounts
    pub accounts: BTreeMap<[u8; 32], CreditAccount>,
    /// Transaction log
    pub ledger: Vec<CreditTx>,
    /// Rate card
    pub rates: RateCard,
    /// Next transaction ID
    next_tx_id: u64,
    /// Statistics
    pub stats: CreditStats,
}

impl QCredits {
    pub fn new() -> Self {
        QCredits {
            accounts: BTreeMap::new(),
            ledger: Vec::new(),
            rates: RateCard::default(),
            next_tx_id: 1,
            stats: CreditStats::default(),
        }
    }

    /// Register a device.
    pub fn register(&mut self, device_id: [u8; 32], name: &str) {
        self.accounts.entry(device_id).or_insert_with(|| CreditAccount {
            device_id,
            name: String::from(name),
            balance: 1000, // Initial grant (1 credit = 1000 millicredits)
            total_earned: 0,
            total_spent: 0,
            trusted: true,
            last_active: 0,
        });
    }

    /// Earn credits for contributing compute.
    pub fn earn(
        &mut self,
        device_id: [u8; 32],
        resource: ResourceType,
        units: u64,
        counterparty: [u8; 32],
        now: u64,
    ) -> u64 {
        let rate = match resource {
            ResourceType::CpuCycles => self.rates.cpu_per_second,
            ResourceType::GpuUnits => self.rates.gpu_per_second,
            ResourceType::NpuInference => self.rates.npu_per_inference,
            ResourceType::StorageGb => self.rates.storage_per_gb_hour,
            ResourceType::BandwidthMb => self.rates.bandwidth_per_mb,
        };
        let amount = rate.saturating_mul(units);

        if let Some(account) = self.accounts.get_mut(&device_id) {
            account.balance += amount as i64;
            account.total_earned += amount;
            account.last_active = now;
        }

        self.record_tx(TxType::Earned, amount, device_id, Some(counterparty), "compute contribution", now);
        self.stats.credits_earned += amount;
        amount
    }

    /// Spend credits for consuming remote compute.
    pub fn spend(
        &mut self,
        device_id: [u8; 32],
        resource: ResourceType,
        units: u64,
        counterparty: [u8; 32],
        now: u64,
    ) -> Result<u64, &'static str> {
        let rate = match resource {
            ResourceType::CpuCycles => self.rates.cpu_per_second,
            ResourceType::GpuUnits => self.rates.gpu_per_second,
            ResourceType::NpuInference => self.rates.npu_per_inference,
            ResourceType::StorageGb => self.rates.storage_per_gb_hour,
            ResourceType::BandwidthMb => self.rates.bandwidth_per_mb,
        };
        let amount = rate.saturating_mul(units);

        let account = self.accounts.get_mut(&device_id)
            .ok_or("Account not found")?;

        if account.balance < amount as i64 {
            return Err("Insufficient credits");
        }

        account.balance -= amount as i64;
        account.total_spent += amount;
        account.last_active = now;

        self.record_tx(TxType::Spent, amount, device_id, Some(counterparty), "compute consumption", now);
        self.stats.credits_spent += amount;
        Ok(amount)
    }

    /// Apply a penalty (Sentinel: fraudulent credit claim).
    pub fn penalize(&mut self, device_id: [u8; 32], amount: u64, reason: &str, now: u64) {
        if let Some(account) = self.accounts.get_mut(&device_id) {
            account.balance -= amount as i64;
            account.trusted = account.balance >= 0;
        }
        self.record_tx(TxType::Penalty, amount, device_id, None, reason, now);
        self.stats.penalties_applied += 1;
    }

    /// Get balance for a device.
    pub fn balance(&self, device_id: &[u8; 32]) -> i64 {
        self.accounts.get(device_id).map(|a| a.balance).unwrap_or(0)
    }

    /// Record a transaction.
    fn record_tx(&mut self, tx_type: TxType, amount: u64, device_id: [u8; 32], counterparty: Option<[u8; 32]>, desc: &str, now: u64) {
        let id = self.next_tx_id;
        self.next_tx_id += 1;

        // Generate receipt hash (simplified)
        let mut hash = [0u8; 32];
        let id_bytes = id.to_le_bytes();
        for i in 0..8 { hash[i] = id_bytes[i]; }
        for i in 0..8 { hash[8 + i] = amount.to_le_bytes()[i]; }

        self.ledger.push(CreditTx {
            id, tx_type, amount, device_id, counterparty,
            timestamp: now,
            description: String::from(desc),
            receipt_hash: hash,
        });
        self.stats.transactions += 1;
    }
}
