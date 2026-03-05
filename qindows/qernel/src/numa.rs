//! # NUMA Topology Awareness
//!
//! Non-Uniform Memory Access topology discovery and optimization.
//! Modern multi-socket / chiplet CPUs (AMD EPYC, Intel Sapphire Rapids)
//! have varying memory latencies depending on which NUMA node a core
//! belongs to. The Qernel uses this information to:
//!
//! - Allocate memory on the same node as the requesting core
//! - Schedule Fibers on cores closest to their memory
//! - Balance Silo placement across nodes for fairness

extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

/// Maximum supported NUMA nodes.
pub const MAX_NUMA_NODES: usize = 64;
/// Maximum supported CPU cores.
pub const MAX_CPUS: usize = 256;

/// A NUMA node — a group of cores sharing a local memory controller.
#[derive(Debug, Clone)]
pub struct NumaNode {
    /// Node ID (0-based)
    pub id: u8,
    /// CPU core IDs belonging to this node
    pub cores: Vec<u8>,
    /// Base physical address of local memory
    pub mem_base: u64,
    /// Size of local memory in bytes
    pub mem_size: u64,
    /// Free memory on this node
    pub mem_free: u64,
    /// Distance to other nodes (index = target node, value = latency units)
    /// Self-distance is always 10 (ACPI SLIT convention).
    pub distances: Vec<u8>,
    /// Total allocations served from this node
    pub alloc_count: AtomicU64,
    /// Total cross-node (remote) accesses detected
    pub remote_accesses: AtomicU64,
}

impl NumaNode {
    pub fn new(id: u8, mem_base: u64, mem_size: u64) -> Self {
        NumaNode {
            id,
            cores: Vec::new(),
            mem_base,
            mem_size,
            mem_free: mem_size,
            distances: Vec::new(),
            alloc_count: AtomicU64::new(0),
            remote_accesses: AtomicU64::new(0),
        }
    }

    /// Check if a physical address belongs to this node's memory range.
    pub fn contains_addr(&self, phys_addr: u64) -> bool {
        phys_addr >= self.mem_base && phys_addr < self.mem_base.saturating_add(self.mem_size)
    }

    /// Check if a CPU core belongs to this node.
    pub fn has_core(&self, core_id: u8) -> bool {
        self.cores.contains(&core_id)
    }

    /// Get the distance (latency cost) to another node.
    pub fn distance_to(&self, target_node: u8) -> u8 {
        self.distances.get(target_node as usize).copied().unwrap_or(255)
    }

    /// Memory pressure ratio (0.0 = empty, 1.0 = full).
    pub fn pressure(&self) -> f64 {
        if self.mem_size == 0 { return 1.0; }
        1.0 - (self.mem_free as f64 / self.mem_size as f64)
    }
}

/// CPU-to-Node mapping.
#[derive(Debug, Clone, Copy)]
pub struct CpuTopology {
    /// Which NUMA node this CPU belongs to
    pub node_id: u8,
    /// Physical core ID
    pub core_id: u8,
    /// Is this a logical (hyper-threaded) core?
    pub is_logical: bool,
    /// Sibling thread (for SMT pairs)
    pub smt_sibling: Option<u8>,
    /// L3 cache shared group (cores sharing the same L3)
    pub l3_group: u8,
}

/// NUMA allocation policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumaPolicy {
    /// Allocate on the local node of the requesting core (default)
    Local,
    /// Interleave allocations round-robin across all nodes
    Interleave,
    /// Bind to a specific node
    Bind(u8),
    /// Prefer a node but fall back to others if full
    Preferred(u8),
    /// Allocate on the node with the most free memory
    LeastLoaded,
}

/// NUMA topology manager.
pub struct NumaTopology {
    /// Discovered NUMA nodes
    pub nodes: Vec<NumaNode>,
    /// Per-CPU topology info
    pub cpus: Vec<CpuTopology>,
    /// Total system memory across all nodes
    pub total_memory: u64,
    /// Total free memory across all nodes
    pub total_free: u64,
    /// Default allocation policy
    pub default_policy: NumaPolicy,
    /// Interleave counter (for round-robin)
    interleave_next: usize,
    /// Is the topology initialized?
    pub initialized: bool,
}

impl NumaTopology {
    pub fn new() -> Self {
        NumaTopology {
            nodes: Vec::new(),
            cpus: Vec::new(),
            total_memory: 0,
            total_free: 0,
            default_policy: NumaPolicy::Local,
            interleave_next: 0,
            initialized: false,
        }
    }

