//! # NPU Scheduler Synapse Bridge (Phase 165)
//!
//! ## Architecture Guardian: The Gap
//! `npu_sched.rs` implements `NpuScheduler`:
//! - `new(cache_budget)` — creates scheduler with model cache budget
//! - `add_core(id)` — registers an NPU core
//! - `submit(silo_id, model_id, task_type, priority, input_size, now)` → task_id
//! - `schedule(now)` — assigns pending tasks to NPU cores
//! - `complete(task_id, output_size, now)` — marks task done
//!
//! **Missing link**: NPU submissions were never gated by CapToken. Any Silo
//! could submit inference workloads to the NPU, potentially monopolizing
//! the neural pipeline and starving the system's Synapse AI subsystem.
//!
//! This module provides `NpuSynapseBridge`:
//! 1. `submit_with_cap_check()` — Synapse:EXEC required for NPU inference
//! 2. `on_apic_tick()` — drives NpuScheduler::schedule() periodically

extern crate alloc;

use crate::npu_sched::{NpuScheduler, TaskType, NpuPriority};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct NpuBridgeStats {
    pub submissions_allowed: u64,
    pub submissions_denied:  u64,
    pub schedule_ticks:      u64,
}

pub struct NpuSynapseBridge {
    pub scheduler: NpuScheduler,
    pub stats:     NpuBridgeStats,
    tick_interval: u64,
    last_tick:     u64,
}

impl NpuSynapseBridge {
    pub fn new() -> Self {
        let mut scheduler = NpuScheduler::new(512 * 1024 * 1024); // 512MB model cache
        // Register NPU cores (up to 2 on typical hardware)
        scheduler.add_core(0);
        scheduler.add_core(1);

        NpuSynapseBridge {
            scheduler,
            stats: NpuBridgeStats::default(),
            tick_interval: 4, // schedule every 4 APIC ticks
            last_tick: 0,
        }
    }

    /// Submit an NPU inference task. Requires Synapse:EXEC cap.
    pub fn submit_with_cap_check(
        &mut self,
        silo_id: u64,
        model_id: u64,
        task_type: TaskType,
        priority: NpuPriority,
        input_size: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> Option<u64> {
        if !forge.check(silo_id, CapType::Synapse, CAP_EXEC, 0, tick) {
            self.stats.submissions_denied += 1;
            crate::serial_println!(
                "[NPU BRIDGE] Silo {} denied NPU task — no Synapse:EXEC cap", silo_id
            );
            return None;
        }
        self.stats.submissions_allowed += 1;
        let task_id = self.scheduler.submit(silo_id, model_id, task_type, priority, input_size, tick);
        Some(task_id)
    }

    /// Drive NpuScheduler periodic scheduling from the APIC timer.
    pub fn on_apic_tick(&mut self, tick: u64) {
        if tick - self.last_tick >= self.tick_interval {
            self.last_tick = tick;
            self.stats.schedule_ticks += 1;
            self.scheduler.schedule(tick);
        }
    }

    /// Signal task completion (called from NPU interrupt handler).
    pub fn on_task_complete(&mut self, task_id: u64, output_size: u64, tick: u64) {
        self.scheduler.complete(task_id, output_size, tick);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NpuBridge: allowed={} denied={} sched_ticks={}",
            self.stats.submissions_allowed, self.stats.submissions_denied, self.stats.schedule_ticks
        );
    }
}
