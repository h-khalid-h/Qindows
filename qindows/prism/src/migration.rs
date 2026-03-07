//! # Prism Data Migration Engine
//!
//! Moves objects between storage tiers based on access patterns,
//! age, and policy. Integrates with the Tiering module to provide
//! automatic hot/warm/cold data management:
//!
//! - **Hot tier**: NVMe SSD — frequently accessed, low latency
//! - **Warm tier**: SATA SSD — moderate access, balanced cost
//! - **Cold tier**: HDD / cloud — rarely accessed, archival
//!
//! Migration decisions are driven by the Sentinel's telemetry.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Storage tier identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    /// NVMe — fastest, most expensive
    Hot,
    /// SATA SSD — balanced
    Warm,
    /// HDD or cloud — cheapest, slowest
    Cold,
    /// Archived (compressed, deduplicated, possibly off-site)
    Archive,
}

impl Tier {
    /// Relative latency cost (lower = faster).
    pub fn latency_cost(&self) -> u32 {
        match self {
            Tier::Hot => 1,
            Tier::Warm => 10,
            Tier::Cold => 100,
            Tier::Archive => 1000,
        }
    }

    /// Relative storage cost per GiB (lower = cheaper).
    pub fn storage_cost(&self) -> u32 {
        match self {
            Tier::Hot => 100,
            Tier::Warm => 30,
            Tier::Cold => 5,
            Tier::Archive => 1,
        }
    }
}

/// Object access statistics (used for migration decisions).
#[derive(Debug, Clone)]
pub struct AccessStats {
    /// Object ID
    pub oid: u64,
    /// Current tier
    pub tier: Tier,
    /// Size in bytes
    pub size: u64,
    /// Total read count
    pub reads: u64,
    /// Total write count
    pub writes: u64,
    /// Last access timestamp (ticks)
    pub last_access: u64,
    /// Creation timestamp
    pub created_at: u64,
    /// Is this object pinned to its current tier?
    pub pinned: bool,
}

/// Migration action.
#[derive(Debug, Clone)]
pub struct MigrationTask {
    /// Object ID
    pub oid: u64,
    /// Source tier
    pub from: Tier,
    /// Target tier
    pub to: Tier,
    /// Object size
    pub size: u64,
    /// Priority (higher = more urgent)
    pub priority: u32,
    /// Migration state
    pub state: MigrationState,
}

/// Migration task state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    /// Queued, waiting to execute
    Pending,
    /// Data being copied to target tier
    Copying,
    /// Verifying integrity on target
    Verifying,
    /// Updating metadata pointers
    Switching,
    /// Cleaning up source copy
    Cleanup,
    /// Done
    Complete,
    /// Failed (will retry)
    Failed,
}

/// Migration policy configuration.
#[derive(Debug, Clone)]
pub struct MigrationPolicy {
    /// Promote to Hot if accessed more than N times in the window
    pub hot_access_threshold: u64,
    /// Demote from Hot if not accessed in N ticks
    pub hot_idle_ticks: u64,
    /// Demote from Warm to Cold if not accessed in N ticks
    pub warm_idle_ticks: u64,
    /// Archive if not accessed in N ticks
    pub archive_idle_ticks: u64,
    /// Maximum objects to migrate per cycle
    pub batch_size: usize,
    /// Minimum size to consider for migration (skip tiny objects)
    pub min_size: u64,
    /// Maximum concurrent migrations
    pub max_concurrent: usize,
}

impl Default for MigrationPolicy {
    fn default() -> Self {
        MigrationPolicy {
            hot_access_threshold: 10,
            hot_idle_ticks: 60 * 60 * 1000,     // 1 hour
            warm_idle_ticks: 24 * 60 * 60 * 1000, // 1 day
            archive_idle_ticks: 30 * 24 * 60 * 60 * 1000, // 30 days
            batch_size: 100,
            min_size: 4096,
            max_concurrent: 4,
        }
    }
}

/// Migration statistics.
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    pub total_migrations: u64,
    pub promotions: u64,    // moved to a faster tier
    pub demotions: u64,     // moved to a slower tier
    pub bytes_migrated: u64,
    pub failed_migrations: u64,
    pub active_migrations: u64,
}

/// The Data Migration Engine.
pub struct MigrationEngine {
    /// Object access statistics
    pub objects: BTreeMap<u64, AccessStats>,
    /// Pending migration tasks
    pub queue: Vec<MigrationTask>,
    /// Active migrations
    pub active: Vec<MigrationTask>,
    /// Migration policy
    pub policy: MigrationPolicy,
    /// Statistics
    pub stats: MigrationStats,
    /// Per-tier capacity (bytes)
    pub tier_capacity: BTreeMap<Tier, u64>,
    /// Per-tier usage (bytes)
    pub tier_usage: BTreeMap<Tier, u64>,
}

impl MigrationEngine {
    pub fn new() -> Self {
        let mut capacity = BTreeMap::new();
        capacity.insert(Tier::Hot, 256 * 1024 * 1024 * 1024);   // 256 GiB
        capacity.insert(Tier::Warm, 1024 * 1024 * 1024 * 1024); // 1 TiB
        capacity.insert(Tier::Cold, 4 * 1024 * 1024 * 1024 * 1024); // 4 TiB
        capacity.insert(Tier::Archive, u64::MAX); // Unlimited

        let mut usage = BTreeMap::new();
        usage.insert(Tier::Hot, 0u64);
        usage.insert(Tier::Warm, 0u64);
        usage.insert(Tier::Cold, 0u64);
        usage.insert(Tier::Archive, 0u64);

        MigrationEngine {
            objects: BTreeMap::new(),
            queue: Vec::new(),
            active: Vec::new(),
            policy: MigrationPolicy::default(),
            stats: MigrationStats::default(),
            tier_capacity: capacity,
            tier_usage: usage,
        }
    }

