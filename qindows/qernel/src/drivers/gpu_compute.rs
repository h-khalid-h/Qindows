//! # GPU Compute Dispatcher
//!
//! Submits general-purpose compute workloads to the GPU.
//! Used by Synapse for neural inference, Prism for parallel hashing,
//! and Aether for image processing / blur effects.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Compute shader stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaderStage {
    Compute,
    Vertex,
    Fragment,
}

/// GPU buffer usage hints.
#[derive(Debug, Clone, Copy)]
pub enum BufferUsage {
    /// Uploaded once, read many times
    Static,
    /// Updated frequently
    Dynamic,
    /// Written by GPU, read by CPU
    Readback,
    /// Shared between compute and graphics
    Shared,
}

/// A GPU buffer handle.
#[derive(Debug, Clone)]
pub struct GpuBuffer {
    /// Buffer ID
    pub id: u64,
    /// Size in bytes
    pub size: u64,
    /// Usage
    pub usage: BufferUsage,
    /// Is this buffer currently mapped?
    pub mapped: bool,
}

/// A compute shader program.
#[derive(Debug, Clone)]
pub struct ComputeShader {
    /// Shader ID
    pub id: u64,
    /// Name
    pub name: String,
    /// Work group size (x, y, z)
    pub workgroup_size: (u32, u32, u32),
    /// Number of bindings (buffers/textures)
    pub binding_count: u32,
    /// Compiled?
    pub compiled: bool,
}

/// A compute dispatch command.
#[derive(Debug, Clone)]
pub struct DispatchCommand {
    /// Shader to execute
    pub shader_id: u64,
    /// Work group counts (x, y, z)
    pub groups: (u32, u32, u32),
    /// Input buffers (binding index → buffer ID)
    pub inputs: Vec<(u32, u64)>,
    /// Output buffers (binding index → buffer ID)
    pub outputs: Vec<(u32, u64)>,
    /// Push constants (small uniform data)
    pub push_constants: Vec<u8>,
}

/// Compute job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Queued,
    Running,
    Complete,
    Failed,
}

/// A submitted compute job.
#[derive(Debug, Clone)]
pub struct ComputeJob {
    pub job_id: u64,
    pub command: DispatchCommand,
    pub status: JobStatus,
    pub submit_time: u64,
    pub complete_time: Option<u64>,
}

/// The GPU Compute Dispatcher.
pub struct ComputeDispatcher {
    /// Registered shaders
    pub shaders: Vec<ComputeShader>,
    /// Allocated buffers
    pub buffers: Vec<GpuBuffer>,
    /// Job queue
    pub jobs: Vec<ComputeJob>,
    /// Next IDs
    next_shader_id: u64,
    next_buffer_id: u64,
    next_job_id: u64,
    /// Stats
    pub stats: ComputeStats,
}

/// Compute statistics.
#[derive(Debug, Clone, Default)]
pub struct ComputeStats {
    pub shaders_compiled: u64,
    pub dispatches: u64,
    pub total_workgroups: u64,
    pub buffers_allocated: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
}

impl ComputeDispatcher {
    pub fn new() -> Self {
        ComputeDispatcher {
            shaders: Vec::new(),
            buffers: Vec::new(),
            jobs: Vec::new(),
            next_shader_id: 1,
            next_buffer_id: 1,
            next_job_id: 1,
            stats: ComputeStats::default(),
        }
    }

    /// Register a compute shader.
    pub fn create_shader(
        &mut self,
        name: &str,
        workgroup_size: (u32, u32, u32),
        bindings: u32,
    ) -> u64 {
        let id = self.next_shader_id;
        self.next_shader_id += 1;

        self.shaders.push(ComputeShader {
            id,
            name: String::from(name),
            workgroup_size,
            binding_count: bindings,
            compiled: true,
        });

        self.stats.shaders_compiled += 1;
        id
    }

    /// Allocate a GPU buffer.
    pub fn create_buffer(&mut self, size: u64, usage: BufferUsage) -> u64 {
        let id = self.next_buffer_id;
        self.next_buffer_id += 1;

        self.buffers.push(GpuBuffer {
            id, size, usage, mapped: false,
        });

        self.stats.buffers_allocated += 1;
        id
    }

    /// Upload data to a GPU buffer.
    pub fn upload(&mut self, buffer_id: u64, data: &[u8]) -> Result<(), &'static str> {
        let buf = self.buffers.iter().find(|b| b.id == buffer_id)
            .ok_or("Buffer not found")?;
        if data.len() as u64 > buf.size {
            return Err("Data exceeds buffer size");
        }
        self.stats.bytes_uploaded += data.len() as u64;
        Ok(())
    }

    /// Submit a compute dispatch.
    pub fn dispatch(&mut self, command: DispatchCommand) -> Result<u64, &'static str> {
        // Verify shader exists
        if !self.shaders.iter().any(|s| s.id == command.shader_id) {
            return Err("Shader not found");
        }

        let job_id = self.next_job_id;
        self.next_job_id += 1;

        let total_groups = command.groups.0 as u64
            * command.groups.1 as u64
            * command.groups.2 as u64;

        self.stats.dispatches += 1;
        self.stats.total_workgroups += total_groups;

        self.jobs.push(ComputeJob {
            job_id,
            command,
            status: JobStatus::Queued,
            submit_time: 0,
            complete_time: None,
        });

        Ok(job_id)
    }

    /// Check job status.
    pub fn job_status(&self, job_id: u64) -> Option<JobStatus> {
        self.jobs.iter().find(|j| j.job_id == job_id).map(|j| j.status)
    }

    /// Free a GPU buffer.
    pub fn destroy_buffer(&mut self, buffer_id: u64) {
        self.buffers.retain(|b| b.id != buffer_id);
    }
}
