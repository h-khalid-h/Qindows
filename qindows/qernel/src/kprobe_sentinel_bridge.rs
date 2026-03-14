//! # KProbe Sentinel Bridge (Phase 154)
//!
//! ## Architecture Guardian: The Gap
//! `kprobe.rs` implements `KProbeManager`:
//! - `add(name, probe_type, target_addr, now)` → Result<u64, &str>
//! - `hit(addr, latency_ns, now)` — records a probe hit
//! - `avg_latency(probe_id)` → u64
//!
//! `q_metrics_bridge.rs` implements `QMetricsBridge`:
//! - `on_syscall(_nr, latency_ticks, tick)`
//! - `on_qring_submit(latency_ticks, tick)`
//! - `on_context_switch(duration_ticks, tick)`
//!
//! **Missing link**: KProbes were never registered at boot or fed from
//! real hotpaths. Probe latency is also a natural input for QMetrics.
//!
//! This module provides `KProbeSentinelBridge` that wires both.

extern crate alloc;

use crate::kprobe::{KProbeManager, ProbeType};
use crate::q_metrics_bridge::QMetricsBridge;

#[derive(Debug, Default, Clone)]
pub struct KProbeBridgeStats {
    pub probes_registered: u64,
    pub hits_recorded:     u64,
    pub reports_exported:  u64,
}

pub struct KProbeSentinelBridge {
    pub probes: KProbeManager,
    pub stats:  KProbeBridgeStats,
    pub silo_dispatch_probe: u64,
    pub syscall_probe:       u64,
    pub qring_probe:         u64,
}

impl KProbeSentinelBridge {
    pub fn new(tick: u64) -> Self {
        let mut probes = KProbeManager::new();

        let silo_dispatch_probe = probes
            .add("silo_dispatch", ProbeType::FunctionEntry, 0xCAFE_0001, tick)
            .unwrap_or(0);
        let syscall_probe = probes
            .add("syscall_dispatch", ProbeType::FunctionEntry, 0xCAFE_0002, tick)
            .unwrap_or(0);
        let qring_probe = probes
            .add("qring_poll", ProbeType::FunctionEntry, 0xCAFE_0003, tick)
            .unwrap_or(0);

        KProbeSentinelBridge {
            probes,
            stats: KProbeBridgeStats { probes_registered: 3, ..Default::default() },
            silo_dispatch_probe,
            syscall_probe,
            qring_probe,
        }
    }

    pub fn on_silo_dispatch(&mut self, latency_ns: u64, now: u64) {
        self.stats.hits_recorded += 1;
        self.probes.hit(0xCAFE_0001, latency_ns, now);
    }

    pub fn on_syscall_dispatch(&mut self, latency_ns: u64, now: u64) {
        self.stats.hits_recorded += 1;
        self.probes.hit(0xCAFE_0002, latency_ns, now);
    }

    pub fn on_qring_poll(&mut self, latency_ns: u64, now: u64) {
        self.stats.hits_recorded += 1;
        self.probes.hit(0xCAFE_0003, latency_ns, now);
    }

    /// Forward probe averages to QMetricsBridge using its real API.
    pub fn report_to_metrics(&mut self, metrics: &mut QMetricsBridge, tick: u64) {
        self.stats.reports_exported += 1;
        let syscall_avg_ticks = self.probes.avg_latency(self.syscall_probe);
        let qring_avg_ticks   = self.probes.avg_latency(self.qring_probe);

        // Feed into QMetricsBridge using its real methods
        metrics.on_syscall(0, syscall_avg_ticks, tick); // syscall_nr=0 (aggregated)
        metrics.on_qring_submit(qring_avg_ticks, tick);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  KProbeBridge: probes={} hits={} reports={} silo_avg={}ns sys_avg={}ns",
            self.stats.probes_registered, self.stats.hits_recorded, self.stats.reports_exported,
            self.probes.avg_latency(self.silo_dispatch_probe),
            self.probes.avg_latency(self.syscall_probe)
        );
    }
}