    /// Discover topology from ACPI SRAT/SLIT tables.
    ///
    /// In production: parse the System Resource Affinity Table (SRAT)
    /// for CPU-to-node and memory-to-node mappings, and the System
    /// Locality Information Table (SLIT) for inter-node distances.
    pub fn discover(&mut self, num_nodes: u8, num_cpus: u8) {
        // Create nodes with equal memory split (simplified)
        let per_node_mem = 4 * 1024 * 1024 * 1024u64; // 4 GiB per node

        for i in 0..num_nodes {
            let base = i as u64 * per_node_mem;
            let mut node = NumaNode::new(i, base, per_node_mem);

            // Build distance vector (SLIT-style)
            for j in 0..num_nodes {
                if i == j {
                    node.distances.push(10); // Local
                } else if (i as i8 - j as i8).unsigned_abs() == 1 {
                    node.distances.push(20); // Adjacent
                } else {
                    node.distances.push(30); // Remote
                }
            }

            self.nodes.push(node);
        }

        // Assign CPUs to nodes round-robin
        let cpus_per_node = if num_nodes > 0 {
            (num_cpus as usize + num_nodes as usize - 1) / num_nodes as usize
        } else {
            num_cpus as usize
        };

        for cpu in 0..num_cpus {
            let node_id = (cpu as usize / cpus_per_node).min(num_nodes.saturating_sub(1) as usize) as u8;

            if let Some(node) = self.nodes.get_mut(node_id as usize) {
                node.cores.push(cpu);
            }

            self.cpus.push(CpuTopology {
                node_id,
                core_id: cpu,
                is_logical: cpu % 2 == 1, // Simplified: odd = HT
                smt_sibling: if cpu % 2 == 0 {
                    Some(cpu.saturating_add(1))
                } else {
                    Some(cpu.saturating_sub(1))
                },
                l3_group: node_id,
            });
        }

        self.total_memory = num_nodes as u64 * per_node_mem;
        self.total_free = self.total_memory;
        self.initialized = true;

        crate::serial_println!(
            "[OK] NUMA: {} nodes, {} CPUs, {} GiB total",
            num_nodes, num_cpus, self.total_memory / (1024 * 1024 * 1024)
        );
    }

    /// Get the NUMA node for a given CPU core.
    pub fn node_for_cpu(&self, core_id: u8) -> Option<u8> {
        self.cpus.get(core_id as usize).map(|t| t.node_id)
    }

    /// Select the best NUMA node for a memory allocation.
    pub fn select_node(&mut self, requesting_core: u8, policy: NumaPolicy) -> Option<u8> {
        if self.nodes.is_empty() {
            return None;
        }

        let node_id = match policy {
            NumaPolicy::Local => {
                self.node_for_cpu(requesting_core).unwrap_or(0)
            }
            NumaPolicy::Bind(n) => n,
            NumaPolicy::Preferred(n) => {
                if self.nodes.get(n as usize).map_or(false, |node| node.mem_free > 0) {
                    n
                } else {
                    // Fall back to least loaded
                    self.find_least_loaded()
                }
            }
            NumaPolicy::LeastLoaded => {
                self.find_least_loaded()
            }
            NumaPolicy::Interleave => {
                let n = self.interleave_next % self.nodes.len();
                self.interleave_next = self.interleave_next.wrapping_add(1);
                n as u8
            }
        };

        // Record the allocation
        if let Some(node) = self.nodes.get(node_id as usize) {
            node.alloc_count.fetch_add(1, Ordering::Relaxed);

            // Check for cross-node access
            let local_node = self.node_for_cpu(requesting_core).unwrap_or(0);
            if local_node != node_id {
                node.remote_accesses.fetch_add(1, Ordering::Relaxed);
            }
        }

        Some(node_id)
    }

    /// Find the node with the most free memory.
    fn find_least_loaded(&self) -> u8 {
        self.nodes.iter()
            .max_by_key(|n| n.mem_free)
            .map(|n| n.id)
            .unwrap_or(0)
    }

    /// Check if a physical address is local to a CPU core.
    pub fn is_local(&self, core_id: u8, phys_addr: u64) -> bool {
        let node_id = self.node_for_cpu(core_id).unwrap_or(0);
        self.nodes.get(node_id as usize)
            .map_or(false, |n| n.contains_addr(phys_addr))
    }

    /// Get allocation statistics per node.
    pub fn stats(&self) -> Vec<(u8, u64, u64, f64)> {
        self.nodes.iter().map(|n| (
            n.id,
            n.alloc_count.load(Ordering::Relaxed),
            n.remote_accesses.load(Ordering::Relaxed),
            n.pressure(),
        )).collect()
    }
}
