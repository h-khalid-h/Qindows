//! # Q-Tiering — Hot/Warm/Cold Data Placement
//!
//! Automatically moves Q-Objects between storage tiers
//! based on access patterns (Section 3.15).
//!
//! Features:
//! - Hot tier: NVMe/RAM (frequently accessed)
//! - Warm tier: SSD (occasionally accessed)
//! - Cold tier: HDD/archive (rarely accessed)
//! - Access tracking with exponential decay
//! - Promotion/demotion policies

extern crate alloc;

use alloc::collections::BTreeMap;

/// Storage tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Hot = 0,
    Warm = 1,
    Cold = 2,
    Archive = 3,
}

/// Tier transition direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierMove {
    Promote,
    Demote,
}

/// Object placement metadata.
#[derive(Debug, Clone)]
pub struct TierEntry {
    pub oid: u64,
    pub silo_id: u64,
    pub tier: Tier,
    pub size: u64,
    pub access_score: f32,
    pub last_access: u64,
    pub access_count: u64,
    pub placed_at: u64,
}

/// Tier capacity and usage.
#[derive(Debug, Clone)]
pub struct TierInfo {
    pub tier: Tier,
    pub capacity: u64,
    pub used: u64,
}

/// Tiering statistics.
#[derive(Debug, Clone, Default)]
pub struct TierStats {
    pub promotions: u64,
    pub demotions: u64,
    pub bytes_promoted: u64,
    pub bytes_demoted: u64,
    pub scans: u64,
}

/// The Q-Tiering Engine.
pub struct QTiering {
    pub entries: BTreeMap<u64, TierEntry>,
    pub tiers: BTreeMap<u8, TierInfo>,
    pub hot_threshold: f32,
    pub cold_threshold: f32,
    pub decay_factor: f32,
    pub stats: TierStats,
}

impl QTiering {
    pub fn new() -> Self {
        let mut tiers = BTreeMap::new();
        tiers.insert(0, TierInfo { tier: Tier::Hot, capacity: 100 * 1024 * 1024 * 1024, used: 0 });
        tiers.insert(1, TierInfo { tier: Tier::Warm, capacity: 500 * 1024 * 1024 * 1024, used: 0 });
        tiers.insert(2, TierInfo { tier: Tier::Cold, capacity: 2000u64 * 1024 * 1024 * 1024, used: 0 });

        QTiering {
            entries: BTreeMap::new(),
            tiers,
            hot_threshold: 10.0,
            cold_threshold: 1.0,
            decay_factor: 0.95,
            stats: TierStats::default(),
        }
    }

    /// Place an object.
    pub fn place(&mut self, oid: u64, silo_id: u64, size: u64, tier: Tier, now: u64) {
        self.entries.insert(oid, TierEntry {
            oid, silo_id, tier, size,
            access_score: 5.0, last_access: now,
            access_count: 0, placed_at: now,
        });
        if let Some(t) = self.tiers.get_mut(&(tier as u8)) {
            t.used += size;
        }
    }

    /// Record an access.
    pub fn access(&mut self, oid: u64, now: u64) {
        if let Some(entry) = self.entries.get_mut(&oid) {
            entry.access_count += 1;
            entry.access_score += 1.0;
            entry.last_access = now;
        }
    }

    /// Scan and apply tiering policy.
    pub fn scan(&mut self) -> alloc::vec::Vec<(u64, TierMove)> {
        self.stats.scans += 1;
        let mut moves = alloc::vec::Vec::new();

        // Apply decay and collect candidates
        let oids: alloc::vec::Vec<u64> = self.entries.keys().copied().collect();
        for oid in oids {
            let entry = match self.entries.get_mut(&oid) {
                Some(e) => e,
                None => continue,
            };
            entry.access_score *= self.decay_factor;

            if entry.access_score >= self.hot_threshold && entry.tier != Tier::Hot {
                moves.push((oid, TierMove::Promote));
            } else if entry.access_score < self.cold_threshold && entry.tier == Tier::Hot {
                moves.push((oid, TierMove::Demote));
            }
        }

        // Apply moves
        for &(oid, direction) in &moves {
            if let Some(entry) = self.entries.get_mut(&oid) {
                let old_tier = entry.tier;
                let new_tier = match direction {
                    TierMove::Promote => match old_tier {
                        Tier::Archive => Tier::Cold,
                        Tier::Cold => Tier::Warm,
                        Tier::Warm | Tier::Hot => Tier::Hot,
                    },
                    TierMove::Demote => match old_tier {
                        Tier::Hot => Tier::Warm,
                        Tier::Warm => Tier::Cold,
                        Tier::Cold | Tier::Archive => Tier::Archive,
                    },
                };

                // Update tier usage
                if let Some(t) = self.tiers.get_mut(&(old_tier as u8)) {
                    t.used = t.used.saturating_sub(entry.size);
                }
                if let Some(t) = self.tiers.get_mut(&(new_tier as u8)) {
                    t.used += entry.size;
                }

                entry.tier = new_tier;
                match direction {
                    TierMove::Promote => {
                        self.stats.promotions += 1;
                        self.stats.bytes_promoted += entry.size;
                    }
                    TierMove::Demote => {
                        self.stats.demotions += 1;
                        self.stats.bytes_demoted += entry.size;
                    }
                }
            }
        }

        moves
    }
}
