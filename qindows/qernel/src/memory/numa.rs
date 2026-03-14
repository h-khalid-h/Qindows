//! # NUMA Allocator
//!
//! Non-Uniform Memory Access architecture support.
//! Enables node-aware frame allocation for optimal core locality
//! on high-end hardware.

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// A physical NUMA node.
#[derive(Debug, Clone)]
pub struct NumaNode {
    pub id: u32,
    pub start_paddr: u64,
    pub end_paddr: u64,
    pub total_frames: u64,
    pub free_frames: u64,
}

/// The Global NUMA Allocator
pub struct NumaAllocator {
    nodes: Vec<NumaNode>,
    pub initialized: bool,
}

impl NumaAllocator {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            initialized: false,
        }
    }

    /// Discover NUMA nodes from system tables (e.g. SRAT)
    pub fn discover(&mut self) {
        // Mock discovery for the genesis alpha
        // In a real system, we parse ACPI SRAT table
        self.nodes.push(NumaNode {
            id: 0,
            start_paddr: 0x0,
            end_paddr: 0x8000_0000, // 2GB
            total_frames: 524288,
            free_frames: 500000,
        });
        
        // Pseudo second node
        self.nodes.push(NumaNode {
            id: 1,
            start_paddr: 0x1_0000_0000,
            end_paddr: 0x1_8000_0000, // 2GB
            total_frames: 524288,
            free_frames: 524288,
        });
        
        self.initialized = true;
    }

    /// Get information about a requested NUMA node
    pub fn node_info(&self, node_id: u32) -> Option<NumaNode> {
        self.nodes.iter().find(|n| n.id == node_id).cloned()
    }

    /// Allocate frames specifically from a requested NUMA node
    pub fn alloc_frames_on_node(&mut self, node_id: u32, count: u64) -> Option<u64> {
        if let Some(node) = self.nodes.iter_mut().find(|n| n.id == node_id) {
            if node.free_frames >= count {
                node.free_frames -= count;
                // Return a mock physical address based on node range
                return Some(node.start_paddr + ((node.total_frames - node.free_frames) * 4096));
            }
        }
        None
    }
}
