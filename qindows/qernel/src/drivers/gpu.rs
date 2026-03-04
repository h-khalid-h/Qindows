//! # Aether Frame Buffer Driver
//!
//! Minimal GPU driver for the boot-time framebuffer.
//! Uses the UEFI GOP framebuffer until the full Aether
//! Vulkan/WGPU compositor is loaded from user space.

/// The Aether framebuffer — direct pixel access via memory-mapped I/O.
pub struct AetherFrameBuffer {
    /// Base address of the framebuffer memory
    buffer: *mut u32,
    /// Width in pixels
    width: usize,
    /// Height in pixels
    height: usize,
    /// Pixels per scanline (may differ from width due to alignment)
    stride: usize,
}

impl AetherFrameBuffer {
    /// Create a new framebuffer from GOP parameters.
    pub fn new(buffer: *mut u32, width: usize, height: usize, stride: usize) -> Self {
        AetherFrameBuffer {
            buffer,
            width,
            height,
            stride,
        }
    }

    /// Clear the entire screen to a single color (ARGB).
    ///
    /// This is the "fastest" whole-screen fill — used during boot
    /// to display the signature Qindows deep black (#06060E).
    pub fn clear(&mut self, color: u32) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.draw_pixel(x, y, color);
            }
        }
    }

    /// Draw a single pixel at (x, y) with color (ARGB).
    #[inline(always)]
    pub fn draw_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            let offset = y * self.stride + x;
            unsafe {
                self.buffer.add(offset).write_volatile(color);
            }
        }
    }

    /// Draw a filled rectangle.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for py in y..y.saturating_add(h).min(self.height) {
            for px in x..x.saturating_add(w).min(self.width) {
                self.draw_pixel(px, py, color);
            }
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

/// Draw the Qindows boot logo — a minimal "Q" glyph.
///
/// This is the first visual the user sees after POST.
/// It's a placeholder until the full Aether vector engine
/// is loaded from user space.
pub fn draw_boot_logo(fb: &mut AetherFrameBuffer) {
    let cx = fb.width() / 2;
    let cy = fb.height() / 2;
    let accent = 0x00_06_D6_A0; // Qindows Cyan

    // Draw a simple "crosshair" to confirm display is working
    // Horizontal line
    fb.fill_rect(cx - 40, cy, 80, 2, accent);
    // Vertical line
    fb.fill_rect(cx, cy - 40, 2, 80, accent);
    // Center dot
    fb.fill_rect(cx - 3, cy - 3, 6, 6, accent);
}
