//! # Qernel Performance Monitoring Counters (PMC)
//!
//! Reads CPU hardware performance counters to detect side-channel
//! attacks (Spectre, Meltdown, cache timing) in real time (Section 7).
//!
//! The Sentinel uses PMC data to catch:
//! - Abnormal cache miss rates (cache side-channel)
//! - Branch misprediction spikes (Spectre-class)
//! - Excessive TLB flushes (Meltdown-class)
//! - Unusual instruction retirement patterns
//!
//! PMCs run on a dedicated Sentinel core and never share data
//! with user-mode code.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// PMC counter type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CounterType {
    /// CPU cycles elapsed
    Cycles,
    /// Instructions retired
    InstructionsRetired,
    /// L1 data cache misses
    L1DCacheMiss,
    /// L2 cache misses
    L2CacheMiss,
    /// Last-level cache misses
    LLCMiss,
    /// Branch mispredictions
    BranchMispredict,
    /// TLB misses
    TlbMiss,
    /// Context switches
    ContextSwitches,
    /// Page faults
    PageFaults,
}

/// A PMC reading for a specific Silo.
#[derive(Debug, Clone)]
pub struct PmcReading {
    /// Silo ID
    pub silo_id: u64,
    /// Counter values
    pub counters: BTreeMap<CounterType, u64>,
    /// Timestamp
    pub timestamp: u64,
    /// CPU core this was sampled from
    pub core_id: u32,
}

/// Anomaly detection thresholds.
#[derive(Debug, Clone)]
pub struct AnomalyThresholds {
    /// Max L1D cache miss rate per 1000 instructions
    pub l1d_miss_per_1k: f32,
    /// Max LLC miss rate per 1000 instructions
    pub llc_miss_per_1k: f32,
    /// Max branch misprediction rate per 1000 instructions
    pub branch_mispredict_per_1k: f32,
    /// Max TLB miss rate per 1000 instructions
    pub tlb_miss_per_1k: f32,
}

impl Default for AnomalyThresholds {
    fn default() -> Self {
        AnomalyThresholds {
            l1d_miss_per_1k: 50.0,
            llc_miss_per_1k: 10.0,
            branch_mispredict_per_1k: 25.0,
            tlb_miss_per_1k: 15.0,
        }
    }
}

/// A detected PMC anomaly.
#[derive(Debug, Clone)]
pub struct PmcAnomaly {
    /// Anomaly ID
    pub id: u64,
    /// Silo that triggered it
    pub silo_id: u64,
    /// Which counter was anomalous
    pub counter: CounterType,
    /// Observed rate (per 1000 instructions)
    pub observed_rate: f32,
    /// Threshold that was exceeded
    pub threshold: f32,
    /// Suspected attack type
    pub suspected: AttackType,
    /// Timestamp
    pub timestamp: u64,
}

/// Suspected attack type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttackType {
    /// Cache timing side-channel (Flush+Reload, Prime+Probe)
    CacheSideChannel,
    /// Spectre-class (branch prediction exploitation)
    SpectreClass,
    /// Meltdown-class (privilege boundary bypass)
    MeltdownClass,
    /// Row-hammer (DRAM bit-flip)
    RowHammer,
    /// Unknown pattern
    Unknown,
}

/// PMC baseline for a Silo (normal behavior).
#[derive(Debug, Clone)]
pub struct PmcBaseline {
    pub silo_id: u64,
    pub avg_l1d_miss_per_1k: f32,
    pub avg_llc_miss_per_1k: f32,
    pub avg_branch_miss_per_1k: f32,
    pub avg_tlb_miss_per_1k: f32,
    pub samples: u64,
}

/// PMC statistics.
#[derive(Debug, Clone, Default)]
pub struct PmcStats {
    pub readings_taken: u64,
    pub anomalies_detected: u64,
    pub cache_attacks_caught: u64,
    pub spectre_attacks_caught: u64,
    pub meltdown_attacks_caught: u64,
    pub baselines_updated: u64,
}

/// The PMC Monitor.
pub struct PmcMonitor {
    /// Per-Silo baselines
    pub baselines: BTreeMap<u64, PmcBaseline>,
    /// Anomaly thresholds
    pub thresholds: AnomalyThresholds,
    /// Detected anomalies
    pub anomalies: Vec<PmcAnomaly>,
    /// Next anomaly ID
    next_anomaly_id: u64,
    /// Statistics
    pub stats: PmcStats,
}

impl PmcMonitor {
    pub fn new() -> Self {
        PmcMonitor {
            baselines: BTreeMap::new(),
            thresholds: AnomalyThresholds::default(),
            anomalies: Vec::new(),
            next_anomaly_id: 1,
            stats: PmcStats::default(),
        }
    }

