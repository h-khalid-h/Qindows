//! # Sentinel Anomaly Scorer — AI Threat Detection (Phase 90)
//!
//! ARCHITECTURE.md §8 — Sentinel: AI Threat Detection:
//! > "Sentinel uses behavioral anomaly detection to catch zero-days"
//! > "PMC (Performance Monitor Counter) baselines per binary"
//! > "Deviation > threshold → escalating enforcement"
//!
//! ## Architecture Guardian: What was missing
//! `sentinel.rs` (Phase 19/38) enforces law violations reactively (cap revocation etc.)
//! `digital_antibody.rs` (Phase 76) handles known threat signatures.
//! `black_box.rs` (Phase 84) records execution traces.
//!
//! What was missing: the **anomaly scoring engine** — looking at normal PMC baselines
//! and scoring live Silos against them. A Silo that has never violated a law can still
//! be exhibiting suspicious behaviour (privilege escalation setup, timing side-channel).
//!
//! This module:
//! 1. Learns a baseline per binary (first N ticks after spawn = learning phase)
//! 2. Computes a live anomaly score using six PMC dimensions:
//!    - IPC (instructions per cycle) — unusually low = stalling on I/O or sleeping to
//!      evade detection
//!    - Cache miss rate — unusually high = scanning large memory (exfil scan)
//!    - Branch misprediction rate — very high = speculative execution gadget abuse
//!    - Syscall rate — very high = DoS / rapid resource claiming
//!    - IPC variation — unstable IPC = timing side-channel (Spectre-like)
//!    - Net bytes/tick — covert network channel
//! 3. Anomaly score 0-100 is fed to `q_manifest_enforcer.rs` as `SentinelAnomalyScore`
//!
//! ## Relationship to other modules
//! - `qtraffic.rs` (Phase 69) provides per-Silo net byte counts
//! - `black_box.rs` (Phase 84) provides syscall log data
//! - `q_manifest_enforcer.rs` (Phase 80) receives the score and applies enforcement

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ── PMC Sample ────────────────────────────────────────────────────────────────

/// A single PMC snapshot from one Silo at one tick.
#[derive(Debug, Clone, Copy, Default)]
pub struct PmcSample {
    /// Instructions retired in this sample window
    pub instructions_retired: u64,
    /// CPU cycles elapsed in this window
    pub cycles: u64,
    /// L3 cache misses
    pub cache_misses: u64,
    /// Total memory access attempts
    pub cache_accesses: u64,
    /// Branch mispredictions
    pub branch_mispredicts: u64,
    /// Total branches
    pub branches: u64,
    /// Syscalls issued
    pub syscall_count: u64,
    /// Network bytes sent (from qtraffic)
    pub net_bytes_sent: u64,
    /// Sample duration in ticks
    pub tick_duration: u64,
}

impl PmcSample {
    /// IPC (instructions per cycle). 0.0 if cycles = 0.
    pub fn ipc(&self) -> f32 {
        if self.cycles == 0 { return 0.0; }
        self.instructions_retired as f32 / self.cycles as f32
    }

    /// L3 cache miss rate (0.0-1.0).
    pub fn cache_miss_rate(&self) -> f32 {
        if self.cache_accesses == 0 { return 0.0; }
        self.cache_misses as f32 / self.cache_accesses as f32
    }

    /// Branch misprediction rate (0.0-1.0).
    pub fn branch_mispredict_rate(&self) -> f32 {
        if self.branches == 0 { return 0.0; }
        self.branch_mispredicts as f32 / self.branches as f32
    }

    /// Syscalls per tick.
    pub fn syscall_rate(&self) -> f32 {
        if self.tick_duration == 0 { return 0.0; }
        self.syscall_count as f32 / self.tick_duration as f32
    }

    /// Net bytes per tick.
    pub fn net_rate(&self) -> f32 {
        if self.tick_duration == 0 { return 0.0; }
        self.net_bytes_sent as f32 / self.tick_duration as f32
    }
}

// ── Baseline Profile ──────────────────────────────────────────────────────────

