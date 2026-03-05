//! # Aether Elastic Rendering — GPU Command-Stream Remoting
//!
//! When local GPU thermals peak or the scene is too complex,
//! Aether offloads the vector scene graph to a cloud Q-Server
//! via command-stream remoting (Section 4.1 / 11.1).
//!
//! Architecture:
//! 1. Local Aether compositor detects thermal/perf threshold exceeded
//! 2. Scene graph is serialized into a compact command stream
//! 3. Command stream is sent to an edge GPU node via Q-Fabric
//! 4. Remote GPU renders the scene, returns compressed vertex data
//! 5. Local device handles final scanout — preserving 0ms input lag
//! 6. Seamless fallback to local when remote node is unavailable

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Render mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Fully local GPU rendering
    Local,
    /// Hybrid: heavy geometry remote, scanout local
    Hybrid,
    /// Fully remote (thin client mode)
    Remote,
    /// Transitioning between modes
    Transitioning,
}

/// Thermal state that triggers elastic scaling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    /// Normal operating temperature
    Normal,
    /// Elevated — approaching throttle point
    Warm,
    /// Throttling — must offload to maintain FPS
    Hot,
    /// Critical — emergency offload
    Critical,
}

/// A render command in the command stream.
#[derive(Debug, Clone)]
pub enum RenderCommand {
    /// Clear the framebuffer
    Clear { color: u32 },
    /// Draw a mesh (vertex buffer reference + transform)
    DrawMesh { mesh_id: u64, transform: [f32; 16] },
    /// Apply a material/shader
    BindMaterial { material_id: u64 },
    /// Set viewport
    SetViewport { x: u32, y: u32, w: u32, h: u32 },
    /// Apply SDF text rendering
    DrawSdfText { text_id: u64, pos_x: f32, pos_y: f32, scale: f32 },
    /// Composite glass effect
    GlassComposite { region_id: u64, blur: f32, tint: u32 },
    /// Present frame (end of command list)
    Present { frame_id: u64 },
}

/// A command stream frame (one frame's worth of commands).
#[derive(Debug, Clone)]
pub struct CommandFrame {
    /// Frame ID
    pub frame_id: u64,
    /// Commands in this frame
    pub commands: Vec<RenderCommand>,
    /// Estimated complexity (higher = heavier to render)
    pub complexity: u64,
    /// Serialized size in bytes
    pub size_bytes: u64,
}

/// A remote GPU node for elastic rendering.
#[derive(Debug, Clone)]
pub struct GpuNode {
    /// Node ID
    pub id: u64,
    /// Node name
    pub name: String,
    /// Available GPU compute units
    pub gpu_units: u32,
    /// VRAM available (bytes)
    pub vram: u64,
    /// Latency to node (ms)
    pub latency_ms: u32,
    /// Is this node currently rendering for us?
    pub active: bool,
    /// Frames rendered
    pub frames_rendered: u64,
}

/// Compressed frame result from remote rendering.
#[derive(Debug, Clone)]
pub struct FrameResult {
    /// Frame ID
    pub frame_id: u64,
    /// Compressed vertex/pixel data size
    pub data_size: u64,
    /// Decode time (microseconds)
    pub decode_us: u64,
    /// Render time on remote (microseconds)
    pub remote_render_us: u64,
}

/// Elastic renderer statistics.
#[derive(Debug, Clone, Default)]
pub struct ElasticStats {
    pub frames_local: u64,
    pub frames_remote: u64,
    pub commands_streamed: u64,
    pub bytes_uploaded: u64,
    pub bytes_downloaded: u64,
    pub mode_switches: u64,
    pub thermal_events: u64,
    pub average_remote_latency_us: u64,
}

/// The Elastic Renderer.
pub struct ElasticRenderer {
    /// Current render mode
    pub mode: RenderMode,
    /// Current thermal state
    pub thermal: ThermalState,
    /// Available GPU nodes
    pub gpu_nodes: BTreeMap<u64, GpuNode>,
    /// Active remote node ID (if in Hybrid/Remote mode)
    pub active_node: Option<u64>,
    /// Complexity threshold for offloading
    pub offload_threshold: u64,
    /// Pending command frames (queued for remote)
    pub pending_frames: Vec<CommandFrame>,
    /// Received results (ready for local scanout)
    pub results: Vec<FrameResult>,
    /// Next frame ID
    next_frame_id: u64,
    /// Statistics
    pub stats: ElasticStats,
}

