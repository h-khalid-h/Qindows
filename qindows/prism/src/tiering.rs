//! # Prism Storage Tiering Engine
//!
//! Intelligent data placement across storage tiers:
//!   Hot (NVMe) → Warm (SSD) → Cold (HDD) → Archive (tape/cloud)
//!
//! Objects are scored by access frequency and recency. The tiering
//! engine periodically scans the heat map and schedules promotions
//! (cold→hot on access) and demotions (hot→cold on aging).

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::string::String;
use alloc::vec::Vec;

// ─── Storage Tiers ──────────────────────────────────────────────────────────

/// A storage tier with performance characteristics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TierClass {
    /// NVMe — fastest, most expensive, limited capacity
    Hot,
    /// SSD — fast, moderate cost
    Warm,
    /// HDD — slow, cheap, large capacity
    Cold,
    /// Tape/cloud — very slow, very cheap, archival
    Archive,
}

impl TierClass {
    pub fn name(&self) -> &'static str {
        match self {
            TierClass::Hot     => "NVMe (Hot)",
            TierClass::Warm    => "SSD (Warm)",
            TierClass::Cold    => "HDD (Cold)",
            TierClass::Archive => "Archive",
        }
    }

    /// Expected read latency in microseconds.
    pub fn latency_us(&self) -> u64 {
        match self {
            TierClass::Hot     => 10,
            TierClass::Warm    => 100,
            TierClass::Cold    => 5_000,
            TierClass::Archive => 500_000,
        }
    }

    /// Throughput in MB/s.
    pub fn throughput_mbps(&self) -> u64 {
        match self {
            TierClass::Hot     => 7_000,
            TierClass::Warm    => 500,
            TierClass::Cold    => 200,
            TierClass::Archive => 50,
        }
    }
}

/// A physical tier instance.
#[derive(Debug, Clone)]
pub struct StorageTier {
    /// Tier class
    pub class: TierClass,
    /// Human name (e.g., "nvme0", "sda")
    pub name: String,
    /// Total capacity in bytes
    pub capacity: u64,
    /// Used capacity in bytes
    pub used: u64,
    /// Number of objects stored
    pub object_count: u64,
}

impl StorageTier {
    pub fn new(class: TierClass, name: &str, capacity: u64) -> Self {
        StorageTier {
            class,
            name: String::from(name),
            capacity,
            used: 0,
            object_count: 0,
        }
    }

    /// Free space in bytes.
    pub fn free(&self) -> u64 {
        self.capacity.saturating_sub(self.used)
    }

    /// Usage ratio (0.0 – 1.0).
    pub fn usage_ratio(&self) -> f32 {
        if self.capacity == 0 { return 0.0; }
        self.used as f32 / self.capacity as f32
    }
}

// ─── Object Heat Tracking ───────────────────────────────────────────────────

/// Heat information for one object.
#[derive(Debug, Clone)]
pub struct ObjectHeat {
    /// Prism object ID
    pub oid: u64,
    /// Object size in bytes
    pub size: u64,
    /// Which tier the object is currently on
    pub current_tier: TierClass,
    /// Total accesses (reads + writes)
    pub access_count: u64,
    /// Accesses in the current window
    pub window_accesses: u64,
    /// Last access timestamp (ns)
    pub last_access: u64,
    /// Computed heat score (higher = hotter)
    pub heat_score: f32,
    /// Is a migration currently in progress?
    pub migrating: bool,
}

// ─── Tiering Policy ─────────────────────────────────────────────────────────

/// Configuration for the tiering engine.
#[derive(Debug, Clone)]
pub struct TieringPolicy {
    /// Heat score above which an object should be promoted
    pub promote_threshold: f32,
    /// Heat score below which an object should be demoted
    pub demote_threshold: f32,
    /// Minimum time between migrations for the same object (ns)
    pub migration_cooldown_ns: u64,
    /// Maximum concurrent migrations
    pub max_concurrent_migrations: usize,
    /// Window size for access counting (ns)
    pub window_size_ns: u64,
    /// Weight for recency in heat calculation (0.0 – 1.0)
    pub recency_weight: f32,
    /// Weight for frequency in heat calculation
    pub frequency_weight: f32,
    /// Minimum object size to consider for tiering (avoid churn)
    pub min_object_size: u64,
}

impl Default for TieringPolicy {
    fn default() -> Self {
        TieringPolicy {
            promote_threshold: 80.0,
            demote_threshold: 20.0,
            migration_cooldown_ns: 300_000_000_000, // 5 minutes
            max_concurrent_migrations: 4,
            window_size_ns: 3_600_000_000_000, // 1 hour
            recency_weight: 0.4,
            frequency_weight: 0.6,
            min_object_size: 4096,
        }
    }
}

