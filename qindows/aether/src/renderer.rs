//! # Aether SDF Renderer
//!
//! Signed Distance Field rendering pipeline for the Qindows UI.
//! All UI elements are mathematical shapes — no bitmaps.
//! This enables infinite scaling, GPU-accelerated rendering,
//! and the signature Q-Glass material effect.

extern crate alloc;

use alloc::vec::Vec;
use crate::math_ext::{F32Ext, F64Ext};

/// A color (premultiplied alpha RGBA).
#[derive(Debug, Clone, Copy, Default)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Color { r, g, b, a }
    }
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Color { r, g, b, a: 1.0 }
    }
    pub fn from_hex(hex: u32) -> Self {
        Color {
            r: ((hex >> 16) & 0xFF) as f32 / 255.0,
            g: ((hex >> 8) & 0xFF) as f32 / 255.0,
            b: (hex & 0xFF) as f32 / 255.0,
            a: ((hex >> 24) & 0xFF) as f32 / 255.0,
        }
    }
    pub fn lerp(&self, other: &Color, t: f32) -> Color {
        Color {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }
}

/// 2D point / vector.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const fn new(x: f32, y: f32) -> Self { Vec2 { x, y } }
    pub fn length(&self) -> f32 { (self.x * self.x + self.y * self.y).sqrt() }
    pub fn dot(&self, other: &Vec2) -> f32 { self.x * other.x + self.y * other.y }
    pub fn sub(&self, other: &Vec2) -> Vec2 { Vec2::new(self.x - other.x, self.y - other.y) }
    pub fn abs(&self) -> Vec2 { Vec2::new(self.x.abs(), self.y.abs()) }
    pub fn max_scalar(&self, s: f32) -> Vec2 { Vec2::new(self.x.max(s), self.y.max(s)) }
}

/// SDF primitives — mathematical distance functions.
pub mod sdf {
    use super::Vec2;

    /// Circle: negative inside, positive outside.
    pub fn circle(p: Vec2, center: Vec2, radius: f32) -> f32 {
        p.sub(&center).length() - radius
    }

    /// Rounded rectangle.
    pub fn rounded_rect(p: Vec2, center: Vec2, half_size: Vec2, radius: f32) -> f32 {
        let d = p.sub(&center).abs().sub(&half_size).max_scalar(0.0);
        d.length() + d.x.min(0.0).max(d.y.min(0.0)) - radius
    }

    /// Line segment (for borders).
    pub fn segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let pa = p.sub(&a);
        let ba = b.sub(&a);
        let t = pa.dot(&ba) / ba.dot(&ba);
        let t = t.max(0.0).min(1.0);
        let closest = Vec2::new(a.x + ba.x * t, a.y + ba.y * t);
        p.sub(&closest).length()
    }

    /// Union of two SDFs (merge shapes).
    pub fn union(d1: f32, d2: f32) -> f32 {
        d1.min(d2)
    }

    /// Smooth union (blend shapes with rounded intersection).
    pub fn smooth_union(d1: f32, d2: f32, k: f32) -> f32 {
        let h = (0.5 + 0.5 * (d2 - d1) / k).max(0.0).min(1.0);
        d2 + (d1 - d2) * h - k * h * (1.0 - h)
    }

    /// Subtraction (cut one shape from another).
    pub fn subtract(d1: f32, d2: f32) -> f32 {
        (-d1).max(d2)
    }

    /// Intersection (only where both shapes overlap).
    pub fn intersect(d1: f32, d2: f32) -> f32 {
        d1.max(d2)
    }

    /// Annular ring (outline of a shape).
    pub fn annular(d: f32, thickness: f32) -> f32 {
        d.abs() - thickness
    }
}

