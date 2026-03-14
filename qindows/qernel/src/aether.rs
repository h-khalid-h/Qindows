//! # Aether Compositor — GPU-Accelerated Window Manager (Phase 59)
//!
//! Aether is Qindows's kernel-side compositor interface. It manages:
//! - **Window registration and Z-ordering** (Silo → Window mapping)
//! - **Direct-to-Scanout (DtS) rendering**: GPU tiles are composited
//!   directly to the display scanout buffer with no intermediate copy
//! - **Frame submission and vsync scheduling** (GPU FIFO driven)
//! - **Surface damage tracking**: only dirty regions are recomposited
//!
//! ## Architecture
//! ```text
//! Silo A ──AetherSubmit()──→ [Aether Compositor Silo]
//! Silo B ──AetherSubmit()──→ [    Z-Order          ] → GPU → Display
//! Cursor  ─────────────────→ [    Overlay           ]
//! ```
//!
//! The compositor is a **privileged user-mode Silo** with a DEVICE CapToken
//! for the GPU DMA engine. This module is the kernel-side interface:
//! window table, submit validation, and vsync callback scheduling.
//!
//! ## Q-Manifest Law 4: Vector-Native UI
//! All windows submit vector command lists (Q-Kit draw calls), not rasters.
//! The GPU executes the vector shader → raster → scanout pipeline.
//!
//! ## Q-Manifest Law 6: Silo Sandbox
//! One Silo's window can NEVER read another Silo's surface data.
//! GPU memory regions are isolated via IOMMU + per-Silo DMAR tables.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ── Window Types ──────────────────────────────────────────────────────────────

/// A window's visual layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowLayer {
    Desktop     = 0,
    Normal      = 1,
    Floating    = 2,
    Overlay     = 3,
    Notification = 4,
    Cursor      = 5,
}

/// Window visibility state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowVisibility {
    Hidden,
    Minimized,
    Visible,
    Fullscreen,
}

/// A display rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, width: w, height: h }
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        !(self.x + self.width as i32 <= other.x
          || other.x + other.width as i32 <= self.x
          || self.y + self.height as i32 <= other.y
          || other.y + other.height as i32 <= self.y)
    }

    pub fn area(&self) -> u64 {
        self.width as u64 * self.height as u64
    }
}

// ── Window Descriptor ─────────────────────────────────────────────────────────

/// A registered Aether window.
#[derive(Debug, Clone)]
pub struct AetherWindow {
    /// Unique window ID
    pub window_id: u64,
    /// Owning Q-Silo (from Law 6: only owner may submit frames)
    pub owner_silo: u64,
    /// Display title
    pub title: alloc::string::String,
    /// Current geometry
    pub rect: Rect,
    /// Z-order layer
    pub layer: WindowLayer,
    /// Visibility state
    pub visibility: WindowVisibility,
    /// GPU surface buffer (DMA address in VRAM)
    pub surface_dma_addr: u64,
    /// Surface buffer size in bytes
    pub surface_size: u64,
    /// Last frame sequence number rendered
    pub last_frame_seq: u64,
    /// Accumulated damage regions (dirty rects not yet composited)
    pub damage: Vec<Rect>,
    /// Health score fed from Sentinel (100 = healthy, <30 = dim/suspend)
    pub health_score: u8,
    /// Number of frames submitted
    pub frames_submitted: u64,
}

impl AetherWindow {
    pub fn new(window_id: u64, owner_silo: u64, title: &str, rect: Rect, layer: WindowLayer) -> Self {
        AetherWindow {
            window_id,
            owner_silo,
            title: alloc::string::String::from(title),
            rect,
            layer,
            visibility: WindowVisibility::Visible,
            surface_dma_addr: 0,
            surface_size: 0,
            last_frame_seq: 0,
            damage: Vec::new(),
            health_score: 100,
            frames_submitted: 0,
        }
    }

    /// Mark a region as damaged (needs recomposite).
    pub fn mark_damage(&mut self, rect: Rect) {
        // Merge with existing damage rects if they overlap
        for existing in &mut self.damage {
            if existing.intersects(&rect) {
                // Expand existing to bounding box
                let x1 = existing.x.min(rect.x);
                let y1 = existing.y.min(rect.y);
                let x2 = (existing.x + existing.width as i32).max(rect.x + rect.width as i32);
                let y2 = (existing.y + existing.height as i32).max(rect.y + rect.height as i32);
                *existing = Rect::new(x1, y1, (x2-x1) as u32, (y2-y1) as u32);
                return;
            }
        }
        self.damage.push(rect);
    }

