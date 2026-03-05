//! # Aether Q-Sync — Independent Per-Window Refresh Rates
//!
//! The Aether compositor supports independent refresh rates per
//! window surface (Section 4.1 of the Qindows Spec):
//!
//! - A 144Hz video refreshes independently
//! - An adjacent static document sits at 0Hz (no GPU cost)
//! - The cursor always runs at max display refresh
//!
//! Q-Sync uses fence-based synchronization: apps send a fencing
//! signal, and the GPU display controller reads directly from app
//! memory (Direct-to-Scanout, < 2ms "Zero-Lag" rendering).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Refresh rate tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RefreshTier {
    /// Static content — no refresh needed (0 Hz effective)
    Static,
    /// Low power — 15 Hz (clocks, idle status)
    LowPower,
    /// Standard — 30 Hz (text editors, file browsers)
    Standard,
    /// Smooth — 60 Hz (general UI, web browsing)
    Smooth,
    /// High — 120 Hz (scrolling, animations)
    High,
    /// Ultra — 144 Hz (gaming, video playback)
    Ultra,
    /// Max — match display native refresh (240+ Hz)
    DisplayNative,
}

impl RefreshTier {
    /// Target frames per second for this tier.
    pub fn fps(&self) -> u32 {
        match self {
            RefreshTier::Static => 0,
            RefreshTier::LowPower => 15,
            RefreshTier::Standard => 30,
            RefreshTier::Smooth => 60,
            RefreshTier::High => 120,
            RefreshTier::Ultra => 144,
            RefreshTier::DisplayNative => 240,
        }
    }

    /// Frame budget in microseconds (0 = no deadline).
    pub fn frame_budget_us(&self) -> u64 {
        let fps = self.fps();
        if fps == 0 { return u64::MAX; }
        1_000_000 / fps as u64
    }
}

/// A GPU fence — signals completion of a frame.
#[derive(Debug, Clone, Copy)]
pub struct GpuFence {
    /// Fence ID
    pub id: u64,
    /// Is the fence signaled (frame ready for scanout)?
    pub signaled: bool,
    /// Frame timestamp
    pub timestamp: u64,
}

/// A surface — a window's render target.
#[derive(Debug, Clone)]
pub struct Surface {
    /// Surface ID (matches window ID)
    pub id: u64,
    /// Surface name (for debugging)
    pub name: String,
    /// Current refresh tier
    pub tier: RefreshTier,
    /// Actual measured FPS
    pub actual_fps: f32,
    /// Frame counter
    pub frame_count: u64,
    /// Last frame presentation timestamp
    pub last_present: u64,
    /// Pending fence (app has submitted a new frame)
    pub pending_fence: Option<GpuFence>,
    /// GPU buffer address (for Direct-to-Scanout)
    pub buffer_addr: u64,
    /// Buffer size (width × height × 4 bytes)
    pub buffer_size: u64,
    /// Is this surface focused (foreground)?
    pub focused: bool,
    /// Is this surface in a visible region?
    pub visible: bool,
    /// Content has changed since last scanout?
    pub dirty: bool,
    /// Accumulated frame times for FPS calculation (last N frames)
    frame_times: Vec<u64>,
}

impl Surface {
    pub fn new(id: u64, name: &str, buffer_addr: u64, buffer_size: u64) -> Self {
        Surface {
            id,
            name: String::from(name),
            tier: RefreshTier::Smooth,
            actual_fps: 0.0,
            frame_count: 0,
            last_present: 0,
            pending_fence: None,
            buffer_addr,
            buffer_size,
            focused: false,
            visible: true,
            dirty: false,
            frame_times: Vec::with_capacity(60),
        }
    }

    /// Submit a new frame (app signals the fence).
    pub fn present(&mut self, fence_id: u64, now: u64) {
        self.pending_fence = Some(GpuFence {
            id: fence_id,
            signaled: true,
            timestamp: now,
        });
        self.dirty = true;

        // Track frame time
        if self.last_present > 0 {
            let dt = now.saturating_sub(self.last_present);
            self.frame_times.push(dt);
            if self.frame_times.len() > 60 {
                self.frame_times.remove(0);
            }
            // Calculate rolling average FPS
            if !self.frame_times.is_empty() {
                let avg_us: u64 = self.frame_times.iter().sum::<u64>()
                    / self.frame_times.len() as u64;
                self.actual_fps = if avg_us > 0 {
                    1_000_000.0 / avg_us as f32
                } else {
                    0.0
                };
            }
        }

        self.last_present = now;
        self.frame_count += 1;
    }

    /// Should this surface be composited this tick?
    pub fn needs_composite(&self, now: u64) -> bool {
        if !self.visible { return false; }
        if self.tier == RefreshTier::Static && !self.dirty { return false; }

        let elapsed = now.saturating_sub(self.last_present);
        elapsed >= self.tier.frame_budget_us()
    }
}