/// A render command in the scene graph.
#[derive(Debug, Clone)]
pub enum RenderCommand {
    /// Draw a rounded rectangle (windows, buttons, cards)
    RoundedRect {
        x: f32, y: f32,
        width: f32, height: f32,
        radius: f32,
        fill: Color,
        border: Option<(f32, Color)>,
    },
    /// Draw a circle (avatars, loading spinners)
    Circle {
        cx: f32, cy: f32,
        radius: f32,
        fill: Color,
    },
    /// Draw text (rendered as SDF glyph atlas)
    Text {
        x: f32, y: f32,
        text: alloc::string::String,
        size: f32,
        color: Color,
    },
    /// Q-Glass blur effect (frosted glass behind a rect)
    GlassBlur {
        x: f32, y: f32,
        width: f32, height: f32,
        radius: f32,
        blur_radius: f32,
        tint: Color,
    },
    /// Shadow (drop shadow beneath a shape)
    Shadow {
        x: f32, y: f32,
        width: f32, height: f32,
        radius: f32,
        blur: f32,
        offset_x: f32,
        offset_y: f32,
        color: Color,
    },
    /// Gradient fill
    Gradient {
        x: f32, y: f32,
        width: f32, height: f32,
        start_color: Color,
        end_color: Color,
        angle: f32,
    },
    /// Image (rasterized from Prism OID)
    Image {
        x: f32, y: f32,
        width: f32, height: f32,
        oid: u64,
    },
    /// Clip rectangle (scissor test)
    PushClip { x: f32, y: f32, width: f32, height: f32 },
    /// End clip
    PopClip,
    /// Opacity layer
    PushOpacity(f32),
    PopOpacity,
}

/// A frame of render commands ready for GPU submission.
pub struct RenderFrame {
    /// Render commands (back-to-front order)
    pub commands: Vec<RenderCommand>,
    /// Frame width in logical pixels
    pub width: f32,
    /// Frame height in logical pixels
    pub height: f32,
    /// DPI scale factor
    pub scale: f32,
    /// Frame sequence number
    pub frame_number: u64,
}

impl RenderFrame {
    pub fn new(width: f32, height: f32, scale: f32) -> Self {
        RenderFrame {
            commands: Vec::new(),
            width,
            height,
            scale,
            frame_number: 0,
        }
    }

    /// Add a render command.
    pub fn push(&mut self, cmd: RenderCommand) {
        self.commands.push(cmd);
    }

    /// Draw a window frame (Q-Glass material).
    pub fn draw_window(
        &mut self,
        x: f32, y: f32,
        width: f32, height: f32,
        title_height: f32,
        focused: bool,
    ) {
        // Shadow
        self.push(RenderCommand::Shadow {
            x, y, width, height,
            radius: 12.0,
            blur: if focused { 24.0 } else { 12.0 },
            offset_x: 0.0,
            offset_y: if focused { 8.0 } else { 4.0 },
            color: Color::rgba(0.0, 0.0, 0.0, if focused { 0.4 } else { 0.2 }),
        });

        // Glass body
        self.push(RenderCommand::GlassBlur {
            x, y, width, height,
            radius: 12.0,
            blur_radius: 20.0,
            tint: Color::rgba(0.1, 0.1, 0.15, 0.85),
        });

        // Title bar gradient
        self.push(RenderCommand::Gradient {
            x, y,
            width, height: title_height,
            start_color: Color::rgba(0.15, 0.15, 0.2, 0.9),
            end_color: Color::rgba(0.1, 0.1, 0.15, 0.9),
            angle: 180.0,
        });

        // Focus glow
        if focused {
            self.push(RenderCommand::RoundedRect {
                x: x - 1.0, y: y - 1.0,
                width: width + 2.0, height: height + 2.0,
                radius: 13.0,
                fill: Color::rgba(0.0, 0.0, 0.0, 0.0),
                border: Some((1.5, Color::rgba(0.024, 0.839, 0.627, 0.6))),
            });
        }
    }

    /// Draw a button.
    pub fn draw_button(
        &mut self,
        x: f32, y: f32,
        width: f32, height: f32,
        label: &str,
        hovered: bool,
        pressed: bool,
    ) {
        let fill = if pressed {
            Color::rgba(0.024, 0.839, 0.627, 0.9)
        } else if hovered {
            Color::rgba(0.024, 0.839, 0.627, 0.7)
        } else {
            Color::rgba(0.2, 0.2, 0.25, 0.8)
        };

        self.push(RenderCommand::RoundedRect {
            x, y, width, height,
            radius: 8.0,
            fill,
            border: None,
        });

        self.push(RenderCommand::Text {
            x: x + width / 2.0,
            y: y + height / 2.0,
            text: alloc::string::String::from(label),
            size: 14.0,
            color: Color::rgb(1.0, 1.0, 1.0),
        });
    }

    /// Get memory estimate for this frame.
    pub fn memory_bytes(&self) -> usize {
        self.commands.len() * core::mem::size_of::<RenderCommand>()
    }
}