/// Learned behavioural baseline for a specific binary OID.
#[derive(Debug, Clone, Default)]
pub struct BinaryBaseline {
    /// Binary OID this baseline belongs to
    pub binary_oid: [u8; 32],
    /// Number of samples contributing to this baseline
    pub sample_count: u32,
    /// Whether we're still in the learning phase
    pub learning: bool,
    /// Baseline IPC (mean)
    pub base_ipc: f32,
    /// Baseline cache miss rate
    pub base_cache_miss: f32,
    /// Baseline branch mispredict rate
    pub base_branch_miss: f32,
    /// Baseline syscall rate
    pub base_syscall_rate: f32,
    /// Baseline net rate
    pub base_net_rate: f32,
    /// Standard deviation of IPC (for detecting IPC variation attacks)
    pub ipc_stddev: f32,
}

impl BinaryBaseline {
    /// Incorporate one more sample into the running mean (online averaging).
    pub fn update(&mut self, sample: &PmcSample) {
        let n = (self.sample_count + 1) as f32;
        let update = |prev: f32, new: f32| -> f32 { prev + (new - prev) / n };
        self.base_ipc          = update(self.base_ipc, sample.ipc());
        self.base_cache_miss   = update(self.base_cache_miss, sample.cache_miss_rate());
        self.base_branch_miss  = update(self.base_branch_miss, sample.branch_mispredict_rate());
        self.base_syscall_rate = update(self.base_syscall_rate, sample.syscall_rate());
        self.base_net_rate     = update(self.base_net_rate, sample.net_rate());
        // Track IPC variation (running variance approx)
        let ipc_diff = sample.ipc() - self.base_ipc;
        // Store running variance (squared std-dev) — no sqrt needed in no_std
        let ipc_variance = update(self.ipc_stddev * self.ipc_stddev, ipc_diff * ipc_diff);
        self.ipc_stddev = ipc_variance; // now stores variance, not std-dev

        self.sample_count += 1;
        // Exit learning phase after 200 samples
        if self.sample_count >= 200 { self.learning = false; }
    }
}

// ── Anomaly Dimension ─────────────────────────────────────────────────────────

/// Which PMC dimension is contributing most to the anomaly score.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyDimension {
    Ipc,           // IPC dramatically dropped (evasion stall)
    CacheMiss,     // Cache miss rate spiked (memory scan)
    BranchMiss,    // Branch misprediction spike (Spectre gadget)
    SyscallRate,   // Syscall rate spiked (DoS / resource grab)
    IpcVariation,  // IPC unstable (timing side channel)
    NetRate,       // Network bytes spike (covert channel / exfil)
    Composite,     // Multiple dimensions contributing
    None,
}

// ── Anomaly Score ─────────────────────────────────────────────────────────────

/// Result of a single anomaly scoring for one Silo.
#[derive(Debug, Clone)]
pub struct AnomalyScore {
    pub silo_id: u64,
    pub binary_oid: [u8; 32],
    pub tick: u64,
    /// Overall score 0-100 (0 = completely normal, 100 = definitely malicious)
    pub score: u8,
    /// Which dimension contributed most
    pub primary_dimension: AnomalyDimension,
    /// Per-dimension scores (0-100 each)
    pub dim_ipc: u8,
    pub dim_cache: u8,
    pub dim_branch: u8,
    pub dim_syscall: u8,
    pub dim_ipc_var: u8,
    pub dim_net: u8,
    /// True if this score exceeds the alert threshold
    pub alert: bool,
}

// ── Per-Silo Scorer State ─────────────────────────────────────────────────────

struct SiloScorer {
    silo_id: u64,
    binary_oid: [u8; 32],
    /// Recent samples (sliding window of 10)
    samples: Vec<PmcSample>,
    max_samples: usize,
    /// Running anomaly score (smoothed)
    smoothed_score: f32,
    /// Consecutive alerts (for escalation)
    alert_streak: u32,
}

impl SiloScorer {
    fn new(silo_id: u64, binary_oid: [u8; 32]) -> Self {
        SiloScorer {
            silo_id,
            binary_oid,
            samples: Vec::new(),
            max_samples: 10,
            smoothed_score: 0.0,
            alert_streak: 0,
        }
    }

    fn push_sample(&mut self, sample: PmcSample) {
        if self.samples.len() >= self.max_samples { self.samples.remove(0); }
        self.samples.push(sample);
    }
}