    /// Register an object for tracking.
    pub fn track(&mut self, oid: u64, tier: Tier, size: u64, now: u64) {
        self.objects.insert(oid, AccessStats {
            oid,
            tier,
            size,
            reads: 0,
            writes: 0,
            last_access: now,
            created_at: now,
            pinned: false,
        });
        *self.tier_usage.entry(tier).or_insert(0) += size;
    }

    /// Record an access to an object.
    pub fn record_access(&mut self, oid: u64, is_write: bool, now: u64) {
        if let Some(stats) = self.objects.get_mut(&oid) {
            if is_write {
                stats.writes = stats.writes.saturating_add(1);
            } else {
                stats.reads = stats.reads.saturating_add(1);
            }
            stats.last_access = now;
        }
    }

    /// Pin an object to its current tier (prevent migration).
    pub fn pin(&mut self, oid: u64) {
        if let Some(stats) = self.objects.get_mut(&oid) {
            stats.pinned = true;
        }
    }

    /// Evaluate all objects and generate migration tasks.
    pub fn evaluate(&mut self, now: u64) {
        let policy = self.policy.clone();
        let mut tasks = Vec::new();

        for stats in self.objects.values() {
            if stats.pinned || stats.size < policy.min_size {
                continue;
            }

            let idle_ticks = now.saturating_sub(stats.last_access);
            let total_accesses = stats.reads.saturating_add(stats.writes);

            let target_tier = match stats.tier {
                Tier::Hot => {
                    if idle_ticks > policy.hot_idle_ticks {
                        Some(Tier::Warm) // Demote
                    } else {
                        None
                    }
                }
                Tier::Warm => {
                    if total_accesses >= policy.hot_access_threshold {
                        Some(Tier::Hot) // Promote
                    } else if idle_ticks > policy.warm_idle_ticks {
                        Some(Tier::Cold) // Demote
                    } else {
                        None
                    }
                }
                Tier::Cold => {
                    if total_accesses >= policy.hot_access_threshold {
                        Some(Tier::Warm) // Promote
                    } else if idle_ticks > policy.archive_idle_ticks {
                        Some(Tier::Archive) // Archive
                    } else {
                        None
                    }
                }
                Tier::Archive => {
                    if total_accesses > 0 {
                        Some(Tier::Cold) // Resurrect
                    } else {
                        None
                    }
                }
            };

            if let Some(target) = target_tier {
                let priority = if target < stats.tier { 10 } else { 5 }; // Promotions are higher priority
                tasks.push(MigrationTask {
                    oid: stats.oid,
                    from: stats.tier,
                    to: target,
                    size: stats.size,
                    priority,
                    state: MigrationState::Pending,
                });
            }

            if tasks.len() >= policy.batch_size {
                break;
            }
        }

        // Sort by priority (highest first)
        tasks.sort_by(|a, b| b.priority.cmp(&a.priority));
        self.queue.extend(tasks);
    }

    /// Execute pending migrations (one step at a time).
    pub fn execute_step(&mut self) {
        // Move pending tasks to active (up to max_concurrent)
        while self.active.len() < self.policy.max_concurrent {
            if let Some(mut task) = self.queue.pop() {
                task.state = MigrationState::Copying;
                self.active.push(task);
            } else {
                break;
            }
        }

        // Advance active tasks through their state machine
        let mut completed = Vec::new();
        for task in &mut self.active {
            match task.state {
                MigrationState::Copying => {
                    // Data copy would happen here via DMA
                    task.state = MigrationState::Verifying;
                }
                MigrationState::Verifying => {
                    // Checksum verification
                    task.state = MigrationState::Switching;
                }
                MigrationState::Switching => {
                    // Update B-tree pointers
                    task.state = MigrationState::Cleanup;
                }
                MigrationState::Cleanup => {
                    // Free source blocks
                    task.state = MigrationState::Complete;
                    completed.push(task.oid);
                }
                _ => {}
            }
        }

        // Finalize completed migrations
        for oid in &completed {
            if let Some(pos) = self.active.iter().position(|t| t.oid == *oid) {
                let task = self.active.remove(pos);

                // Update tracking
                if let Some(stats) = self.objects.get_mut(oid) {
                    // Update tier usage
                    if let Some(usage) = self.tier_usage.get_mut(&task.from) {
                        *usage = usage.saturating_sub(task.size);
                    }
                    *self.tier_usage.entry(task.to).or_insert(0) += task.size;

                    stats.tier = task.to;
                    stats.reads = 0;
                    stats.writes = 0;
                }

                // Update stats
                self.stats.total_migrations += 1;
                self.stats.bytes_migrated = self.stats.bytes_migrated.saturating_add(task.size);
                if task.to < task.from {
                    self.stats.promotions += 1;
                } else {
                    self.stats.demotions += 1;
                }
            }
        }

        self.stats.active_migrations = self.active.len() as u64;
    }

    /// Get tier utilization percentages.
    pub fn tier_utilization(&self) -> Vec<(Tier, f64)> {
        self.tier_capacity.iter().map(|(tier, cap)| {
            let used = self.tier_usage.get(tier).copied().unwrap_or(0);
            let pct = if *cap == 0 || *cap == u64::MAX {
                0.0
            } else {
                used as f64 / *cap as f64 * 100.0
            };
            (*tier, pct)
        }).collect()
    }
}
