//! # Nexus Edge-Kernel — Process Offloading to Cloud
//!
//! Right-click a heavy process (e.g., 3D render) to "Scale to Cloud."
//! The Qernel serializes local Fibers and Memory Objects and moves
//! them to high-performance cloud Q-Servers (Section 5).
//!
//! Architecture:
//! 1. User selects "Scale to Cloud" for a Silo
//! 2. All Fibers in the Silo are frozen and serialized
//! 3. Memory pages are streamed to the edge node via Q-Fabric
//! 4. The remote node hydrates the Silo and resumes execution
//! 5. UI stays local — only render commands are streamed back
//! 6. When done, results migrate back and local Silo resumes

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Edge node status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeStatus {
    /// Available for offloading
    Available,
    /// Currently running an offloaded task
    Busy,
    /// Node is overloaded
    Overloaded,
    /// Node is unreachable
    Offline,
}

/// An edge node (remote compute resource).
#[derive(Debug, Clone)]
pub struct EdgeNode {
    /// Node ID (from mesh routing)
    pub id: [u8; 32],
    /// Human-readable name
    pub name: String,
    /// Status
    pub status: NodeStatus,
    /// Available CPU cores
    pub cpu_cores: u32,
    /// Available GPU compute units
    pub gpu_units: u32,
    /// Available RAM (bytes)
    pub ram_available: u64,
    /// Round-trip latency (ms)
    pub latency_ms: u32,
    /// Bandwidth to this node (bytes/sec)
    pub bandwidth: u64,
    /// Trust score (from Sentinel reputation)
    pub trust_score: u8,
    /// Tasks currently running on this node
    pub active_tasks: u32,
    /// Total tasks completed
    pub tasks_completed: u64,
}

/// Offload task state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffloadState {
    /// Queued for offloading
    Queued,
    /// Serializing local Silo state
    Serializing,
    /// Streaming data to edge node
    Streaming,
    /// Running on the edge node
    Running,
    /// Downloading results
    Retrieving,
    /// Completed successfully
    Complete,
    /// Failed (will retry or fall back to local)
    Failed,
    /// Cancelled by user
    Cancelled,
}

/// Vibe requirements for selecting an edge node.
#[derive(Debug, Clone)]
pub struct VibeRequirement {
    /// Minimum CPU cores needed
    pub min_cores: u32,
    /// Minimum GPU units needed
    pub min_gpu: u32,
    /// Minimum RAM needed (bytes)
    pub min_ram: u64,
    /// Maximum acceptable latency (ms)
    pub max_latency: u32,
    /// Minimum trust score
    pub min_trust: u8,
}

impl Default for VibeRequirement {
    fn default() -> Self {
        VibeRequirement {
            min_cores: 2,
            min_gpu: 0,
            min_ram: 512 * 1024 * 1024, // 512 MiB
            max_latency: 100,
            min_trust: 50,
        }
    }
}

/// An offload task.
#[derive(Debug, Clone)]
pub struct OffloadTask {
    /// Task ID
    pub id: u64,
    /// Source Silo ID (local)
    pub silo_id: u64,
    /// Target edge node
    pub edge_node: [u8; 32],
    /// State
    pub state: OffloadState,
    /// Serialized state size (bytes)
    pub state_size: u64,
    /// Bytes transferred so far
    pub bytes_transferred: u64,
    /// Start time
    pub started_at: u64,
    /// Estimated completion time
    pub estimated_completion: u64,
    /// Number of fibers offloaded
    pub fiber_count: u32,
    /// Number of memory pages offloaded
    pub page_count: u64,
}

/// Edge-Kernel statistics.
#[derive(Debug, Clone, Default)]
pub struct EdgeStats {
    pub tasks_offloaded: u64,
    pub tasks_completed: u64,
    pub tasks_failed: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
    pub compute_seconds_saved: u64,
    pub average_speedup: f32,
}

/// The Edge-Kernel Manager.
pub struct EdgeKernel {
    /// Known edge nodes
    pub nodes: BTreeMap<[u8; 32], EdgeNode>,
    /// Active offload tasks
    pub tasks: BTreeMap<u64, OffloadTask>,
    /// Next task ID
    next_task_id: u64,
    /// Statistics
    pub stats: EdgeStats,
}

impl EdgeKernel {
    pub fn new() -> Self {
        EdgeKernel {
            nodes: BTreeMap::new(),
            tasks: BTreeMap::new(),
            next_task_id: 1,
            stats: EdgeStats::default(),
        }
    }

    /// Register a discovered edge node.
    pub fn register_node(&mut self, node: EdgeNode) {
        self.nodes.insert(node.id, node);
    }

