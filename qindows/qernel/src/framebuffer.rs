//! # Qernel Framebuffer Driver
//!
//! Provides basic framebuffer access for early boot graphics
//! and fallback display. Supports linear framebuffer from
//! UEFI/BIOS VBE, pixel plotting, rect fill, and scrolling.

/// Pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 32-bit BGRA (blue in lowest byte)
    Bgr32,
    /// 32-bit RGBA
    Rgb32,
    /// 24-bit BGR
    Bgr24,
    /// 16-bit RGB565
    Rgb565,
}

impl PixelFormat {
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            PixelFormat::Bgr32 | PixelFormat::Rgb32 => 4,
            PixelFormat::Bgr24 => 3,
            PixelFormat::Rgb565 => 2,
        }
    }
}

/// Framebuffer configuration (from bootloader).
#[derive(Debug, Clone, Copy)]
pub struct FbConfig {
    /// Physical address of the framebuffer
    pub phys_addr: u64,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Stride (bytes per scanline, may include padding)
    pub stride: u32,
    /// Pixel format
    pub format: PixelFormat,
}

/// A color value.
#[derive(Debug, Clone, Copy)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Pixel {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Pixel { r, g, b } }
    pub const BLACK: Pixel = Pixel::rgb(0, 0, 0);
    pub const WHITE: Pixel = Pixel::rgb(255, 255, 255);
    pub const RED: Pixel = Pixel::rgb(255, 0, 0);
    pub const GREEN: Pixel = Pixel::rgb(0, 255, 0);
    pub const BLUE: Pixel = Pixel::rgb(0, 0, 255);
}

/// The Framebuffer driver.
pub struct Framebuffer {
    /// Virtual address of the mapped framebuffer
    pub virt_addr: u64,
    /// Configuration
    pub config: FbConfig,
    /// Total size in bytes
    pub size: usize,
    /// Pixels written
    pub pixels_written: u64,
}

impl Framebuffer {
    /// Create a new framebuffer driver.
    ///
    /// # Safety
    /// `virt_addr` must map to the framebuffer's physical memory.
    pub unsafe fn new(virt_addr: u64, config: FbConfig) -> Self {
        let bpp = config.format.bytes_per_pixel();
        let size = config.stride as usize * config.height as usize;

        Framebuffer {
            virt_addr,
            config,
            size,
            pixels_written: 0,
        }
    }

    /// Plot a single pixel.
    pub unsafe fn put_pixel(&mut self, x: u32, y: u32, color: Pixel) {
        if x >= self.config.width || y >= self.config.height { return; }

        let bpp = self.config.format.bytes_per_pixel();
        let offset = (y as usize * self.config.stride as usize) + (x as usize * bpp);
        let ptr = (self.virt_addr as usize + offset) as *mut u8;

        match self.config.format {
            PixelFormat::Bgr32 => {
                *ptr = color.b;
                *ptr.add(1) = color.g;
                *ptr.add(2) = color.r;
                *ptr.add(3) = 0xFF;
            }
            PixelFormat::Rgb32 => {
                *ptr = color.r;
                *ptr.add(1) = color.g;
                *ptr.add(2) = color.b;
                *ptr.add(3) = 0xFF;
            }
            PixelFormat::Bgr24 => {
                *ptr = color.b;
                *ptr.add(1) = color.g;
                *ptr.add(2) = color.r;
            }
            PixelFormat::Rgb565 => {
                let val = ((color.r as u16 >> 3) << 11)
                    | ((color.g as u16 >> 2) << 5)
                    | (color.b as u16 >> 3);
                *(ptr as *mut u16) = val;
            }
        }
        self.pixels_written += 1;
    }

    /// Fill a rectangle with a solid color.
    pub unsafe fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Pixel) {
        let x_end = (x + w).min(self.config.width);
        let y_end = (y + h).min(self.config.height);

        for py in y..y_end {
            for px in x..x_end {
                self.put_pixel(px, py, color);
            }
        }
    }

    /// Clear the entire screen.
    pub unsafe fn clear(&mut self, color: Pixel) {
        self.fill_rect(0, 0, self.config.width, self.config.height, color);
    }

    /// Draw a horizontal line.
    pub unsafe fn hline(&mut self, x: u32, y: u32, length: u32, color: Pixel) {
        let end = (x + length).min(self.config.width);
        for px in x..end {
            self.put_pixel(px, y, color);
        }
    }

    /// Draw a vertical line.
    pub unsafe fn vline(&mut self, x: u32, y: u32, length: u32, color: Pixel) {
        let end = (y + length).min(self.config.height);
        for py in y..end {
            self.put_pixel(x, py, color);
        }
    }

    /// Scroll the screen up by `lines` pixel rows.
    pub unsafe fn scroll_up(&mut self, lines: u32) {
        if lines >= self.config.height { return self.clear(Pixel::BLACK); }

        let bpp = self.config.format.bytes_per_pixel();
        let stride = self.config.stride as usize;
        let src_offset = lines as usize * stride;
        let copy_size = (self.config.height as usize - lines as usize) * stride;

        let fb = self.virt_addr as *mut u8;
        core::ptr::copy(fb.add(src_offset), fb, copy_size);

        // Clear the bottom
        let clear_start = copy_size;
        core::ptr::write_bytes(fb.add(clear_start), 0, lines as usize * stride);
    }

    /// Draw an 8×16 bitmap glyph (for early boot console).
    pub unsafe fn draw_glyph(&mut self, x: u32, y: u32, glyph: &[u8; 16], fg: Pixel, bg: Pixel) {
        for row in 0..16u32 {
            let bits = glyph[row as usize];
            for col in 0..8u32 {
                let color = if bits & (0x80 >> col) != 0 { fg } else { bg };
                self.put_pixel(x + col, y + row, color);
            }
        }
    }

    /// Width in characters (8px wide glyphs).
    pub fn text_cols(&self) -> u32 { self.config.width / 8 }

    /// Height in characters (16px tall glyphs).
    pub fn text_rows(&self) -> u32 { self.config.height / 16 }
}