/// A pending or active migration.
#[derive(Debug, Clone)]
pub struct Migration {
    /// Migration ID
    pub id: u64,
    /// Object being migrated
    pub oid: u64,
    /// Source tier
    pub from: TierClass,
    /// Destination tier
    pub to: TierClass,
    /// Object size
    pub size: u64,
    /// Migration state
    pub state: MigrationState,
    /// Started at (ns)
    pub started_at: u64,
    /// Bytes copied so far
    pub bytes_copied: u64,
}

/// Migration state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    /// Queued, waiting to start
    Queued,
    /// Copying data
    Copying,
    /// Verifying integrity
    Verifying,
    /// Updating metadata (atomic pointer swing)
    Committing,
    /// Done
    Complete,
    /// Failed
    Failed,
}

// ─── Tiering Engine ─────────────────────────────────────────────────────────

/// Tiering statistics.
#[derive(Debug, Clone, Default)]
pub struct TieringStats {
    pub promotions: u64,
    pub demotions: u64,
    pub bytes_promoted: u64,
    pub bytes_demoted: u64,
    pub migrations_failed: u64,
    pub heat_scans: u64,
}

/// The Storage Tiering Engine.
pub struct TieringEngine {
    /// Available storage tiers
    pub tiers: Vec<StorageTier>,
    /// Object heat map: OID → heat info
    pub heat_map: BTreeMap<u64, ObjectHeat>,
    /// Active/queued migrations
    pub migrations: Vec<Migration>,
    /// Policy configuration
    pub policy: TieringPolicy,
    /// Next migration ID
    next_migration_id: u64,
    /// Statistics
    pub stats: TieringStats,
}

impl TieringEngine {
    pub fn new(policy: TieringPolicy) -> Self {
        TieringEngine {
            tiers: Vec::new(),
            heat_map: BTreeMap::new(),
            migrations: Vec::new(),
            policy,
            next_migration_id: 1,
            stats: TieringStats::default(),
        }
    }

    /// Add a storage tier.
    pub fn add_tier(&mut self, tier: StorageTier) {
        self.tiers.push(tier);
        // Keep sorted Hot → Archive
        self.tiers.sort_by_key(|t| t.class);
    }

    /// Record an access to an object.
    pub fn record_access(&mut self, oid: u64, size: u64, tier: TierClass, now: u64) {
        let entry = self.heat_map.entry(oid).or_insert_with(|| ObjectHeat {
            oid,
            size,
            current_tier: tier,
            access_count: 0,
            window_accesses: 0,
            last_access: now,
            heat_score: 50.0,
            migrating: false,
        });

        entry.access_count += 1;
        entry.window_accesses += 1;
        entry.last_access = now;
    }

    /// Compute heat scores for all tracked objects.
    pub fn compute_heat_scores(&mut self, now: u64) {
        self.stats.heat_scans += 1;

        for heat in self.heat_map.values_mut() {
            // Recency component: exponential decay
            let age_ns = now.saturating_sub(heat.last_access);
            let age_hours = age_ns as f64 / 3_600_000_000_000.0;
            let recency = (100.0 * (-0.1 * age_hours).exp()) as f32;

            // Frequency component: log scale of accesses in window
            let freq = if heat.window_accesses == 0 {
                0.0
            } else {
                let log_val = (heat.window_accesses as f32).ln();
                (log_val * 20.0).min(100.0)
            };

            // Weighted combination
            heat.heat_score = self.policy.recency_weight * recency
                            + self.policy.frequency_weight * freq;

            // Clamp to 0–100
            heat.heat_score = heat.heat_score.max(0.0).min(100.0);
        }
    }