impl ElasticRenderer {
    pub fn new() -> Self {
        ElasticRenderer {
            mode: RenderMode::Local,
            thermal: ThermalState::Normal,
            gpu_nodes: BTreeMap::new(),
            active_node: None,
            offload_threshold: 10_000,
            pending_frames: Vec::new(),
            results: Vec::new(),
            next_frame_id: 1,
            stats: ElasticStats::default(),
        }
    }

    /// Register a remote GPU node.
    pub fn add_gpu_node(&mut self, node: GpuNode) {
        self.gpu_nodes.insert(node.id, node);
    }

    /// Update thermal state — triggers mode switch if needed.
    pub fn update_thermal(&mut self, state: ThermalState) {
        let old = self.thermal;
        self.thermal = state;

        if old != state {
            self.stats.thermal_events += 1;
            match state {
                ThermalState::Hot | ThermalState::Critical => {
                    self.escalate_to_remote();
                }
                ThermalState::Normal => {
                    self.fallback_to_local();
                }
                _ => {}
            }
        }
    }

    /// Submit a frame for rendering.
    pub fn submit_frame(&mut self, commands: Vec<RenderCommand>, complexity: u64) -> u64 {
        let frame_id = self.next_frame_id;
        self.next_frame_id += 1;

        let size = commands.len() as u64 * 64; // Estimated serialized size

        let frame = CommandFrame {
            frame_id,
            commands,
            complexity,
            size_bytes: size,
        };

        match self.mode {
            RenderMode::Local => {
                // Render locally; check if we should escalate
                if complexity > self.offload_threshold && self.active_node.is_none() {
                    self.escalate_to_remote();
                }
                self.stats.frames_local += 1;
            }
            RenderMode::Hybrid | RenderMode::Remote => {
                // Queue for remote rendering
                self.stats.commands_streamed += frame.commands.len() as u64;
                self.stats.bytes_uploaded = self.stats.bytes_uploaded.saturating_add(size);
                self.pending_frames.push(frame);
                self.stats.frames_remote += 1;
            }
            RenderMode::Transitioning => {
                // During transition, render locally as fallback
                self.stats.frames_local += 1;
            }
        }

        frame_id
    }

    /// Receive a rendered frame result from remote node.
    pub fn receive_result(&mut self, result: FrameResult) {
        self.stats.bytes_downloaded = self.stats.bytes_downloaded
            .saturating_add(result.data_size);

        // Update average latency (EMA)
        let lat = result.remote_render_us + result.decode_us;
        self.stats.average_remote_latency_us =
            (self.stats.average_remote_latency_us * 7 + lat) / 8;

        self.results.push(result);
    }

    /// Escalate to remote rendering.
    fn escalate_to_remote(&mut self) {
        if self.active_node.is_some() { return; }

        // Find best available GPU node
        let best = self.gpu_nodes.values()
            .filter(|n| !n.active && n.latency_ms < 50)
            .min_by_key(|n| n.latency_ms);

        if let Some(node) = best {
            let node_id = node.id;
            if let Some(n) = self.gpu_nodes.get_mut(&node_id) {
                n.active = true;
            }
            self.active_node = Some(node_id);
            self.mode = RenderMode::Hybrid;
            self.stats.mode_switches += 1;
        }
    }

    /// Fall back to local rendering.
    fn fallback_to_local(&mut self) {
        if let Some(node_id) = self.active_node.take() {
            if let Some(n) = self.gpu_nodes.get_mut(&node_id) {
                n.active = false;
            }
        }
        self.mode = RenderMode::Local;
        self.stats.mode_switches += 1;
    }

    /// Drain pending frames (send to remote).
    pub fn drain_pending(&mut self) -> Vec<CommandFrame> {
        core::mem::take(&mut self.pending_frames)
    }

    /// Drain received results (for local scanout).
    pub fn drain_results(&mut self) -> Vec<FrameResult> {
        core::mem::take(&mut self.results)
    }
}
