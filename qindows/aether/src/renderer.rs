//! # Aether SDF Renderer
//!
//! Signed Distance Field rendering pipeline for the Qindows UI.
//! All UI elements are mathematical shapes — no bitmaps.
//! This enables infinite scaling, GPU-accelerated rendering,
//! and the signature Q-Glass material effect.

extern crate alloc;

use alloc::vec::Vec;
use crate::math_ext::F32Ext;

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

    /// Software Rasterizer: render this entire frame using a pixel-plotting callback.
    ///
    /// Since Aether is a `#![no_std]` UI library isolated from the kernel and hardware,
    /// it does not talk to the `Framebuffer` directly. Instead, the kernel passes
    /// a closure `put_pixel(x, y, color)` and Aether executes the math.
    ///
    /// This is slow for a full 4K screen, but perfectly demonstrates the architecture
    /// during Genesis Alpha.
    pub fn rasterize<F>(&self, mut put_pixel: F)
    where
        F: FnMut(u32, u32, Color),
    {
        // For software rendering, we compute bounding boxes for every command
        // and evaluate the SDF for each pixel in that box.
        for cmd in &self.commands {
            match cmd {
                RenderCommand::RoundedRect { x, y, width, height, radius, fill, border } => {
                    let min_x = *x as i32;
                    let min_y = *y as i32;
                    let max_x = (*x + *width) as i32;
                    let max_y = (*y + *height) as i32;

                    let cx = *x + *width / 2.0;
                    let cy = *y + *height / 2.0;
                    let half_w = *width / 2.0;
                    let half_h = *height / 2.0;

                    for py in min_y..=max_y {
                        for px in min_x..=max_x {
                            if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                                continue;
                            }

                            let p = Vec2::new(px as f32, py as f32);
                            let center = Vec2::new(cx, cy);
                            let half_size = Vec2::new(half_w, half_h);
                            
                            // Evaluate the SDF
                            let dist = sdf::rounded_rect(p, center, half_size, *radius);

                            if dist <= 0.0 {
                                // Inside the rect
                                let mut final_color = *fill;

                                // Handle borders
                                if let Some((border_width, border_color)) = border {
                                    if dist > -*border_width {
                                        final_color = *border_color;
                                    }
                                }
                                put_pixel(px as u32, py as u32, final_color);
                            }
                        }
                    }
                }
                RenderCommand::GlassBlur { x, y, width, height, radius, tint, .. } => {
                    // Glass blur is a heavy convolution matrix in hardware.
                    // For our software fallback, we will just tint it.
                    let min_x = *x as i32;
                    let min_y = *y as i32;
                    let max_x = (*x + *width) as i32;
                    let max_y = (*y + *height) as i32;

                    let cx = *x + *width / 2.0;
                    let cy = *y + *height / 2.0;
                    let half_size = Vec2::new(*width / 2.0, *height / 2.0);

                    for py in min_y..=max_y {
                        for px in min_x..=max_x {
                            if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                                continue;
                            }
                            let dist = sdf::rounded_rect(
                                Vec2::new(px as f32, py as f32),
                                Vec2::new(cx, cy),
                                half_size,
                                *radius,
                            );
                            if dist <= 0.0 {
                                put_pixel(px as u32, py as u32, *tint);
                            }
                        }
                    }
                }
                RenderCommand::Shadow { x, y, width, height, radius, blur, offset_x, offset_y, color } => {
                    // Shadows are just larger, faint rounded rects under the main body.
                    let shadow_width = *width + *blur * 2.0;
                    let shadow_height = *height + *blur * 2.0;
                    let sx = *x - *blur + *offset_x;
                    let sy = *y - *blur + *offset_y;

                    let min_x = sx as i32;
                    let min_y = sy as i32;
                    let max_x = (sx + shadow_width) as i32;
                    let max_y = (sy + shadow_height) as i32;

                    let cx = sx + shadow_width / 2.0;
                    let cy = sy + shadow_height / 2.0;
                    let half_size = Vec2::new(shadow_width / 2.0, shadow_height / 2.0);

                    for py in min_y..=max_y {
                        for px in min_x..=max_x {
                            if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                                continue;
                            }
                            let dist = sdf::rounded_rect(
                                Vec2::new(px as f32, py as f32),
                                Vec2::new(cx, cy),
                                half_size,
                                *radius + *blur,
                            );
                            if dist <= 0.0 {
                                // Fade the shadow based on distance to the edge
                                let alpha = 1.0 - (dist / -*blur).max(0.0).min(1.0);
                                let mut final_color = *color;
                                final_color.a *= alpha;
                                put_pixel(px as u32, py as u32, final_color);
                            }
                        }
                    }
                }
                RenderCommand::Gradient { x, y, width, height, start_color, end_color, .. } => {
                    // Top-to-bottom generic linear gradient
                    let min_x = *x as i32;
                    let min_y = *y as i32;
                    let max_x = (*x + *width) as i32;
                    let max_y = (*y + *height) as i32;

                    for py in min_y..=max_y {
                        for px in min_x..=max_x {
                            if px < 0 || py < 0 || px >= self.width as i32 || py >= self.height as i32 {
                                continue;
                            }
                            let t = ((py as f32 - *y) / *height).max(0.0).min(1.0);
                            let color = start_color.lerp(end_color, t);
                            put_pixel(px as u32, py as u32, color);
                        }
                    }
                }
                RenderCommand::Text { x, y, text, size: _, color } => {
                    // For software rendering without the TTF/SDF glyph atlas loaded,
                    // we'll just simulate rendering by passing the text layout box back
                    // as a small debug block or delegating to the kernel's VGA font.
                    // (Real SDF font rendering requires parsing a TTF table).
                    // As a hack for the Alpha bootloader demo, we just rely on qernel's debug text.
                    let _ = (x, y, text, color);
                }
                // Ignoring clipping and opacity stacks for the initial software loop
                _ => {}
            }
        }
    }
}