    /// Process a PMC reading and check for anomalies.
    pub fn process(&mut self, reading: &PmcReading) -> Vec<PmcAnomaly> {
        self.stats.readings_taken += 1;
        let mut found = Vec::new();

        let instructions = reading.counters.get(&CounterType::InstructionsRetired)
            .copied().unwrap_or(1).max(1) as f32;
        let scale = 1000.0 / instructions;

        // Check L1D cache miss rate
        if let Some(&l1d) = reading.counters.get(&CounterType::L1DCacheMiss) {
            let rate = l1d as f32 * scale;
            if rate > self.thresholds.l1d_miss_per_1k {
                found.push(self.create_anomaly(
                    reading.silo_id, CounterType::L1DCacheMiss, rate,
                    self.thresholds.l1d_miss_per_1k, AttackType::CacheSideChannel,
                    reading.timestamp,
                ));
            }
        }

        // Check LLC miss rate
        if let Some(&llc) = reading.counters.get(&CounterType::LLCMiss) {
            let rate = llc as f32 * scale;
            if rate > self.thresholds.llc_miss_per_1k {
                found.push(self.create_anomaly(
                    reading.silo_id, CounterType::LLCMiss, rate,
                    self.thresholds.llc_miss_per_1k, AttackType::CacheSideChannel,
                    reading.timestamp,
                ));
            }
        }

        // Check branch misprediction rate (Spectre)
        if let Some(&br) = reading.counters.get(&CounterType::BranchMispredict) {
            let rate = br as f32 * scale;
            if rate > self.thresholds.branch_mispredict_per_1k {
                found.push(self.create_anomaly(
                    reading.silo_id, CounterType::BranchMispredict, rate,
                    self.thresholds.branch_mispredict_per_1k, AttackType::SpectreClass,
                    reading.timestamp,
                ));
            }
        }

        // Check TLB miss rate (Meltdown)
        if let Some(&tlb) = reading.counters.get(&CounterType::TlbMiss) {
            let rate = tlb as f32 * scale;
            if rate > self.thresholds.tlb_miss_per_1k {
                found.push(self.create_anomaly(
                    reading.silo_id, CounterType::TlbMiss, rate,
                    self.thresholds.tlb_miss_per_1k, AttackType::MeltdownClass,
                    reading.timestamp,
                ));
            }
        }

        // Update baseline (EMA)
        self.update_baseline(reading, &[
            (CounterType::L1DCacheMiss, scale),
            (CounterType::LLCMiss, scale),
            (CounterType::BranchMispredict, scale),
            (CounterType::TlbMiss, scale),
        ]);

        found
    }

    /// Create an anomaly record.
    fn create_anomaly(
        &mut self, silo_id: u64, counter: CounterType, rate: f32,
        threshold: f32, suspected: AttackType, timestamp: u64,
    ) -> PmcAnomaly {
        let id = self.next_anomaly_id;
        self.next_anomaly_id += 1;
        self.stats.anomalies_detected += 1;

        match suspected {
            AttackType::CacheSideChannel => self.stats.cache_attacks_caught += 1,
            AttackType::SpectreClass => self.stats.spectre_attacks_caught += 1,
            AttackType::MeltdownClass => self.stats.meltdown_attacks_caught += 1,
            _ => {}
        }

        let anomaly = PmcAnomaly { id, silo_id, counter, observed_rate: rate, threshold, suspected, timestamp };
        self.anomalies.push(anomaly.clone());
        anomaly
    }

    /// Update behavioral baseline with EMA.
    fn update_baseline(&mut self, reading: &PmcReading, rates: &[(CounterType, f32)]) {
        let baseline = self.baselines.entry(reading.silo_id)
            .or_insert_with(|| PmcBaseline {
                silo_id: reading.silo_id,
                avg_l1d_miss_per_1k: 0.0,
                avg_llc_miss_per_1k: 0.0,
                avg_branch_miss_per_1k: 0.0,
                avg_tlb_miss_per_1k: 0.0,
                samples: 0,
            });

        let alpha = 0.05f32;
        let scale = rates.get(0).map(|r| r.1).unwrap_or(1.0);

        for (ct, _) in rates {
            if let Some(&val) = reading.counters.get(ct) {
                let rate = val as f32 * scale;
                match ct {
                    CounterType::L1DCacheMiss => {
                        baseline.avg_l1d_miss_per_1k = baseline.avg_l1d_miss_per_1k * (1.0 - alpha) + rate * alpha;
                    }
                    CounterType::LLCMiss => {
                        baseline.avg_llc_miss_per_1k = baseline.avg_llc_miss_per_1k * (1.0 - alpha) + rate * alpha;
                    }
                    CounterType::BranchMispredict => {
                        baseline.avg_branch_miss_per_1k = baseline.avg_branch_miss_per_1k * (1.0 - alpha) + rate * alpha;
                    }
                    CounterType::TlbMiss => {
                        baseline.avg_tlb_miss_per_1k = baseline.avg_tlb_miss_per_1k * (1.0 - alpha) + rate * alpha;
                    }
                    _ => {}
                }
            }
        }
        baseline.samples += 1;
        self.stats.baselines_updated += 1;
    }
}
