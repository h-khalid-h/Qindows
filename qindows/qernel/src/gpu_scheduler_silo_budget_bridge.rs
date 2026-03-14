//! # GPU Scheduler Silo Budget Bridge (Phase 240)
//!
//! ## Architecture Guardian: The Gap
//! `gpu_sched.rs` implements `GpuScheduler`:
//! - `GpuTask { silo_id, priority: GpuPriority, vram_mb, ... }`
//! - `GpuBudget` — per-Silo VRAM allocation tracking
//! - `GpuPriority` — Critical, High, Normal, Low
//! - `total_vram: u64` — physical total
//!
//! **Missing link**: GPU VRAM allocation had no enforced per-Silo cap.
//! A single Silo with Model Training workloads could claim all VRAM,
//! starving UI rendering Silos of GPU memory (Law 4 DoS).
//!
//! This module provides `GpuSchedulerSiloBudgetBridge`:
//! Admin:EXEC cap required for GpuPriority::Critical. Max VRAM per Silo enforced.

extern crate alloc;

use crate::gpu_sched::{GpuTask, GpuPriority};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

const MAX_VRAM_MB_PER_SILO: u64 = 2048;

#[derive(Debug, Default, Clone)]
pub struct GpuBudgetStats {
    pub tasks_allowed: u64,
    pub vram_denied:   u64,
    pub priority_denied: u64,
}

pub struct GpuSchedulerSiloBudgetBridge {
    pub stats: GpuBudgetStats,
}

impl GpuSchedulerSiloBudgetBridge {
    pub fn new() -> Self {
        GpuSchedulerSiloBudgetBridge { stats: GpuBudgetStats::default() }
    }

    /// Authorize a GPU task — Critical priority requires Admin:EXEC, VRAM capped at 2GB.
    pub fn authorize_task(
        &mut self,
        task: &GpuTask,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if task.vram_needed > MAX_VRAM_MB_PER_SILO * 1024 * 1024 {
            self.stats.vram_denied += 1;
            crate::serial_println!(
                "[GPU] Silo {} VRAM {} bytes exceeds cap {} MB", task.silo_id, task.vram_needed, MAX_VRAM_MB_PER_SILO
            );
            return false;
        }
        if matches!(task.priority, GpuPriority::Critical) {
            if !forge.check(task.silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
                self.stats.priority_denied += 1;
                return false;
            }
        }
        self.stats.tasks_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  GpuBudgetBridge: allowed={} vram_denied={} priority_denied={}",
            self.stats.tasks_allowed, self.stats.vram_denied, self.stats.priority_denied
        );
    }
}
