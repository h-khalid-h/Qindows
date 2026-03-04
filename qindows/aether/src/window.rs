//! # Aether Window Manager
//!
//! Manages the lifecycle of application windows in the scene graph.
//! Each window belongs to a Q-Silo and is rendered independently
//! by the GPU — even if the app freezes, the window is still movable.
//!
//! Features:
//! - Z-order management with 3D depth stacking
//! - Focus tracking and keyboard routing
//! - Window snapping (halves, thirds, quadrants)
//! - Virtual desktops (Q-Spaces)
//! - Animated transitions for all state changes

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Window identifier (globally unique).
pub type WindowId = u64;

/// Window states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowState {
    /// Normal — visible, interactive
    Normal,
    /// Maximized — fills the current Q-Space
    Maximized,
    /// Minimized — hidden but alive
    Minimized,
    /// Snapped to a screen region (left half, top-right quadrant, etc.)
    Snapped(SnapRegion),
    /// Fullscreen — covers everything including taskbar
    Fullscreen,
    /// Being dragged by the user
    Dragging,
    /// Being resized by the user
    Resizing(ResizeEdge),
    /// Animating a state transition
    Transitioning,
    /// Frozen by the Sentinel (unresponsive app)
    Frozen,
}

/// Snap regions for window tiling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapRegion {
    LeftHalf,
    RightHalf,
    TopHalf,
    BottomHalf,
    TopLeftQuadrant,
    TopRightQuadrant,
    BottomLeftQuadrant,
    BottomRightQuadrant,
    LeftThird,
    CenterThird,
    RightThird,
}

/// Resize edges
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeEdge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// A window in the Aether compositor.
pub struct Window {
    /// Unique identifier
    pub id: WindowId,
    /// Owning Silo ID
    pub silo_id: u64,
    /// Human-readable title
    pub title: String,
    /// Current state
    pub state: WindowState,
    /// Position (x, y) in logical pixels
    pub x: f32,
    pub y: f32,
    /// Size (width, height) in logical pixels
    pub width: f32,
    pub height: f32,
    /// Minimum size constraints
    pub min_width: f32,
    pub min_height: f32,
    /// Z-depth (higher = in front)
    pub z_order: u32,
    /// Opacity (0.0 = invisible, 1.0 = fully opaque)
    pub opacity: f32,
    /// Corner radius for the window frame
    pub corner_radius: f32,
    /// Shadow blur radius
    pub shadow_radius: f32,
    /// Which Q-Space this window belongs to
    pub space_id: u32,
    /// Is this window focused (receives keyboard input)?
    pub focused: bool,
    /// Is the application responsive?
    pub responsive: bool,
    /// Saved position/size before maximize/snap (for restore)
    pub saved_rect: Option<(f32, f32, f32, f32)>,
    /// Frame buffer surface ID (GPU texture for this window's content)
    pub surface_id: u64,
}

impl Window {
    /// Create a new window.
    pub fn new(
        id: WindowId,
        silo_id: u64,
        title: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> Self {
        Window {
            id,
            silo_id,
            title,
            state: WindowState::Normal,
            x,
            y,
            width,
            height,
            min_width: 200.0,
            min_height: 150.0,
            z_order: 0,
            opacity: 1.0,
            corner_radius: 12.0,
            shadow_radius: 24.0,
            space_id: 0,
            focused: false,
            responsive: true,
            saved_rect: None,
            surface_id: 0,
        }
    }

    /// Maximize this window to fill the screen.
    pub fn maximize(&mut self, screen_w: f32, screen_h: f32) {
        if self.state != WindowState::Maximized {
            self.saved_rect = Some((self.x, self.y, self.width, self.height));
            self.x = 0.0;
            self.y = 0.0;
            self.width = screen_w;
            self.height = screen_h;
            self.state = WindowState::Maximized;
        }
    }

    /// Restore from maximized/snapped to the previous size.
    pub fn restore(&mut self) {
        if let Some((x, y, w, h)) = self.saved_rect.take() {
            self.x = x;
            self.y = y;
            self.width = w;
            self.height = h;
            self.state = WindowState::Normal;
        }
    }

    /// Snap to a screen region.
    pub fn snap(&mut self, region: SnapRegion, screen_w: f32, screen_h: f32) {
        self.saved_rect = Some((self.x, self.y, self.width, self.height));
        let (x, y, w, h) = match region {
            SnapRegion::LeftHalf => (0.0, 0.0, screen_w / 2.0, screen_h),
            SnapRegion::RightHalf => (screen_w / 2.0, 0.0, screen_w / 2.0, screen_h),
            SnapRegion::TopHalf => (0.0, 0.0, screen_w, screen_h / 2.0),
            SnapRegion::BottomHalf => (0.0, screen_h / 2.0, screen_w, screen_h / 2.0),
            SnapRegion::TopLeftQuadrant => (0.0, 0.0, screen_w / 2.0, screen_h / 2.0),
            SnapRegion::TopRightQuadrant => (screen_w / 2.0, 0.0, screen_w / 2.0, screen_h / 2.0),
            SnapRegion::BottomLeftQuadrant => (0.0, screen_h / 2.0, screen_w / 2.0, screen_h / 2.0),
            SnapRegion::BottomRightQuadrant => (screen_w / 2.0, screen_h / 2.0, screen_w / 2.0, screen_h / 2.0),
            SnapRegion::LeftThird => (0.0, 0.0, screen_w / 3.0, screen_h),
            SnapRegion::CenterThird => (screen_w / 3.0, 0.0, screen_w / 3.0, screen_h),
            SnapRegion::RightThird => (screen_w * 2.0 / 3.0, 0.0, screen_w / 3.0, screen_h),
        };
        self.x = x;
        self.y = y;
        self.width = w;
        self.height = h;
        self.state = WindowState::Snapped(region);
    }

    /// Check if a point is inside this window.
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.width
            && py >= self.y && py <= self.y + self.height
    }
}