    /// Clear all damage after a successful composite pass.
    pub fn clear_damage(&mut self) {
        self.damage.clear();
    }

    pub fn has_damage(&self) -> bool {
        !self.damage.is_empty()
    }
}

// ── Q-Kit Vector Frame ────────────────────────────────────────────────────────

/// A Q-Kit vector draw command.
#[derive(Debug, Clone)]
pub enum QKitCmd {
    /// Fill a rectangle with a solid color (RGBA)
    FillRect { rect: Rect, color: u32 },
    /// Draw rounded rectangle
    RoundRect { rect: Rect, radius: u32, color: u32 },
    /// Draw text (font handled by GPU vector shader)
    DrawText { x: i32, y: i32, text: alloc::string::String, color: u32, size: u32 },
    /// Blit an image (from Prism object OID)
    BlitImage { dst: Rect, src_oid: u64 },
    /// Draw a Bezier path (vector shape)
    DrawPath { points: Vec<(i32, i32)>, stroke_color: u32, fill_color: u32 },
    /// GPU blur / frosted glass effect
    Blur { rect: Rect, radius: u32 },
    /// Present (flush commands to GPU FIFO)
    Present,
}

/// A complete vector frame submitted by a Silo.
#[derive(Debug, Clone)]
pub struct QKitFrame {
    /// Window this frame targets
    pub window_id: u64,
    /// Submitting Silo (must match window owner — checked by Aether)
    pub silo_id: u64,
    /// Frame sequence number
    pub seq: u64,
    /// Draw commands
    pub commands: Vec<QKitCmd>,
    /// Damage hint (Silo knows which rect it changed)
    pub damage_hint: Option<Rect>,
}

// ── Vsync Scheduler ───────────────────────────────────────────────────────────

/// Vsync event record.
#[derive(Debug, Clone, Copy)]
pub struct VsyncEvent {
    /// Frame sequence number
    pub seq: u64,
    /// Kernel tick of the vsync interrupt
    pub tick: u64,
    /// Duration of the previous frame (ticks)
    pub frame_time: u64,
}

// ── Aether Compositor ─────────────────────────────────────────────────────────

/// Compositor statistics.
#[derive(Debug, Default, Clone)]
pub struct AetherStats {
    pub windows_registered: u64,
    pub windows_destroyed: u64,
    pub frames_submitted: u64,
    pub frames_composited: u64,
    pub frames_dropped: u64,
    pub damage_regions_merged: u64,
    pub vsync_events: u64,
}

/// The Aether compositor kernel interface.
///
/// Maintains the window registry, validates frame submissions, drives
/// surface damage tracking, and schedules GPU composite passes on vsync.
pub struct AetherCompositor {
    /// All registered windows: window_id → AetherWindow
    pub windows: BTreeMap<u64, AetherWindow>,
    /// Z-ordered window list (sorted by layer, then registration order)
    pub z_order: Vec<u64>,
    /// Pending frames awaiting GPU submission
    pub pending_frames: Vec<QKitFrame>,
    /// Vsync history (last 8 events)
    pub vsync_history: Vec<VsyncEvent>,
    /// Display resolution
    pub display_width: u32,
    pub display_height: u32,
    /// Frame counter
    pub frame_seq: u64,
    /// Stats
    pub stats: AetherStats,
    /// Next window ID to assign
    next_window_id: u64,
}

impl AetherCompositor {
    pub fn new(width: u32, height: u32) -> Self {
        AetherCompositor {
            windows: BTreeMap::new(),
            z_order: Vec::new(),
            pending_frames: Vec::new(),
            vsync_history: Vec::new(),
            display_width: width,
            display_height: height,
            frame_seq: 0,
            stats: AetherStats::default(),
            next_window_id: 1,
        }
    }

    /// Register a new window for a Silo.
    ///
    /// Called by the `AetherRegister` syscall handler.
    /// Returns the assigned window ID.
    pub fn register_window(
        &mut self,
        owner_silo: u64,
        title: &str,
        rect: Rect,
        layer: WindowLayer,
    ) -> u64 {
        let wid = self.next_window_id;
        self.next_window_id += 1;

        let window = AetherWindow::new(wid, owner_silo, title, rect, layer);
        self.windows.insert(wid, window);
        self.rebuild_z_order();
        self.stats.windows_registered += 1;

        crate::serial_println!(
            "[AETHER] Window {} registered: \"{}\" {}x{} layer={:?} silo={}",
            wid, title, rect.width, rect.height, layer, owner_silo
        );
        wid
    }

