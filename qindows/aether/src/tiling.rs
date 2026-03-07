//! # Aether Window Tiling Manager
//!
//! Automatic and manual window tiling for the Qindows desktop.
//! Supports grid layouts, splits, zones, and snap-to-edge.

extern crate alloc;

use alloc::string::String;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::vec::Vec;

/// A rectangle (screen coordinates).
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, width: w, height: h }
    }

    /// Split horizontally into left and right at ratio (0.0-1.0).
    pub fn split_h(&self, ratio: f32) -> (Rect, Rect) {
        let w1 = self.width * ratio;
        (
            Rect::new(self.x, self.y, w1, self.height),
            Rect::new(self.x + w1, self.y, self.width - w1, self.height),
        )
    }

    /// Split vertically into top and bottom at ratio.
    pub fn split_v(&self, ratio: f32) -> (Rect, Rect) {
        let h1 = self.height * ratio;
        (
            Rect::new(self.x, self.y, self.width, h1),
            Rect::new(self.x, self.y + h1, self.width, self.height - h1),
        )
    }

    /// Check if a point is inside.
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.width
            && py >= self.y && py < self.y + self.height
    }

    /// Apply gap/margin inset.
    pub fn inset(&self, gap: f32) -> Rect {
        Rect::new(
            self.x + gap,
            self.y + gap,
            (self.width - 2.0 * gap).max(0.0),
            (self.height - 2.0 * gap).max(0.0),
        )
    }
}

/// Tiling layout modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TilingMode {
    /// No tiling (floating)
    Float,
    /// Master + stack (like i3/sway)
    MasterStack,
    /// Equal grid
    Grid,
    /// Horizontal splits only
    Columns,
    /// Vertical splits only
    Rows,
    /// Spiral (fibonacci-like)
    Spiral,
    /// User-defined zones
    Zones,
}

/// A snap edge for drag-to-snap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapEdge {
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Maximize,
}

impl SnapEdge {
    /// Calculate the snapped rectangle for a given screen area.
    pub fn snap_rect(&self, screen: &Rect) -> Rect {
        match self {
            SnapEdge::Left => Rect::new(screen.x, screen.y, screen.width / 2.0, screen.height),
            SnapEdge::Right => Rect::new(screen.x + screen.width / 2.0, screen.y, screen.width / 2.0, screen.height),
            SnapEdge::Top => Rect::new(screen.x, screen.y, screen.width, screen.height / 2.0),
            SnapEdge::Bottom => Rect::new(screen.x, screen.y + screen.height / 2.0, screen.width, screen.height / 2.0),
            SnapEdge::TopLeft => Rect::new(screen.x, screen.y, screen.width / 2.0, screen.height / 2.0),
            SnapEdge::TopRight => Rect::new(screen.x + screen.width / 2.0, screen.y, screen.width / 2.0, screen.height / 2.0),
            SnapEdge::BottomLeft => Rect::new(screen.x, screen.y + screen.height / 2.0, screen.width / 2.0, screen.height / 2.0),
            SnapEdge::BottomRight => Rect::new(screen.x + screen.width / 2.0, screen.y + screen.height / 2.0, screen.width / 2.0, screen.height / 2.0),
            SnapEdge::Maximize => *screen,
        }
    }
}

/// A tiled window entry.
#[derive(Debug, Clone)]
pub struct TiledWindow {
    /// Window/widget ID
    pub window_id: u64,
    /// Calculated tile rectangle
    pub rect: Rect,
    /// Is this window the master?
    pub is_master: bool,
    /// User-forced floating override?
    pub force_float: bool,
}

/// User-defined tiling zone.
#[derive(Debug, Clone)]
pub struct Zone {
    pub name: String,
    pub rect: Rect,
}

/// The Tiling Manager.
pub struct TilingManager {
    /// Current tiling mode
    pub mode: TilingMode,
    /// Screen work area (usable area minus taskbar)
    pub work_area: Rect,
    /// Managed windows
    pub windows: Vec<TiledWindow>,
    /// Gap between tiles (pixels)
    pub gap: f32,
    /// Master area ratio (for MasterStack)
    pub master_ratio: f32,
    /// User-defined zones (for Zones mode)
    pub zones: Vec<Zone>,
    /// Active snap preview
    pub snap_preview: Option<Rect>,
}

impl TilingManager {
    pub fn new(screen_width: f32, screen_height: f32, taskbar_height: f32) -> Self {
        TilingManager {
            mode: TilingMode::MasterStack,
            work_area: Rect::new(0.0, 0.0, screen_width, screen_height - taskbar_height),
            windows: Vec::new(),
            gap: 8.0,
            master_ratio: 0.55,
            zones: Vec::new(),
            snap_preview: None,
        }
    }

    /// Add a window to the tiling layout.
    pub fn add_window(&mut self, window_id: u64) {
        self.windows.push(TiledWindow {
            window_id,
            rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            is_master: self.windows.is_empty(),
            force_float: false,
        });
        self.retile();
    }

    /// Remove a window.
    pub fn remove_window(&mut self, window_id: u64) {
        self.windows.retain(|w| w.window_id != window_id);
        // Reassign master if needed
        if !self.windows.is_empty() && !self.windows.iter().any(|w| w.is_master) {
            self.windows[0].is_master = true;
        }
        self.retile();
    }