    /// Find the best edge node matching requirements.
    pub fn find_best_node(&self, req: &VibeRequirement) -> Option<&EdgeNode> {
        self.nodes.values()
            .filter(|n| {
                n.status == NodeStatus::Available
                    && n.cpu_cores >= req.min_cores
                    && n.gpu_units >= req.min_gpu
                    && n.ram_available >= req.min_ram
                    && n.latency_ms <= req.max_latency
                    && n.trust_score >= req.min_trust
            })
            .min_by_key(|n| {
                // Score: lower = better (latency-weighted)
                (n.latency_ms as u64) * 10
                    + (100u64.saturating_sub(n.trust_score as u64))
                    + n.active_tasks as u64 * 50
            })
    }

    /// Offload a Silo to a cloud edge node.
    pub fn offload(
        &mut self,
        silo_id: u64,
        fiber_count: u32,
        page_count: u64,
        state_size: u64,
        req: &VibeRequirement,
        now: u64,
    ) -> Result<u64, &'static str> {
        let node_id = self.find_best_node(req)
            .ok_or("No suitable edge node found")?
            .id;

        let task_id = self.next_task_id;
        self.next_task_id += 1;

        self.tasks.insert(task_id, OffloadTask {
            id: task_id,
            silo_id,
            edge_node: node_id,
            state: OffloadState::Queued,
            state_size,
            bytes_transferred: 0,
            started_at: now,
            estimated_completion: 0,
            fiber_count,
            page_count,
        });

        // Mark node as busy
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.active_tasks += 1;
            if node.active_tasks >= node.cpu_cores {
                node.status = NodeStatus::Busy;
            }
        }

        self.stats.tasks_offloaded += 1;
        Ok(task_id)
    }

    /// Advance a task through its state machine.
    pub fn tick_task(&mut self, task_id: u64) {
        let task = match self.tasks.get_mut(&task_id) {
            Some(t) => t,
            None => return,
        };

        match task.state {
            OffloadState::Queued => {
                task.state = OffloadState::Serializing;
            }
            OffloadState::Serializing => {
                // In production: freeze fibers, serialize registers + stack + heap
                task.state = OffloadState::Streaming;
            }
            OffloadState::Streaming => {
                // Simulate streaming progress
                let chunk = task.state_size / 10;
                task.bytes_transferred = task.bytes_transferred.saturating_add(chunk);
                self.stats.bytes_uploaded = self.stats.bytes_uploaded.saturating_add(chunk);

                if task.bytes_transferred >= task.state_size {
                    task.state = OffloadState::Running;
                }
            }
            OffloadState::Running => {
                // In production: wait for completion signal from edge node
                task.state = OffloadState::Retrieving;
            }
            OffloadState::Retrieving => {
                // Download results
                let result_size = task.state_size / 4; // Results are smaller
                self.stats.bytes_downloaded = self.stats.bytes_downloaded
                    .saturating_add(result_size);
                task.state = OffloadState::Complete;
            }
            OffloadState::Complete | OffloadState::Failed | OffloadState::Cancelled => {}
        }
    }

    /// Finalize completed tasks.
    pub fn finalize(&mut self) {
        let completed: Vec<u64> = self.tasks.iter()
            .filter(|(_, t)| t.state == OffloadState::Complete)
            .map(|(&id, _)| id)
            .collect();

        for id in completed {
            if let Some(task) = self.tasks.remove(&id) {
                // Release edge node capacity
                if let Some(node) = self.nodes.get_mut(&task.edge_node) {
                    node.active_tasks = node.active_tasks.saturating_sub(1);
                    node.tasks_completed += 1;
                    if node.status == NodeStatus::Busy && node.active_tasks < node.cpu_cores {
                        node.status = NodeStatus::Available;
                    }
                }
                self.stats.tasks_completed += 1;
            }
        }
    }

    /// Cancel an offload task and fall back to local execution.
    pub fn cancel(&mut self, task_id: u64) -> Result<(), &'static str> {
        let task = self.tasks.get_mut(&task_id)
            .ok_or("Task not found")?;

        if task.state == OffloadState::Complete {
            return Err("Task already completed");
        }

        task.state = OffloadState::Cancelled;

        // Release edge node
        if let Some(node) = self.nodes.get_mut(&task.edge_node) {
            node.active_tasks = node.active_tasks.saturating_sub(1);
        }

        Ok(())
    }

    /// Get active offload count.
    pub fn active_count(&self) -> usize {
        self.tasks.values()
            .filter(|t| matches!(t.state,
                OffloadState::Queued | OffloadState::Serializing |
                OffloadState::Streaming | OffloadState::Running |
                OffloadState::Retrieving
            ))
            .count()
    }
}