/// Asynchronous Timewarp state (Section 4.1).
/// Shifts the desktop image based on mouse micro-movements
/// to hide hardware sensor latency.
#[derive(Debug, Clone)]
pub struct AsyncTimewarp {
    /// Is timewarp active?
    pub enabled: bool,
    /// Latest mouse delta X (sub-pixel)
    pub mouse_dx: f32,
    /// Latest mouse delta Y (sub-pixel)
    pub mouse_dy: f32,
    /// Timewarp shift applied (pixels)
    pub shift_x: f32,
    pub shift_y: f32,
    /// Prediction factor (how much to extrapolate)
    pub prediction_factor: f32,
}

impl AsyncTimewarp {
    pub fn new() -> Self {
        AsyncTimewarp {
            enabled: true,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            shift_x: 0.0,
            shift_y: 0.0,
            prediction_factor: 1.5,
        }
    }

    /// Update with raw mouse delta (from high-priority input fiber).
    pub fn update_mouse(&mut self, dx: f32, dy: f32) {
        self.mouse_dx = dx;
        self.mouse_dy = dy;
        if self.enabled {
            self.shift_x = dx * self.prediction_factor;
            self.shift_y = dy * self.prediction_factor;
        }
    }

    /// Get the timewarp correction to apply to the framebuffer.
    pub fn get_correction(&self) -> (f32, f32) {
        if self.enabled {
            (self.shift_x, self.shift_y)
        } else {
            (0.0, 0.0)
        }
    }

    /// Reset after frame is composited with new data.
    pub fn reset(&mut self) {
        self.shift_x = 0.0;
        self.shift_y = 0.0;
    }
}

/// Q-Sync statistics.
#[derive(Debug, Clone, Default)]
pub struct QSyncStats {
    pub frames_composited: u64,
    pub frames_skipped: u64,  // Static surfaces that didn't need update
    pub scanouts: u64,
    pub timewarp_corrections: u64,
    pub gpu_power_saved_pct: f32,
}

/// The Q-Sync Compositor Extension.
pub struct QSync {
    /// All registered surfaces
    pub surfaces: BTreeMap<u64, Surface>,
    /// Display native refresh rate (Hz)
    pub display_refresh: u32,
    /// Async timewarp engine
    pub timewarp: AsyncTimewarp,
    /// Statistics
    pub stats: QSyncStats,
}

impl QSync {
    pub fn new(display_refresh: u32) -> Self {
        QSync {
            surfaces: BTreeMap::new(),
            display_refresh,
            timewarp: AsyncTimewarp::new(),
            stats: QSyncStats::default(),
        }
    }

    /// Register a surface.
    pub fn add_surface(&mut self, id: u64, name: &str, buffer_addr: u64, buffer_size: u64) {
        self.surfaces.insert(id, Surface::new(id, name, buffer_addr, buffer_size));
    }

    /// Remove a surface.
    pub fn remove_surface(&mut self, id: u64) {
        self.surfaces.remove(&id);
    }

    /// Set a surface's refresh tier.
    pub fn set_tier(&mut self, id: u64, tier: RefreshTier) {
        if let Some(surface) = self.surfaces.get_mut(&id) {
            surface.tier = tier;
        }
    }

    /// Auto-tune tier based on content activity.
    pub fn auto_tune(&mut self, now: u64) {
        for surface in self.surfaces.values_mut() {
            if !surface.visible {
                surface.tier = RefreshTier::Static;
                continue;
            }

            // Promote focused surfaces to at least Smooth
            if surface.focused && surface.tier < RefreshTier::Smooth {
                surface.tier = RefreshTier::Smooth;
            }

            // If no frames in 2 seconds, demote to Static
            if now.saturating_sub(surface.last_present) > 2_000_000 {
                surface.tier = RefreshTier::Static;
                surface.dirty = false;
            }
        }
    }

    /// Composite tick — determine which surfaces need rendering.
    pub fn composite_tick(&mut self, now: u64) -> Vec<u64> {
        let mut to_composite = Vec::new();

        for surface in self.surfaces.values_mut() {
            if surface.needs_composite(now) {
                to_composite.push(surface.id);
                surface.dirty = false;
                self.stats.frames_composited += 1;
            } else {
                self.stats.frames_skipped += 1;
            }
        }

        // Apply timewarp correction if compositing
        if !to_composite.is_empty() {
            let (tx, ty) = self.timewarp.get_correction();
            if tx.abs() > 0.01 || ty.abs() > 0.01 {
                self.stats.timewarp_corrections += 1;
            }
            self.timewarp.reset();
        }

        self.stats.scanouts += 1;
        to_composite
    }

    /// Calculate GPU power savings from Q-Sync.
    pub fn power_savings(&self) -> f32 {
        if self.surfaces.is_empty() { return 0.0; }

        let max_cost = self.surfaces.len() as f32 * self.display_refresh as f32;
        let actual_cost: f32 = self.surfaces.values()
            .map(|s| s.tier.fps() as f32)
            .sum();

        if max_cost > 0.0 {
            ((max_cost - actual_cost) / max_cost * 100.0).max(0.0)
        } else {
            0.0
        }
    }
}
