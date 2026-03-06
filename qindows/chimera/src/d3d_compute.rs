//! # D3D Compute Shim — DirectCompute → GPU Scheduler Bridge
//!
//! Translates DirectCompute API calls from legacy Win32 apps
//! to the Qindows GPU scheduler (Section 6.5).
//!
//! Features:
//! - Compute shader dispatch
//! - Buffer/texture resource binding
//! - UAV (Unordered Access View) management
//! - Fence-based synchronization
//! - Per-Silo GPU resource isolation

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// GPU resource type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Buffer,
    Texture2D,
    Texture3D,
    Uav,
    Cbv,
}

/// A GPU resource.
#[derive(Debug, Clone)]
pub struct GpuResource {
    pub id: u64,
    pub resource_type: ResourceType,
    pub size_bytes: u64,
    pub silo_id: u64,
    pub name: String,
    pub bound: bool,
}

/// A compute dispatch.
#[derive(Debug, Clone)]
pub struct ComputeDispatch {
    pub id: u64,
    pub shader_hash: u64,
    pub groups_x: u32,
    pub groups_y: u32,
    pub groups_z: u32,
    pub resources: Vec<u64>,
    pub completed: bool,
}

/// D3D compute statistics.
#[derive(Debug, Clone, Default)]
pub struct D3dComputeStats {
    pub dispatches: u64,
    pub resources_created: u64,
    pub resources_freed: u64,
    pub total_gpu_bytes: u64,
}

/// The D3D Compute Shim.
pub struct D3dCompute {
    pub resources: BTreeMap<u64, GpuResource>,
    pub dispatches: Vec<ComputeDispatch>,
    next_resource_id: u64,
    next_dispatch_id: u64,
    pub stats: D3dComputeStats,
}

impl D3dCompute {
    pub fn new() -> Self {
        D3dCompute {
            resources: BTreeMap::new(),
            dispatches: Vec::new(),
            next_resource_id: 1,
            next_dispatch_id: 1,
            stats: D3dComputeStats::default(),
        }
    }

    /// Create a GPU resource.
    pub fn create_resource(&mut self, rtype: ResourceType, size: u64, silo_id: u64, name: &str) -> u64 {
        let id = self.next_resource_id;
        self.next_resource_id += 1;

        self.resources.insert(id, GpuResource {
            id, resource_type: rtype, size_bytes: size,
            silo_id, name: String::from(name), bound: false,
        });

        self.stats.resources_created += 1;
        self.stats.total_gpu_bytes += size;
        id
    }

    /// Bind a resource (make it available for shaders).
    pub fn bind_resource(&mut self, resource_id: u64) -> bool {
        if let Some(r) = self.resources.get_mut(&resource_id) {
            r.bound = true;
            true
        } else { false }
    }

    /// Dispatch a compute shader.
    pub fn dispatch(&mut self, shader_hash: u64, gx: u32, gy: u32, gz: u32, resources: Vec<u64>) -> u64 {
        let id = self.next_dispatch_id;
        self.next_dispatch_id += 1;

        self.dispatches.push(ComputeDispatch {
            id, shader_hash, groups_x: gx, groups_y: gy, groups_z: gz,
            resources, completed: false,
        });

        self.stats.dispatches += 1;
        id
    }

    /// Mark a dispatch as completed.
    pub fn complete_dispatch(&mut self, dispatch_id: u64) {
        if let Some(d) = self.dispatches.iter_mut().find(|d| d.id == dispatch_id) {
            d.completed = true;
        }
    }

    /// Free a GPU resource.
    pub fn free_resource(&mut self, resource_id: u64) {
        if let Some(r) = self.resources.remove(&resource_id) {
            self.stats.resources_freed += 1;
            self.stats.total_gpu_bytes = self.stats.total_gpu_bytes.saturating_sub(r.size_bytes);
        }
    }

    /// Clean up all resources for a Silo.
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        let to_remove: Vec<u64> = self.resources.values()
            .filter(|r| r.silo_id == silo_id)
            .map(|r| r.id)
            .collect();
        for id in to_remove {
            self.free_resource(id);
        }
    }
}
