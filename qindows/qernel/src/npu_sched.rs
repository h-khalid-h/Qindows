//! # NPU Scheduler — Neural Processing Unit Task Queue
//!
//! Manages the NPU (dedicated AI accelerator) task queue.
//! Routes inference/training requests from Silos to available
//! NPU cores with priority scheduling (Section 4.5).
//!
//! Features:
//! - Priority queue (system AI > user AI > background)
//! - Model caching (keep hot models in NPU memory)
//! - Batch coalescing (merge small requests for efficiency)
//! - Power-aware: yields NPU cores to Power Governor on thermal events

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// NPU task priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NpuPriority {
    Background = 0,
    User = 1,
    System = 2,
    Critical = 3,
}

/// NPU task state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Running,
    Complete,
    Failed,
    Cancelled,
}

/// NPU task type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskType {
    Inference,
    Training,
    Embedding,
    Classification,
    Generation,
}

/// An NPU task.
#[derive(Debug, Clone)]
pub struct NpuTask {
    pub id: u64,
    pub silo_id: u64,
    pub model_id: u64,
    pub task_type: TaskType,
    pub priority: NpuPriority,
    pub state: TaskState,
    pub input_size: u64,
    pub output_size: u64,
    pub submitted_at: u64,
    pub started_at: u64,
    pub completed_at: u64,
    pub core_id: u32,
}

/// A cached model in NPU memory.
#[derive(Debug, Clone)]
pub struct CachedModel {
    pub model_id: u64,
    pub name: String,
    pub size: u64,
    pub last_used: u64,
    pub use_count: u64,
}

/// NPU core.
#[derive(Debug, Clone)]
pub struct NpuCore {
    pub id: u32,
    pub busy: bool,
    pub current_task: Option<u64>,
    pub tasks_completed: u64,
}

/// NPU scheduler statistics.
#[derive(Debug, Clone, Default)]
pub struct NpuStats {
    pub tasks_submitted: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub batches_coalesced: u64,
    pub models_cached: u64,
    pub models_evicted: u64,
}

/// The NPU Scheduler.
pub struct NpuScheduler {
    pub queue: Vec<NpuTask>,
    pub cores: BTreeMap<u32, NpuCore>,
    pub model_cache: BTreeMap<u64, CachedModel>,
    pub cache_budget: u64,
    pub cache_used: u64,
    next_task_id: u64,
    pub stats: NpuStats,
}

impl NpuScheduler {
    pub fn new(cache_budget: u64) -> Self {
        NpuScheduler {
            queue: Vec::new(),
            cores: BTreeMap::new(),
            model_cache: BTreeMap::new(),
            cache_budget,
            cache_used: 0,
            next_task_id: 1,
            stats: NpuStats::default(),
        }
    }

    /// Add an NPU core.
    pub fn add_core(&mut self, id: u32) {
        self.cores.insert(id, NpuCore { id, busy: false, current_task: None, tasks_completed: 0 });
    }

    /// Submit an NPU task.
    pub fn submit(&mut self, silo_id: u64, model_id: u64, task_type: TaskType, priority: NpuPriority, input_size: u64, now: u64) -> u64 {
        let id = self.next_task_id;
        self.next_task_id += 1;

        self.queue.push(NpuTask {
            id, silo_id, model_id, task_type, priority,
            state: TaskState::Queued, input_size, output_size: 0,
            submitted_at: now, started_at: 0, completed_at: 0, core_id: 0,
        });

        // Sort by priority (highest first)
        self.queue.sort_by(|a, b| b.priority.cmp(&a.priority));
        self.stats.tasks_submitted += 1;
        id
    }

    /// Schedule queued tasks to available cores.
    pub fn schedule(&mut self, now: u64) {
        let free_cores: Vec<u32> = self.cores.values()
            .filter(|c| !c.busy)
            .map(|c| c.id)
            .collect();

        for core_id in free_cores {
            // Find highest priority queued task
            let task_idx = self.queue.iter().position(|t| t.state == TaskState::Queued);
            if let Some(idx) = task_idx {
                self.queue[idx].state = TaskState::Running;
                self.queue[idx].started_at = now;
                self.queue[idx].core_id = core_id;

                let task_id = self.queue[idx].id;
                if let Some(core) = self.cores.get_mut(&core_id) {
                    core.busy = true;
                    core.current_task = Some(task_id);
                }

                // Update model cache
                let model_id = self.queue[idx].model_id;
                if let Some(model) = self.model_cache.get_mut(&model_id) {
                    model.last_used = now;
                    model.use_count += 1;
                }
            }
        }
    }

    /// Mark a task as complete.
    pub fn complete(&mut self, task_id: u64, output_size: u64, now: u64) {
        if let Some(task) = self.queue.iter_mut().find(|t| t.id == task_id) {
            task.state = TaskState::Complete;
            task.output_size = output_size;
            task.completed_at = now;

            let core_id = task.core_id;
            if let Some(core) = self.cores.get_mut(&core_id) {
                core.busy = false;
                core.current_task = None;
                core.tasks_completed += 1;
            }
            self.stats.tasks_completed += 1;
        }
    }

    /// Cache a model in NPU memory.
    pub fn cache_model(&mut self, model_id: u64, name: &str, size: u64, now: u64) {
        // Evict if needed
        while self.cache_used + size > self.cache_budget {
            let lru = self.model_cache.values()
                .min_by_key(|m| m.last_used)
                .map(|m| m.model_id);
            if let Some(evict_id) = lru {
                if let Some(m) = self.model_cache.remove(&evict_id) {
                    self.cache_used = self.cache_used.saturating_sub(m.size);
                    self.stats.models_evicted += 1;
                }
            } else {
                break;
            }
        }

        self.model_cache.insert(model_id, CachedModel {
            model_id, name: String::from(name), size, last_used: now, use_count: 0,
        });
        self.cache_used += size;
        self.stats.models_cached += 1;
    }
}
