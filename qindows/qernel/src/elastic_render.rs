//! # Elastic Render — Aether GPU Offload to Q-Server (Phase 81)
//!
//! ARCHITECTURE.md §9 — Nexus: Elastic Rendering:
//! > "Local GPU hits thermal limit → Aether sends **Vector Scene Graph** (not a video) to Q-Server"
//! > "Q-Server renders heavy lighting/ray-trace → returns compressed vertex data"
//! > "Local device still handles final scanout + input prediction → **0ms perceived latency increase**"
//!
//! ## Architecture Guardian: Key design insight
//! Traditional remote desktop sends **pixels** (video stream).
//! Q-Elastic sends a **Vector Scene Graph** — the same SDF math descriptions Aether
//! uses locally. The Q-Server runs the shader computation; the local device handles:
//! - Input prediction (cursor movement, scroll physics) — never feels laggy
//! - Final scanout (the last compositor step)
//! - Window decorations, chrome (not GPU-intensive)
//!
//! This means:
//! - **Bandwidth**: scene graph << pixel video (typically 10× smaller)
//! - **Latency**: Q-Server can start rendering while the graph is still in transit
//! - **Quality**: full ray-trace / global illumination at 4K even on integrated GPU
//!
//! ## Architecture Guardian: Layering
//! ```text
//! ElasticRenderEngine (this module)
//!     │  Concern: WHEN to offload, scene graph encoding, result reassembly
//!     │
//!     ├── GpuThermalSensor: detects thermal limit crossing (→ triggers offload)
//!     ├── SceneGraphExport: serializes Aether's SDF scene to wire format
//!     ├── RenderJob: tracks a remote render request lifecycle
//!     └── ElasticRenderEngine: orchestration
//!
//! NOT responsible for: Nexus peer selection (nexus.rs), Q-Fabric send (qfabric.rs),
//!                      Aether compositor (aether.rs), final scanout (GPU driver)
//! ```

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Thermal State ─────────────────────────────────────────────────────────────

/// GPU thermal state (triggers elastic render decisions).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuThermalState {
    /// Normal operation: render locally
    Cool,
    /// Approaching limit: pre-fetch connect to Q-Server but render locally
    Warm,
    /// At thermal limit: offload new frames to Q-Server
    Hot,
    /// Critical: emergency offload ALL rendering, GPU clock cut
    Critical,
}

impl GpuThermalState {
    pub fn from_temp_millideg(temp: u32) -> Self {
        match temp {
            0..=79_999       => Self::Cool,
            80_000..=89_999  => Self::Warm,
            90_000..=99_999  => Self::Hot,
            _                => Self::Critical,
        }
    }

    pub fn should_offload(self) -> bool {
        matches!(self, Self::Hot | Self::Critical)
    }
}

// ── SDF Node (minimal scene graph element) ────────────────────────────────────

/// A single Signed Distance Function node in the Aether scene graph.
/// This is the unit of the Vector Scene Graph that gets transmitted to Q-Server.
#[derive(Debug, Clone)]
pub struct SdfNode {
    /// Unique node ID in the scene
    pub node_id: u32,
    /// SDF primitive type: "rounded_rect", "circle", "text_glyph", "glass_pane", etc.
    pub primitive: String,
    /// Parameters (varies by primitive): [x, y, width, height, radius, ...]
    pub params: [f32; 8],
    /// RGBA color (f32 0-1)
    pub color: [f32; 4],
    /// Material properties: [reflectance, roughness, refraction_ior, emission]
    pub material: [f32; 4],
    /// Z-order depth
    pub z_order: i32,
    /// Parent node ID (0 = root)
    pub parent_id: u32,
    /// Clip region (for overflow:hidden etc.)
    pub clip: Option<[f32; 4]>,
    /// Is this node GPU-heavy? (enables offload decision per node, not whole frame)
    pub gpu_heavy: bool,
}

/// An Aether scene graph ready for export.
#[derive(Debug, Clone, Default)]
pub struct SceneGraph {
    pub nodes: Vec<SdfNode>,
    /// Viewport: [x, y, width, height]
    pub viewport: [f32; 4],
    /// Frame sequence number
    pub frame_seq: u64,
    /// Timestamp of this frame (kernel tick)
    pub frame_tick: u64,
    /// Render quality hint for Q-Server (0=fast, 100=max quality)
    pub quality_hint: u8,
    /// Does this frame have ray-trace-heavy nodes?
    pub has_ray_trace: bool,
}