// ── Sentinel Anomaly Scorer Statistics ────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AnomalyScorerStats {
    pub silos_monitored: u64,
    pub samples_processed: u64,
    pub alerts_generated: u64,
    pub baselines_learned: u64,
    pub alert_escalations: u64,  // alert_streak > 5: report to q_manifest_enforcer
}

// ── Sentinel Anomaly Scorer ───────────────────────────────────────────────────

/// Sentinel AI anomaly detection and scoring engine.
pub struct SentinelAnomalyScorer {
    /// Per-Silo scorers: silo_id → scorer
    scorers: BTreeMap<u64, SiloScorer>,
    /// Per-binary learned baselines: binary_oid_key → baseline
    baselines: BTreeMap<u64, BinaryBaseline>,
    /// Alert threshold (default 65 — below 65 = warn, 65-85 = throttle, >85 = quarantine)
    pub alert_threshold: u8,
    /// Quarantine threshold (auto-report to enforcer)
    pub quarantine_threshold: u8,
    /// Statistics
    pub stats: AnomalyScorerStats,
    /// Recent alert history (last 64)
    pub recent_alerts: Vec<AnomalyScore>,
}

impl SentinelAnomalyScorer {
    pub fn new() -> Self {
        SentinelAnomalyScorer {
            scorers: BTreeMap::new(),
            baselines: BTreeMap::new(),
            alert_threshold: 65,
            quarantine_threshold: 85,
            stats: AnomalyScorerStats::default(),
            recent_alerts: Vec::new(),
        }
    }

    fn oid_key(oid: &[u8; 32]) -> u64 {
        u64::from_le_bytes([oid[0], oid[1], oid[2], oid[3], oid[4], oid[5], oid[6], oid[7]])
    }

    /// Register a new Silo for monitoring (called from SiloEventBus on Spawned event).
    pub fn register_silo(&mut self, silo_id: u64, binary_oid: [u8; 32]) {
        self.scorers.insert(silo_id, SiloScorer::new(silo_id, binary_oid));
        self.stats.silos_monitored += 1;
        // Ensure baseline entry exists (may have been learned from previous runs)
        let key = Self::oid_key(&binary_oid);
        self.baselines.entry(key).or_insert_with(|| {
            let mut b = BinaryBaseline::default();
            b.binary_oid = binary_oid;
            b.learning = true;
            self.stats.baselines_learned += 1;
            b
        });
    }

    /// Deregister on Silo vaporization.
    pub fn unregister_silo(&mut self, silo_id: u64) {
        self.scorers.remove(&silo_id);
    }