    /// Swap two windows.
    pub fn swap_windows(&mut self, a: u64, b: u64) {
        let pos_a = self.windows.iter().position(|w| w.window_id == a);
        let pos_b = self.windows.iter().position(|w| w.window_id == b);
        if let (Some(i), Some(j)) = (pos_a, pos_b) {
            self.windows.swap(i, j);
            self.retile();
        }
    }

    /// Promote a window to master.
    pub fn promote_to_master(&mut self, window_id: u64) {
        for w in &mut self.windows {
            w.is_master = w.window_id == window_id;
        }
        // Move master to front
        if let Some(pos) = self.windows.iter().position(|w| w.window_id == window_id) {
            let win = self.windows.remove(pos);
            self.windows.insert(0, win);
        }
        self.retile();
    }

    /// Recalculate all tile positions.
    pub fn retile(&mut self) {
        let tiled: Vec<usize> = self.windows.iter().enumerate()
            .filter(|(_, w)| !w.force_float)
            .map(|(i, _)| i)
            .collect();

        let count = tiled.len();
        if count == 0 { return; }

        let area = self.work_area.inset(self.gap);

        match self.mode {
            TilingMode::Float => {} // No tiling

            TilingMode::MasterStack => {
                if count == 1 {
                    self.windows[tiled[0]].rect = area;
                } else {
                    let (master_area, stack_area) = area.split_h(self.master_ratio);
                    self.windows[tiled[0]].rect = master_area.inset(self.gap / 2.0);

                    let stack_count = count - 1;
                    let stack_h = stack_area.height / stack_count as f32;
                    for (i, &idx) in tiled[1..].iter().enumerate() {
                        self.windows[idx].rect = Rect::new(
                            stack_area.x + self.gap / 2.0,
                            stack_area.y + i as f32 * stack_h + self.gap / 2.0,
                            stack_area.width - self.gap,
                            stack_h - self.gap,
                        );
                    }
                }
            }

            TilingMode::Grid => {
                let cols = (count as f32).sqrt().ceil() as usize;
                let rows = (count + cols - 1) / cols;
                let cell_w = area.width / cols as f32;
                let cell_h = area.height / rows as f32;

                for (i, &idx) in tiled.iter().enumerate() {
                    let col = i % cols;
                    let row = i / cols;
                    self.windows[idx].rect = Rect::new(
                        area.x + col as f32 * cell_w + self.gap / 2.0,
                        area.y + row as f32 * cell_h + self.gap / 2.0,
                        cell_w - self.gap,
                        cell_h - self.gap,
                    );
                }
            }

            TilingMode::Columns => {
                let col_w = area.width / count as f32;
                for (i, &idx) in tiled.iter().enumerate() {
                    self.windows[idx].rect = Rect::new(
                        area.x + i as f32 * col_w + self.gap / 2.0,
                        area.y + self.gap / 2.0,
                        col_w - self.gap,
                        area.height - self.gap,
                    );
                }
            }

            TilingMode::Rows => {
                let row_h = area.height / count as f32;
                for (i, &idx) in tiled.iter().enumerate() {
                    self.windows[idx].rect = Rect::new(
                        area.x + self.gap / 2.0,
                        area.y + i as f32 * row_h + self.gap / 2.0,
                        area.width - self.gap,
                        row_h - self.gap,
                    );
                }
            }

            TilingMode::Spiral => {
                let mut remaining = area;
                for (i, &idx) in tiled.iter().enumerate() {
                    if i == count - 1 {
                        self.windows[idx].rect = remaining.inset(self.gap / 2.0);
                    } else if i % 2 == 0 {
                        let (left, right) = remaining.split_h(0.5);
                        self.windows[idx].rect = left.inset(self.gap / 2.0);
                        remaining = right;
                    } else {
                        let (top, bottom) = remaining.split_v(0.5);
                        self.windows[idx].rect = top.inset(self.gap / 2.0);
                        remaining = bottom;
                    }
                }
            }

            TilingMode::Zones => {
                for (i, &idx) in tiled.iter().enumerate() {
                    if let Some(zone) = self.zones.get(i) {
                        self.windows[idx].rect = zone.rect.inset(self.gap / 2.0);
                    }
                }
            }
        }
    }

    /// Detect snap edge from cursor position near screen edges.
    pub fn detect_snap(&self, x: f32, y: f32, threshold: f32) -> Option<SnapEdge> {
        let s = &self.work_area;
        let near_left = x < s.x + threshold;
        let near_right = x > s.x + s.width - threshold;
        let near_top = y < s.y + threshold;
        let near_bottom = y > s.y + s.height - threshold;

        match (near_left, near_right, near_top, near_bottom) {
            (true, _, true, _) => Some(SnapEdge::TopLeft),
            (true, _, _, true) => Some(SnapEdge::BottomLeft),
            (_, true, true, _) => Some(SnapEdge::TopRight),
            (_, true, _, true) => Some(SnapEdge::BottomRight),
            (true, _, _, _) => Some(SnapEdge::Left),
            (_, true, _, _) => Some(SnapEdge::Right),
            (_, _, true, _) => Some(SnapEdge::Top),
            (_, _, _, true) => Some(SnapEdge::Bottom),
            _ => None,
        }
    }

    /// Set tiling mode and retile.
    pub fn set_mode(&mut self, mode: TilingMode) {
        self.mode = mode;
        self.retile();
    }

    /// Toggle a window's floating state.
    pub fn toggle_float(&mut self, window_id: u64) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.window_id == window_id) {
            w.force_float = !w.force_float;
        }
        self.retile();
    }
}