/// The Aether Window Manager.
pub struct WindowManager {
    /// All managed windows
    pub windows: Vec<Window>,
    /// Currently focused window
    pub focused_id: Option<WindowId>,
    /// Next available window ID
    next_id: WindowId,
    /// Total number of Q-Spaces (virtual desktops)
    pub num_spaces: u32,
    /// Active Q-Space
    pub active_space: u32,
    /// Screen dimensions
    pub screen_width: f32,
    pub screen_height: f32,
    /// Next z-order value
    next_z: u32,
}

impl WindowManager {
    pub fn new(screen_width: f32, screen_height: f32) -> Self {
        WindowManager {
            windows: Vec::new(),
            focused_id: None,
            next_id: 1,
            num_spaces: 4,
            active_space: 0,
            screen_width,
            screen_height,
            next_z: 1,
        }
    }

    /// Create a new window for a Silo.
    pub fn create_window(
        &mut self,
        silo_id: u64,
        title: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    ) -> WindowId {
        let id = self.next_id;
        self.next_id += 1;

        let mut window = Window::new(id, silo_id, title, x, y, width, height);
        window.z_order = self.next_z;
        self.next_z += 1;
        window.space_id = self.active_space;

        self.windows.push(window);
        self.focus_window(id);
        id
    }

    /// Focus a window — brings it to the front and sends keyboard input to it.
    pub fn focus_window(&mut self, id: WindowId) {
        // Unfocus current
        if let Some(old_id) = self.focused_id {
            if let Some(w) = self.windows.iter_mut().find(|w| w.id == old_id) {
                w.focused = false;
            }
        }

        // Focus new window + bring to front
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == id) {
            w.focused = true;
            w.z_order = self.next_z;
            self.next_z += 1;
            self.focused_id = Some(id);
        }
    }

    /// Close a window.
    pub fn close_window(&mut self, id: WindowId) {
        self.windows.retain(|w| w.id != id);
        if self.focused_id == Some(id) {
            // Focus the topmost remaining window
            self.focused_id = self
                .windows
                .iter()
                .filter(|w| w.space_id == self.active_space)
                .max_by_key(|w| w.z_order)
                .map(|w| w.id);
        }
    }

    /// Close all windows belonging to a Silo (on vaporization).
    pub fn close_silo_windows(&mut self, silo_id: u64) {
        self.windows.retain(|w| w.silo_id != silo_id);
    }

    /// Get the window under a screen point (for mouse click routing).
    pub fn window_at_point(&self, x: f32, y: f32) -> Option<WindowId> {
        self.windows
            .iter()
            .filter(|w| w.space_id == self.active_space && w.state != WindowState::Minimized)
            .filter(|w| w.contains_point(x, y))
            .max_by_key(|w| w.z_order)
            .map(|w| w.id)
    }

    /// Get all visible windows for the current Q-Space, sorted by z-order.
    pub fn visible_windows(&self) -> Vec<&Window> {
        let mut visible: Vec<&Window> = self
            .windows
            .iter()
            .filter(|w| w.space_id == self.active_space && w.state != WindowState::Minimized)
            .collect();
        visible.sort_by_key(|w| w.z_order);
        visible
    }

    /// Switch to a different Q-Space (virtual desktop).
    pub fn switch_space(&mut self, space: u32) {
        if space < self.num_spaces {
            self.active_space = space;
            // Update focus to topmost window in new space
            self.focused_id = self
                .windows
                .iter()
                .filter(|w| w.space_id == space)
                .max_by_key(|w| w.z_order)
                .map(|w| w.id);
        }
    }

    /// Move a window to a different Q-Space.
    pub fn move_to_space(&mut self, window_id: WindowId, space: u32) {
        if let Some(w) = self.windows.iter_mut().find(|w| w.id == window_id) {
            w.space_id = space;
        }
    }

    /// Dim a Silo's windows (Sentinel enforcement).
    pub fn dim_silo(&mut self, silo_id: u64) {
        for window in &mut self.windows {
            if window.silo_id == silo_id {
                window.opacity = 0.5;
                window.responsive = false;
                window.state = WindowState::Frozen;
            }
        }
    }
}