    /// Feed a PMC sample and return the anomaly score.
    /// Returns None if the scorer doesn't know about this Silo.
    pub fn score(&mut self, silo_id: u64, sample: PmcSample, tick: u64) -> Option<AnomalyScore> {
        let binary_oid = self.scorers.get(&silo_id)?.binary_oid;
        let key = Self::oid_key(&binary_oid);

        // Update baseline (always — learning never fully stops after phase)
        if let Some(baseline) = self.baselines.get_mut(&key) {
            baseline.update(&sample);
        }

        let baseline = self.baselines.get(&key)?;

        // Still in learning phase — don't score yet
        if baseline.learning {
            if let Some(scorer) = self.scorers.get_mut(&silo_id) {
                scorer.push_sample(sample);
            }
            self.stats.samples_processed += 1;
            return None;
        }

        // Score each dimension: deviation from baseline (in multiples of baseline value)
        let dim_ipc = Self::score_deviation(sample.ipc(), baseline.base_ipc, true);
        let dim_cache = Self::score_deviation(sample.cache_miss_rate(), baseline.base_cache_miss, false);
        let dim_branch = Self::score_deviation(sample.branch_mispredict_rate(), baseline.base_branch_miss, false);
        let dim_syscall = Self::score_deviation(sample.syscall_rate(), baseline.base_syscall_rate, false);
        let dim_ipc_var = if baseline.ipc_stddev > 0.0 {
            let deviation = (sample.ipc() - baseline.base_ipc).abs() / (baseline.ipc_stddev + 0.01);
            (deviation * 20.0).min(100.0) as u8
        } else { 0u8 };
        let dim_net = Self::score_deviation(sample.net_rate(), baseline.base_net_rate, false);

        // Composite score: max + weighted sum
        let composite = ((dim_ipc as u32
            + dim_cache as u32 * 2
            + dim_branch as u32 * 2
            + dim_syscall as u32
            + dim_ipc_var as u32 * 3
            + dim_net as u32 * 2) / 11).min(100) as u8;

        // Identify primary dimension
        let dims = [
            (dim_ipc,    AnomalyDimension::Ipc),
            (dim_cache,  AnomalyDimension::CacheMiss),
            (dim_branch, AnomalyDimension::BranchMiss),
            (dim_syscall,AnomalyDimension::SyscallRate),
            (dim_ipc_var,AnomalyDimension::IpcVariation),
            (dim_net,    AnomalyDimension::NetRate),
        ];
        let primary = dims.iter().max_by_key(|(s, _)| *s)
            .map(|(_, d)| *d)
            .unwrap_or(AnomalyDimension::None);

        let alert = composite >= self.alert_threshold;
        if alert { self.stats.alerts_generated += 1; }

        let score = AnomalyScore {
            silo_id,
            binary_oid,
            tick,
            score: composite,
            primary_dimension: primary,
            dim_ipc,
            dim_cache,
            dim_branch,
            dim_syscall,
            dim_ipc_var,
            dim_net,
            alert,
        };

        if alert {
            crate::serial_println!(
                "[SENTINEL ANOMALY] Silo {} score={} primary={:?} (IPC:{} Cache:{} Branch:{} Sys:{} Var:{} Net:{})",
                silo_id, composite, primary,
                dim_ipc, dim_cache, dim_branch, dim_syscall, dim_ipc_var, dim_net
            );
            if self.recent_alerts.len() >= 64 { self.recent_alerts.remove(0); }
            self.recent_alerts.push(score.clone());

            // Track alert streak
            if let Some(scorer) = self.scorers.get_mut(&silo_id) {
                scorer.alert_streak += 1;
                if scorer.alert_streak > 5 {
                    self.stats.alert_escalations += 1;
                    crate::serial_println!(
                        "[SENTINEL ANOMALY] ⚠ ESCALATION: Silo {} alert_streak={}. Reporting to q_manifest_enforcer.",
                        silo_id, scorer.alert_streak
                    );
                }
            }
        } else if let Some(scorer) = self.scorers.get_mut(&silo_id) {
            scorer.alert_streak = 0;
        }

        if let Some(scorer) = self.scorers.get_mut(&silo_id) {
            scorer.push_sample(sample);
            scorer.smoothed_score = scorer.smoothed_score * 0.9 + composite as f32 * 0.1;
        }
        self.stats.samples_processed += 1;

        Some(score)
    }

    /// Score a dimension: how far is `live` from `baseline`?
    /// If `lower_is_worse` = true (e.g. IPC), penalise drops. Otherwise penalise spikes.
    fn score_deviation(live: f32, baseline: f32, lower_is_worse: bool) -> u8 {
        if baseline < 0.001 { return if live > 0.1 { 30 } else { 0 }; }
        let ratio = live / baseline;
        let deviation = if lower_is_worse {
            // penalise drops below baseline
            if ratio < 1.0 { (1.0 - ratio) * 150.0 } else { 0.0 }
        } else {
            // penalise spikes above baseline
            if ratio > 1.0 { (ratio - 1.0) * 50.0 } else { 0.0 }
        };
        deviation.min(100.0) as u8
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║  Sentinel Anomaly Scorer (§8)        ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Silos monitored:  {:>6}             ║", self.stats.silos_monitored);
        crate::serial_println!("║ Samples processed:{:>6}K            ║", self.stats.samples_processed / 1000);
        crate::serial_println!("║ Alerts generated: {:>6}             ║", self.stats.alerts_generated);
        crate::serial_println!("║ Escalations:      {:>6}             ║", self.stats.alert_escalations);
        crate::serial_println!("║ Baselines learned:{:>6}             ║", self.stats.baselines_learned);
        crate::serial_println!("║ Alert threshold:  {:>6}%            ║", self.alert_threshold);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