impl SceneGraph {
    /// Approximate wire size (for Q-Fabric bandwidth planning).
    pub fn wire_size_bytes(&self) -> usize {
        self.nodes.len() * (4 + 32 + 8*4 + 4*4 + 4*4 + 4 + 4) + 64
    }

    /// Count GPU-heavy nodes that should definitely be server-rendered.
    pub fn heavy_node_count(&self) -> usize {
        self.nodes.iter().filter(|n| n.gpu_heavy).count()
    }
}

// ── Render Result ─────────────────────────────────────────────────────────────

/// The result returned by the Q-Server after rendering.
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// Which frame this corresponds to
    pub frame_seq: u64,
    /// Rendered pixels as compressed vertex/tile data (Zstd-compressed)
    /// In production: might be GPU-texture-compressed (BC7/ASTC)
    pub compressed_pixels: Vec<u8>,
    /// Width and height of rendered output
    pub width: u32,
    pub height: u32,
    /// Was ray-tracing used?
    pub ray_traced: bool,
    /// Q-Server render time (microseconds)
    pub render_us: u32,
    /// Bytes transmitted (compressed) vs uncompressed equivalent
    pub compression_ratio: f32,
}

// ── Render Job ────────────────────────────────────────────────────────────────

/// Lifecycle phase of an elastic render job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderJobPhase {
    Encoding,      // packing scene graph for transmission
    Transmitting,  // sent to Q-Server, waiting for ack
    Rendering,     // Q-Server is executing the shaders
    Receiving,     // result coming back over Q-Fabric
    Complete,      // result received, ready for local compositor
    Failed,        // timeout or Q-Server error — fall back to local render
}

#[derive(Debug, Clone)]
pub struct RenderJob {
    pub job_id: u64,
    pub frame_seq: u64,
    pub scene: SceneGraph,
    pub phase: RenderJobPhase,
    pub server_node_id: u64,
    pub submitted_at: u64,
    pub result: Option<RenderResult>,
    pub fallback_rendered_locally: bool,
}

// ── Elastic Render Statistics ─────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ElasticRenderStats {
    pub frames_rendered_local: u64,
    pub frames_offloaded: u64,
    pub frames_failed_fallback: u64,
    pub avg_remote_render_us: u64,
    pub avg_scene_wire_bytes: u64,
    pub total_bytes_transmitted: u64,
    pub gpu_thermal_trips: u64,   // how many times thermal limit was crossed
    pub quality_upgrades: u64,    // frames that got ray-traced on Q-Server
}

// ── Elastic Render Engine ─────────────────────────────────────────────────────

/// Aether's GPU elastic rendering coordinator.
pub struct ElasticRenderEngine {
    /// Current GPU thermal state
    pub gpu_state: GpuThermalState,
    /// GPU temperature (millidegrees Celsius)
    pub gpu_temp_millideg: u32,
    /// Active render jobs: job_id → job
    pub active_jobs: BTreeMap<u64, RenderJob>,
    /// Completed jobs (last 32 frames)
    pub completed: Vec<RenderJob>,
    pub max_completed: usize,
    /// Statistics
    pub stats: ElasticRenderStats,
    /// Next job ID
    next_job_id: u64,
    /// Preferred Q-Server node (from last Compute Auction bid win)
    pub preferred_server: Option<u64>,
    /// Maximum frames in-flight on Q-Server simultaneously
    pub max_inflight: usize,
}

impl ElasticRenderEngine {
    pub fn new() -> Self {
        ElasticRenderEngine {
            gpu_state: GpuThermalState::Cool,
            gpu_temp_millideg: 60_000,
            active_jobs: BTreeMap::new(),
            completed: Vec::new(),
            max_completed: 32,
            stats: ElasticRenderStats::default(),
            next_job_id: 1,
            preferred_server: None,
            max_inflight: 3,
        }
    }

    /// Update GPU temperature (called from thermal.rs monitor every 100ms).
    pub fn update_thermal(&mut self, temp_millideg: u32) {
        let prev = self.gpu_state;
        self.gpu_temp_millideg = temp_millideg;
        self.gpu_state = GpuThermalState::from_temp_millideg(temp_millideg);
        if prev == GpuThermalState::Cool && self.gpu_state.should_offload() {
            self.stats.gpu_thermal_trips += 1;
            crate::serial_println!(
                "[ELASTIC] GPU thermal trip: {}°C → state={:?}. Starting elastic render.",
                temp_millideg / 1000, self.gpu_state
            );
        }
    }

