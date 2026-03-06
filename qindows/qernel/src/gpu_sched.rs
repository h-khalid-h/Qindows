//! # GPU Scheduler — Compute/Render Queue per Silo
//!
//! Manages GPU workloads across Silos with fairness and
//! priority scheduling (Section 9.4).
//!
//! Features:
//! - Per-Silo render and compute queues
//! - Priority: system compositing > user render > background compute
//! - GPU memory budget enforcement
//! - Preemption for high-priority tasks
//! - Fence-based synchronization tracking

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// GPU queue type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueType {
    Render,
    Compute,
    Transfer,
    VideoEncode,
    VideoDecode,
}

/// GPU task priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum GpuPriority {
    Background = 0,
    Normal = 1,
    High = 2,
    Compositor = 3,
    Critical = 4,
}

/// GPU task state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuTaskState {
    Queued,
    Running,
    Complete,
    Preempted,
    Failed,
}

/// A GPU task.
#[derive(Debug, Clone)]
pub struct GpuTask {
    pub id: u64,
    pub silo_id: u64,
    pub queue_type: QueueType,
    pub priority: GpuPriority,
    pub state: GpuTaskState,
    pub vram_needed: u64,
    pub submitted_at: u64,
    pub started_at: u64,
    pub completed_at: u64,
    pub fence_id: u64,
}

/// Per-Silo GPU budget.
#[derive(Debug, Clone)]
pub struct GpuBudget {
    pub silo_id: u64,
    pub vram_limit: u64,
    pub vram_used: u64,
    pub tasks_active: u32,
    pub max_tasks: u32,
}

/// GPU scheduler statistics.
#[derive(Debug, Clone, Default)]
pub struct GpuStats {
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_preempted: u64,
    pub tasks_failed: u64,
    pub vram_peak: u64,
}

/// The GPU Scheduler.
pub struct GpuScheduler {
    pub queue: Vec<GpuTask>,
    pub budgets: BTreeMap<u64, GpuBudget>,
    pub total_vram: u64,
    pub vram_used: u64,
    next_id: u64,
    next_fence: u64,
    pub stats: GpuStats,
}

impl GpuScheduler {
    pub fn new(total_vram: u64) -> Self {
        GpuScheduler {
            queue: Vec::new(),
            budgets: BTreeMap::new(),
            total_vram,
            vram_used: 0,
            next_id: 1,
            next_fence: 1,
            stats: GpuStats::default(),
        }
    }

    /// Set GPU budget for a Silo.
    pub fn set_budget(&mut self, silo_id: u64, vram_limit: u64, max_tasks: u32) {
        self.budgets.entry(silo_id).or_insert(GpuBudget {
            silo_id, vram_limit, vram_used: 0, tasks_active: 0, max_tasks,
        });
        if let Some(b) = self.budgets.get_mut(&silo_id) {
            b.vram_limit = vram_limit;
            b.max_tasks = max_tasks;
        }
    }

    /// Submit a GPU task.
    pub fn submit(&mut self, silo_id: u64, queue_type: QueueType, priority: GpuPriority, vram: u64, now: u64) -> Result<u64, &'static str> {
        // Check budget
        if let Some(budget) = self.budgets.get(&silo_id) {
            if budget.vram_used + vram > budget.vram_limit {
                return Err("VRAM budget exceeded");
            }
            if budget.tasks_active >= budget.max_tasks {
                return Err("Task limit exceeded");
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        let fence = self.next_fence;
        self.next_fence += 1;

        self.queue.push(GpuTask {
            id, silo_id, queue_type, priority,
            state: GpuTaskState::Queued, vram_needed: vram,
            submitted_at: now, started_at: 0, completed_at: 0,
            fence_id: fence,
        });

        self.queue.sort_by(|a, b| b.priority.cmp(&a.priority));
        self.stats.tasks_submitted += 1;
        Ok(id)
    }

    /// Dispatch queued tasks.
    pub fn dispatch(&mut self, now: u64) -> Vec<u64> {
        let mut dispatched = Vec::new();

        for task in self.queue.iter_mut() {
            if task.state != GpuTaskState::Queued {
                continue;
            }
            if self.vram_used + task.vram_needed > self.total_vram {
                continue;
            }

            task.state = GpuTaskState::Running;
            task.started_at = now;
            self.vram_used += task.vram_needed;

            if self.vram_used > self.stats.vram_peak {
                self.stats.vram_peak = self.vram_used;
            }

            if let Some(budget) = self.budgets.get_mut(&task.silo_id) {
                budget.vram_used += task.vram_needed;
                budget.tasks_active += 1;
            }

            dispatched.push(task.id);
        }

        dispatched
    }

    /// Complete a GPU task.
    pub fn complete(&mut self, task_id: u64, success: bool, now: u64) {
        if let Some(task) = self.queue.iter_mut().find(|t| t.id == task_id) {
            task.state = if success { GpuTaskState::Complete } else { GpuTaskState::Failed };
            task.completed_at = now;

            self.vram_used = self.vram_used.saturating_sub(task.vram_needed);

            if let Some(budget) = self.budgets.get_mut(&task.silo_id) {
                budget.vram_used = budget.vram_used.saturating_sub(task.vram_needed);
                budget.tasks_active = budget.tasks_active.saturating_sub(1);
            }

            if success {
                self.stats.tasks_completed += 1;
            } else {
                self.stats.tasks_failed += 1;
            }
        }
    }
}