    /// Submit a Q-Kit vector frame for compositing.
    ///
    /// ## Q-Manifest Law 6: Silo Sandbox
    /// The submitting Silo MUST own the target window. Any cross-Silo
    /// frame injection is rejected with a Sentinel-reportable fault.
    pub fn submit_frame(&mut self, frame: QKitFrame) -> Result<(), &'static str> {
        let window = self.windows.get_mut(&frame.window_id)
            .ok_or("Aether: window not found")?;

        // Law 6: Silo must own the window
        if window.owner_silo != frame.silo_id {
            crate::serial_println!(
                "[AETHER FAULT] Silo {} attempted to write Silo {}'s window {}",
                frame.silo_id, window.owner_silo, frame.window_id
            );
            return Err("Aether: cross-Silo frame injection blocked (Law VI)");
        }

        // Mark damage from hint or full window rect
        let damage = frame.damage_hint.unwrap_or(window.rect);
        window.mark_damage(damage);
        window.frames_submitted += 1;
        window.last_frame_seq = frame.seq;

        self.stats.frames_submitted += 1;
        self.pending_frames.push(frame);

        Ok(())
    }

    /// Process a vsync interrupt — composite all pending frames.
    ///
    /// In production: dispatch Q-Kit commands to the GPU command ring.
    /// Clears damage regions after successful GPU submission.
    pub fn on_vsync(&mut self, tick: u64) {
        self.frame_seq += 1;
        let prev_time = self.vsync_history.last().map(|e| e.tick).unwrap_or(tick);

        let event = VsyncEvent {
            seq: self.frame_seq,
            tick,
            frame_time: tick.saturating_sub(prev_time),
        };

        if self.vsync_history.len() >= 8 {
            self.vsync_history.remove(0);
        }
        self.vsync_history.push(event);
        self.stats.vsync_events += 1;

        // Composite pending frames
        let count = self.pending_frames.len();
        if count > 0 {
            crate::serial_println!(
                "[AETHER] vsync tick={}: compositing {} frames",
                tick, count
            );

            // Process each frame's damage
            for frame in self.pending_frames.drain(..) {
                if let Some(window) = self.windows.get_mut(&frame.window_id) {
                    window.clear_damage();
                    self.stats.frames_composited += 1;
                }
            }
        }
    }

    /// Update a window's health indicator (from Sentinel).
    ///
    /// When health_score < 30, the window is dimmed (80% opacity)
    /// indicating the Silo is in trouble.
    pub fn set_window_health(&mut self, window_id: u64, health_score: u8) {
        if let Some(window) = self.windows.get_mut(&window_id) {
            window.health_score = health_score;
            if health_score < 30 {
                crate::serial_println!(
                    "[AETHER] Window {} dimmed (health={})",
                    window_id, health_score
                );
            }
        }
    }

    /// Destroy a window (called from Silo vaporize path).
    pub fn destroy_window(&mut self, window_id: u64, silo_id: u64) -> Result<(), &'static str> {
        let window = self.windows.get(&window_id)
            .ok_or("Aether: window not found")?;

        if window.owner_silo != silo_id {
            return Err("Aether: cannot destroy window owned by another Silo");
        }

        self.windows.remove(&window_id);
        self.z_order.retain(|&id| id != window_id);
        self.stats.windows_destroyed += 1;
        crate::serial_println!("[AETHER] Window {} destroyed (Silo {})", window_id, silo_id);
        Ok(())
    }

    /// Returns all windows that have pending damage, in Z-order.
    pub fn dirty_windows(&self) -> Vec<u64> {
        self.z_order.iter()
            .filter(|&&wid| self.windows.get(&wid).map(|w| w.has_damage()).unwrap_or(false))
            .copied()
            .collect()
    }

    // ── Private ────────────────────────────────────────────────────────────

    fn rebuild_z_order(&mut self) {
        self.z_order = self.windows.keys().copied().collect();
        // Sort by layer (ascending = desktop first, cursor last)
        let windows = &self.windows;
        self.z_order.sort_by_key(|id| {
            windows.get(id).map(|w| w.layer as u8).unwrap_or(0)
        });
    }
}
