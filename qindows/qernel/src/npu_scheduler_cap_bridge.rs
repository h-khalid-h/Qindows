//! # NPU Scheduler Cap Bridge (Phase 233)
//!
//! ## Architecture Guardian: The Gap
//! `npu_sched.rs` implements `NpuScheduler`:
//! - `NpuTask { silo_id, task_type: TaskType, priority: NpuPriority, ... }`
//! - `TaskType` — Inference, Training, Preprocessing, Vision, NLP
//! - `NpuPriority` — Critical, High, Normal, Low
//!
//! **Missing link**: `NpuPriority::Critical` was assignable by any Silo,
//! allowing a Silo to monopolize all NPU processing time (Law 4 DoS).
//!
//! This module provides `NpuSchedulerCapBridge`:
//! Admin:EXEC cap required to submit with NpuPriority::Critical.

extern crate alloc;

use crate::npu_sched::{NpuTask, NpuPriority};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct NpuCapStats {
    pub tasks_allowed:   u64,
    pub priority_denied: u64,
}

pub struct NpuSchedulerCapBridge {
    pub stats: NpuCapStats,
}

impl NpuSchedulerCapBridge {
    pub fn new() -> Self {
        NpuSchedulerCapBridge { stats: NpuCapStats::default() }
    }

    /// Authorize NPU task — Critical priority requires Admin:EXEC cap.
    pub fn authorize_task(
        &mut self,
        task: &NpuTask,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        match task.priority {
            NpuPriority::Critical => {
                if !forge.check(task.silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
                    self.stats.priority_denied += 1;
                    crate::serial_println!(
                        "[NPU] Silo {} Critical priority denied — no Admin:EXEC cap", task.silo_id
                    );
                    return false;
                }
            }
            _ => {}
        }
        self.stats.tasks_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  NpuCapBridge: allowed={} priority_denied={}",
            self.stats.tasks_allowed, self.stats.priority_denied
        );
    }
}
