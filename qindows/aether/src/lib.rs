//! # Aether — Vector Compositor Engine
//!
//! Aether discards pixel-based bitmaps for GPU-accelerated
//! Signed Distance Fields (SDF). Every button, icon, and font is
//! a mathematical formula executed on the GPU.
//!
//! Features:
//! - **Zero-Copy Scanout**: App frames map directly to display
//! - **SDF Rendering**: Infinite resolution scaling
//! - **Q-Glass Material**: Real-time ray-traced transparency
//! - **Scene Graph**: UI independent of app state

#![no_std]

extern crate alloc;

pub mod accessibility;
pub mod animation;
pub mod a11y;
pub mod animations;
pub mod clipboard;
pub mod clipboard_mgr;
pub mod context_menu;
pub mod dnd;
pub mod font;
pub mod file_picker;
pub mod input;
pub mod layout;
pub mod lockscreen;
pub mod notifications;
pub mod notif_center;
pub mod renderer;
pub mod taskbar;
pub mod theme;
pub mod theme_engine;
pub mod themes;
pub mod tiling;
pub mod widget_kit;
pub mod widgets;
pub mod window;

use alloc::vec::Vec;

/// 2D vector
#[derive(Debug, Clone, Copy, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

/// A rectangle in screen space.
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Material types for Q-Glass rendering.
#[derive(Debug, Clone, Copy)]
pub enum Material {
    /// Transparent blur effect (like frosted glass)
    Glass { blur_radius: f32, tint: u32 },
    /// Metallic surface with reflections
    Acrylic,
    /// Simple solid color
    Solid(u32),
    /// Linear gradient between two colors
    Gradient { from: u32, to: u32 },
}

/// A vector path — the fundamental drawing primitive.
///
/// UI elements are NOT pixel bitmaps. They are mathematical
/// paths that the GPU renders at any resolution.
#[derive(Debug, Clone)]
pub struct QPath {
    /// Mathematical control points
    pub points: Vec<Vec2>,
    /// Fill material
    pub fill: Material,
    /// Stroke width (0 = no stroke)
    pub stroke_width: f32,
    /// Corner radius (for rounded rects)
    pub corner_radius: f32,
    /// Blur radius (for shadow/glass effects)
    pub blur_radius: f32,
}

impl QPath {
    /// Create a rounded rectangle path.
    pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Self {
        QPath {
            points: alloc::vec![
                Vec2 { x, y },
                Vec2 { x: x + w, y: y + h },
            ],
            fill: Material::Solid(0xFFFFFFFF),
            stroke_width: 0.0,
            corner_radius: 0.0,
            blur_radius: 0.0,
        }
    }

    pub fn with_corner_radius(mut self, r: f32) -> Self {
        self.corner_radius = r;
        self
    }

    pub fn with_material(mut self, m: Material) -> Self {
        self.fill = m;
        self
    }
}

/// A node in the Aether scene graph.
///
/// The compositor maintains a tree of SceneNodes.
/// Even if an app freezes, the GPU can still move/resize
/// the window at 144Hz using the scene graph.
#[derive(Debug)]
pub struct SceneNode {
    /// Silo that owns this node (for isolation)
    pub silo_id: u64,
    /// Position in the 3D scene (x, y, z-depth)
    pub position: (f32, f32, f32),
    /// Size in logical pixels
    pub size: (f32, f32),
    /// Opacity (0.0 = invisible, 1.0 = opaque)
    pub opacity: f32,
    /// Visual elements in this node
    pub paths: Vec<QPath>,
    /// Whether this node's Silo is responsive
    pub responsive: bool,
}

/// The Aether Compositor — manages the global scene graph.
pub struct Compositor {
    pub nodes: Vec<SceneNode>,
}

impl Compositor {
    pub const fn new() -> Self {
        Compositor { nodes: Vec::new() }
    }

    /// Register a new scene node for a Silo.
    pub fn register(&mut self, node: SceneNode) -> usize {
        self.nodes.push(node);
        self.nodes.len() - 1
    }

    /// Dim a window (used by Sentinel when an app becomes unresponsive).
    pub fn dim_window(&mut self, silo_id: u64) {
        for node in &mut self.nodes {
            if node.silo_id == silo_id {
                node.opacity = 0.5;
                node.responsive = false;
            }
        }
    }

    /// Remove all scene nodes for a vaporized Silo.
    pub fn remove_silo(&mut self, silo_id: u64) {
        self.nodes.retain(|n| n.silo_id != silo_id);
    }
}

/// SDF utility: signed distance to a rounded rectangle.
///
/// This is the core GPU shader function that makes Qindows
/// UI elements mathematically perfect at any resolution.
pub fn sdf_rounded_rect(point: Vec2, rect_half: Vec2, radius: f32) -> f32 {
    let dx = (point.x.abs() - rect_half.x + radius).max(0.0);
    let dy = (point.y.abs() - rect_half.y + radius).max(0.0);
    (dx * dx + dy * dy).sqrt() - radius
}
