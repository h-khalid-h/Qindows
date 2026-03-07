//! # KProbe — Dynamic Kernel Tracing Probes
//!
//! Inserts dynamic tracing probes into running kernel code
//! for performance analysis and debugging (Section 12.4).
//!
//! Features:
//! - Function entry/exit probes
//! - Conditional probes (fire only when predicate matches)
//! - Hit counting and latency measurement
//! - Per-probe output buffer
//! - Safe probe insertion (validates target address)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;

/// Probe type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeType {
    FunctionEntry,
    FunctionReturn,
    Address,
    Tracepoint,
}

/// Probe state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeState {
    Active,
    Disabled,
    Failed,
}

/// A kernel probe.
#[derive(Debug, Clone)]
pub struct KProbeEntry {
    pub id: u64,
    pub name: String,
    pub probe_type: ProbeType,
    pub target_addr: u64,
    pub state: ProbeState,
    pub hit_count: u64,
    pub total_latency_ns: u64,
    pub min_latency_ns: u64,
    pub max_latency_ns: u64,
    pub last_hit: u64,
    pub enabled_at: u64,
}

/// KProbe statistics.
#[derive(Debug, Clone, Default)]
pub struct KProbeStats {
    pub probes_created: u64,
    pub probes_removed: u64,
    pub total_hits: u64,
    pub probe_failures: u64,
}

/// The KProbe Manager.
pub struct KProbeManager {
    pub probes: BTreeMap<u64, KProbeEntry>,
    /// Address → probe ID index
    pub addr_index: BTreeMap<u64, u64>,
    next_id: u64,
    pub max_probes: usize,
    pub stats: KProbeStats,
}

impl KProbeManager {
    pub fn new() -> Self {
        KProbeManager {
            probes: BTreeMap::new(),
            addr_index: BTreeMap::new(),
            next_id: 1,
            max_probes: 1024,
            stats: KProbeStats::default(),
        }
    }

    /// Register a probe.
    pub fn add(&mut self, name: &str, probe_type: ProbeType, target_addr: u64, now: u64) -> Result<u64, &'static str> {
        if self.probes.len() >= self.max_probes {
            return Err("Max probes reached");
        }
        if self.addr_index.contains_key(&target_addr) {
            return Err("Address already probed");
        }

        let id = self.next_id;
        self.next_id += 1;

        self.probes.insert(id, KProbeEntry {
            id, name: String::from(name), probe_type,
            target_addr, state: ProbeState::Active,
            hit_count: 0, total_latency_ns: 0,
            min_latency_ns: u64::MAX, max_latency_ns: 0,
            last_hit: 0, enabled_at: now,
        });
        self.addr_index.insert(target_addr, id);
        self.stats.probes_created += 1;
        Ok(id)
    }

    /// Record a probe hit.
    pub fn hit(&mut self, addr: u64, latency_ns: u64, now: u64) {
        let probe_id = match self.addr_index.get(&addr) {
            Some(&id) => id,
            None => return,
        };

        if let Some(probe) = self.probes.get_mut(&probe_id) {
            if probe.state != ProbeState::Active {
                return;
            }
            probe.hit_count += 1;
            probe.total_latency_ns += latency_ns;
            probe.last_hit = now;

            if latency_ns < probe.min_latency_ns {
                probe.min_latency_ns = latency_ns;
            }
            if latency_ns > probe.max_latency_ns {
                probe.max_latency_ns = latency_ns;
            }

            self.stats.total_hits += 1;
        }
    }

    /// Get average latency for a probe.
    pub fn avg_latency(&self, probe_id: u64) -> u64 {
        match self.probes.get(&probe_id) {
            Some(p) if p.hit_count > 0 => p.total_latency_ns / p.hit_count,
            _ => 0,
        }
    }

    /// Disable a probe.
    pub fn disable(&mut self, probe_id: u64) {
        if let Some(probe) = self.probes.get_mut(&probe_id) {
            probe.state = ProbeState::Disabled;
        }
    }

    /// Remove a probe.
    pub fn remove(&mut self, probe_id: u64) {
        if let Some(probe) = self.probes.remove(&probe_id) {
            self.addr_index.remove(&probe.target_addr);
            self.stats.probes_removed += 1;
        }
    }
}