    /// Scan heat map and schedule necessary migrations.
    pub fn schedule_migrations(&mut self, now: u64) {
        let active = self.migrations.iter()
            .filter(|m| m.state != MigrationState::Complete && m.state != MigrationState::Failed)
            .count();

        if active >= self.policy.max_concurrent_migrations {
            return;
        }

        // Collect candidates (can't mutate heat_map while iterating)
        let mut promotions: Vec<(u64, u64, TierClass)> = Vec::new();
        let mut demotions: Vec<(u64, u64, TierClass)> = Vec::new();

        for heat in self.heat_map.values() {
            if heat.migrating || heat.size < self.policy.min_object_size {
                continue;
            }

            if heat.heat_score >= self.policy.promote_threshold && heat.current_tier != TierClass::Hot {
                // Promote: move to a hotter tier
                let target = match heat.current_tier {
                    TierClass::Archive => TierClass::Cold,
                    TierClass::Cold    => TierClass::Warm,
                    TierClass::Warm    => TierClass::Hot,
                    TierClass::Hot     => continue,
                };
                promotions.push((heat.oid, heat.size, target));
            } else if heat.heat_score <= self.policy.demote_threshold && heat.current_tier != TierClass::Archive {
                // Demote: move to a colder tier
                let target = match heat.current_tier {
                    TierClass::Hot     => TierClass::Warm,
                    TierClass::Warm    => TierClass::Cold,
                    TierClass::Cold    => TierClass::Archive,
                    TierClass::Archive => continue,
                };
                demotions.push((heat.oid, heat.size, target));
            }
        }

        // Enqueue promotions first (they improve performance)
        for (oid, size, target) in promotions {
            if active + self.migrations.len() >= self.policy.max_concurrent_migrations {
                break;
            }
            self.enqueue_migration(oid, size, target, now, true);
        }

        // Then demotions (free up hot-tier space)
        for (oid, size, target) in demotions {
            if active + self.migrations.len() >= self.policy.max_concurrent_migrations {
                break;
            }
            self.enqueue_migration(oid, size, target, now, false);
        }
    }

    /// Enqueue a migration.
    fn enqueue_migration(
        &mut self,
        oid: u64,
        size: u64,
        target: TierClass,
        now: u64,
        is_promotion: bool,
    ) {
        let current_tier = match self.heat_map.get(&oid) {
            Some(h) => h.current_tier,
            None => return,
        };

        // Check target tier has space
        let has_space = self.tiers.iter()
            .any(|t| t.class == target && t.free() >= size);
        if !has_space { return; }

        let id = self.next_migration_id;
        self.next_migration_id += 1;

        self.migrations.push(Migration {
            id,
            oid,
            from: current_tier,
            to: target,
            size,
            state: MigrationState::Queued,
            started_at: now,
            bytes_copied: 0,
        });

        // Mark object as migrating
        if let Some(heat) = self.heat_map.get_mut(&oid) {
            heat.migrating = true;
        }

        if is_promotion {
            self.stats.promotions += 1;
            self.stats.bytes_promoted += size;
        } else {
            self.stats.demotions += 1;
            self.stats.bytes_demoted += size;
        }
    }

    /// Complete a migration (called after data copy finishes).
    pub fn complete_migration(&mut self, migration_id: u64) {
        if let Some(migration) = self.migrations.iter_mut().find(|m| m.id == migration_id) {
            migration.state = MigrationState::Complete;
            migration.bytes_copied = migration.size;

            let oid = migration.oid;
            let new_tier = migration.to;
            let size = migration.size;
            let old_tier = migration.from;

            // Update heat map
            if let Some(heat) = self.heat_map.get_mut(&oid) {
                heat.current_tier = new_tier;
                heat.migrating = false;
                heat.window_accesses = 0; // Reset window
            }

            // Update tier capacities
            for tier in &mut self.tiers {
                if tier.class == old_tier {
                    tier.used = tier.used.saturating_sub(size);
                    tier.object_count = tier.object_count.saturating_sub(1);
                }
                if tier.class == new_tier {
                    tier.used += size;
                    tier.object_count += 1;
                }
            }
        }
    }

    /// Fail a migration.
    pub fn fail_migration(&mut self, migration_id: u64) {
        if let Some(migration) = self.migrations.iter_mut().find(|m| m.id == migration_id) {
            migration.state = MigrationState::Failed;
            self.stats.migrations_failed += 1;

            // Unmark as migrating
            if let Some(heat) = self.heat_map.get_mut(&migration.oid) {
                heat.migrating = false;
            }
        }
    }

    /// Get tier summary.
    pub fn tier_summary(&self) -> Vec<(TierClass, u64, u64, f32)> {
        self.tiers.iter().map(|t| {
            (t.class, t.used, t.capacity, t.usage_ratio())
        }).collect()
    }

    /// Reset window access counters (call at end of each window).
    pub fn reset_window(&mut self) {
        for heat in self.heat_map.values_mut() {
            heat.window_accesses = 0;
        }
    }

    /// Purge completed/failed migrations older than `max_age_ns`.
    pub fn purge_old_migrations(&mut self, now: u64, max_age_ns: u64) {
        self.migrations.retain(|m| {
            if m.state == MigrationState::Complete || m.state == MigrationState::Failed {
                now.saturating_sub(m.started_at) < max_age_ns
            } else {
                true
            }
        });
    }
}