    /// Should this frame be offloaded? Decision point called by Aether per vsync.
    pub fn should_offload(&self, scene: &SceneGraph) -> bool {
        if self.active_jobs.len() >= self.max_inflight { return false; } // too many in-flight
        if self.preferred_server.is_none() { return false; }             // no server connected

        match self.gpu_state {
            GpuThermalState::Hot | GpuThermalState::Critical => true,
            GpuThermalState::Warm => scene.has_ray_trace || scene.heavy_node_count() > 10,
            GpuThermalState::Cool => false,
        }
    }

    /// Submit a scene graph for remote rendering. Returns job_id.
    pub fn submit_scene(&mut self, scene: SceneGraph, server_node_id: u64, tick: u64) -> u64 {
        let job_id = self.next_job_id;
        self.next_job_id += 1;
        let wire_sz = scene.wire_size_bytes() as u64;
        let frame_seq = scene.frame_seq;

        crate::serial_println!(
            "[ELASTIC] Submitting frame {} to node {:016x}: {} nodes, {}B wire, ray_trace={}",
            frame_seq, server_node_id, scene.nodes.len(), wire_sz, scene.has_ray_trace
        );

        let job = RenderJob {
            job_id,
            frame_seq,
            scene,
            phase: RenderJobPhase::Transmitting,
            server_node_id,
            submitted_at: tick,
            result: None,
            fallback_rendered_locally: false,
        };

        self.active_jobs.insert(job_id, job);
        self.stats.frames_offloaded += 1;
        self.stats.total_bytes_transmitted += wire_sz;
        self.stats.avg_scene_wire_bytes =
            self.stats.total_bytes_transmitted / self.stats.frames_offloaded;

        job_id
    }

    /// Q-Fabric callback: render result received from Q-Server.
    pub fn receive_result(&mut self, job_id: u64, result: RenderResult, tick: u64) -> bool {
        if let Some(mut job) = self.active_jobs.remove(&job_id) {
            crate::serial_println!(
                "[ELASTIC] Frame {} result: {}×{} {}B compressed (ratio {:.1}×) {}μs render",
                result.frame_seq, result.width, result.height,
                result.compressed_pixels.len(), result.compression_ratio, result.render_us
            );
            // Update stats
            let total = self.stats.frames_offloaded;
            self.stats.avg_remote_render_us =
                (self.stats.avg_remote_render_us * (total-1) + result.render_us as u64) / total;
            if result.ray_traced { self.stats.quality_upgrades += 1; }
            job.result = Some(result);
            job.phase = RenderJobPhase::Complete;
            if self.completed.len() >= self.max_completed { self.completed.remove(0); }
            self.completed.push(job);
            let _ = tick;
            true
        } else {
            false
        }
    }

    /// Timeout: Q-Server didn't respond — fall back to local render.
    pub fn handle_timeout(&mut self, job_id: u64, tick: u64) {
        if let Some(mut job) = self.active_jobs.remove(&job_id) {
            crate::serial_println!(
                "[ELASTIC] Frame {} timeout at tick {} — falling back to local render.",
                job.frame_seq, tick
            );
            job.fallback_rendered_locally = true;
            job.phase = RenderJobPhase::Failed;
            self.stats.frames_failed_fallback += 1;
            self.stats.frames_rendered_local += 1;
            if self.completed.len() >= self.max_completed { self.completed.remove(0); }
            self.completed.push(job);
        }
    }

    /// Create a synthetic demo scene for testing.
    pub fn demo_scene(frame_seq: u64, tick: u64) -> SceneGraph {
        let mut scene = SceneGraph {
            viewport: [0.0, 0.0, 2560.0, 1440.0],
            frame_seq,
            frame_tick: tick,
            quality_hint: 85,
            has_ray_trace: true,
            ..Default::default()
        };
        // Glass background
        scene.nodes.push(SdfNode {
            node_id: 1,
            primitive: "glass_pane".to_string(),
            params: [0.0, 0.0, 2560.0, 1440.0, 0.0, 0.0, 0.0, 0.0],
            color: [0.1, 0.1, 0.15, 0.85],
            material: [0.9, 0.1, 1.45, 0.0],
            z_order: 0,
            parent_id: 0,
            clip: None,
            gpu_heavy: true, // glass refraction is GPU-heavy
        });
        // Window chrome
        scene.nodes.push(SdfNode {
            node_id: 2,
            primitive: "rounded_rect".to_string(),
            params: [100.0, 100.0, 800.0, 600.0, 16.0, 0.0, 0.0, 0.0],
            color: [0.2, 0.2, 0.25, 0.95],
            material: [0.3, 0.7, 0.0, 0.0],
            z_order: 10,
            parent_id: 1,
            clip: None,
            gpu_heavy: false,
        });
        scene
    }
}
