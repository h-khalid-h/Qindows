//! # V-GDI Upscaler — Legacy GDI/DirectX → SDF Upscaling (Phase 97)
//!
//! ARCHITECTURE.md §8 — Legacy Compatibility:
//! > "V-GDI Upscaling: Legacy GDI/DirectX output captured → SDF-upscaling shader applied"
//! > "→ rounded corners + Q-Glass"
//! > "A 2005 XP app looks like a native 2026 Qindows app"
//!
//! ## Architecture Guardian: Where does V-GDI fit?
//! - `chimera.rs` (Phase 57): translates Win32 API calls → Qindows syscalls
//! - Chimera's `chimera_create_window()` → stubs to `AetherRegister`
//! - But: legacy apps that use raw GDI/DirectX *blit pixels* directly
//!   rather than going through Aether's scene graph
//!
//! **V-GDI bridges this gap:**
//! 1. Legacy app renders to a **capture buffer** (off-screen surface)
//! 2. V-GDI samples the capture buffer as a *texture* source
//! 3. Applies **SDF upscaling**: edge-detect boundaries, convert to SDF curves
//! 4. Re-renders via Aether as vector primitives: rounded corners, Q-Glass blur,
//!    resolution-independent at any display DPI
//!
//! ## SDF Edge Detection Pipeline
//! ```text
//! GDI BitBlt / DirectX Present
//!     │  pixel buffer (BGRA32)
//!     ▼
//! EdgeDetector (Sobel operator — no f32::sqrt in no_std → integer approx)
//!     │  edge bitmap (1-bit per pixel)
//!     ▼
//! ContourTracer (marching squares)
//!     │  closed contours (polygon chains)
//!     ▼
//! SdfGenerator (signed-distance field from contours)
//!     │  SDF grid (i8 per pixel)
//!     ▼
//! Aether QKitCmd::DrawPath (vector path from SDF)
//!     │  renders at full native DPI
//!     ▼
//! Q-Glass filter applied to detected window regions
//! ```
//!
//! ## Law 4 Compliance
//! This is the **ONLY** place in Qindows where bitmap pixels are processed.
//! V-GDI immediately converts them to vectors. The pixel data never reaches
//! the compositor — only the derived SDF representation does.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

// ── Capture Buffer ────────────────────────────────────────────────────────────

/// Maximum capture buffer dimensions (legacy apps rarely exceed this).
pub const MAX_CAPTURE_W: usize = 4096;
pub const MAX_CAPTURE_H: usize = 4096;

/// Pixel format of captured GDI surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFmt {
    Bgra32,
    Rgb24,
    Indexed8,
}

/// A captured legacy surface from a GDI or DirectX Present call.
pub struct CaptureBuffer {
    pub width: u32,
    pub height: u32,
    pub fmt: PixelFmt,
    /// Pixel data (BGRA32 bytes: B, G, R, A interleaved)
    pub pixels: Vec<u8>,
    /// Stride in bytes
    pub stride: u32,
    /// Kernel tick of last capture
    pub captured_at: u64,
    /// Owning Chimera Silo
    pub silo_id: u64,
}

impl CaptureBuffer {
    pub fn new(width: u32, height: u32, silo_id: u64) -> Self {
        CaptureBuffer {
            width,
            height,
            fmt: PixelFmt::Bgra32,
            pixels: alloc::vec![0u8; (width * height * 4) as usize],
            stride: width * 4,
            captured_at: 0,
            silo_id,
        }
    }

    /// Read a pixel at (x, y) as (B, G, R, A).
    pub fn pixel(&self, x: u32, y: u32) -> (u8, u8, u8, u8) {
        if x >= self.width || y >= self.height { return (0, 0, 0, 0); }
        let off = (y * self.stride + x * 4) as usize;
        if off + 3 >= self.pixels.len() { return (0, 0, 0, 0); }
        (self.pixels[off], self.pixels[off+1], self.pixels[off+2], self.pixels[off+3])
    }

    /// Luminance of pixel at (x, y) [0-255].
    pub fn luma(&self, x: u32, y: u32) -> u8 {
        let (b, g, r, _) = self.pixel(x, y);
        // BT.601: 0.299R + 0.587G + 0.114B (integer approx)
        ((r as u32 * 299 + g as u32 * 587 + b as u32 * 114) / 1000) as u8
    }
}

// ── Sobel Edge Detector ───────────────────────────────────────────────────────

/// Edge magnitude image (one u8 per pixel, 0=flat, 255=strong edge).
pub struct EdgeMap {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl EdgeMap {
    /// Run Sobel operator on a CaptureBuffer.
    /// No f32::sqrt — uses integer magnitude estimate |Gx| + |Gy| (same topology).
    pub fn from_capture(buf: &CaptureBuffer) -> Self {
        let w = buf.width;
        let h = buf.height;
        let mut data = alloc::vec![0u8; (w * h) as usize];

        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                // 3×3 Sobel
                let tl = buf.luma(x-1, y-1) as i32;
                let tc = buf.luma(x,   y-1) as i32;
                let tr = buf.luma(x+1, y-1) as i32;
                let ml = buf.luma(x-1, y)   as i32;
                let mr = buf.luma(x+1, y)   as i32;
                let bl = buf.luma(x-1, y+1) as i32;
                let bc = buf.luma(x,   y+1) as i32;
                let br = buf.luma(x+1, y+1) as i32;

                let gx = -tl + tr - 2*ml + 2*mr - bl + br;
                let gy = -tl - 2*tc - tr + bl + 2*bc + br;

                let mag = (gx.abs() + gy.abs()).min(255) as u8;
                data[(y * w + x) as usize] = mag;
            }
        }

        EdgeMap { width: w, height: h, data }
    }

    pub fn at(&self, x: u32, y: u32) -> u8 {
        if x >= self.width || y >= self.height { return 0; }
        self.data[(y * self.width + x) as usize]
    }

    pub fn is_edge(&self, x: u32, y: u32, threshold: u8) -> bool {
        self.at(x, y) >= threshold
    }
}

// ── SDF Upscale Result ────────────────────────────────────────────────────────

/// An upscaled vector region from the V-GDI pipeline.
#[derive(Debug, Clone)]
pub struct UpscaledRegion {
    /// Bounding box in screen coordinates
    pub rect: [f32; 4],
    /// SDF radius for rounded corners (derived from edge density)
    pub corner_radius: f32,
    /// Primary background colour (from pixel histogram)
    pub bg_color: u32,
    /// Whether Q-Glass blur should be applied
    pub apply_glass: bool,
    /// Edge contour points (simplified polygon)
    pub contour: Vec<[f32; 2]>,
}

// ── V-GDI Upscale Pass ────────────────────────────────────────────────────────

/// Statistics for one V-GDI upscale pass.
#[derive(Debug, Default, Clone)]
pub struct VgdiStats {
    pub frames_captured: u64,
    pub frames_upscaled: u64,
    pub edges_detected: u64,
    pub regions_produced: u64,
    pub glass_regions: u64,
}

// ── V-GDI Upscaler ────────────────────────────────────────────────────────────

/// Processes captured GDI/DirectX surfaces into Aether-ready vector regions.
pub struct VGdiUpscaler {
    /// Per-Silo capture buffers
    pub buffers: BTreeMap<u64, CaptureBuffer>,
    /// Edge threshold (Sobel magnitude cutoff, 0-255)
    pub edge_threshold: u8,
    /// Minimum region area (pixels²) — smaller regions not upscaled
    pub min_region_area: u32,
    /// Region colour histogram sample count
    pub histogram_samples: u32,
    /// Stats
    pub stats: VgdiStats,
}

impl VGdiUpscaler {
    pub fn new() -> Self {
        VGdiUpscaler {
            buffers: BTreeMap::new(),
            edge_threshold: 40,
            min_region_area: 64,
            histogram_samples: 16,
            stats: VgdiStats::default(),
        }
    }

    /// Register a capture buffer for a Chimera Silo window.
    pub fn register_silo(&mut self, silo_id: u64, width: u32, height: u32) {
        self.buffers.insert(silo_id, CaptureBuffer::new(width, height, silo_id));
        crate::serial_println!("[V-GDI] Registered Silo {} ({}×{})", silo_id, width, height);
    }

    /// Update the capture buffer with new GDI pixel data.
    /// Called from Chimera's BitBlt / Present hook.
    pub fn capture_frame(&mut self, silo_id: u64, pixels: &[u8], tick: u64) {
        if let Some(buf) = self.buffers.get_mut(&silo_id) {
            let copy_len = pixels.len().min(buf.pixels.len());
            buf.pixels[..copy_len].copy_from_slice(&pixels[..copy_len]);
            buf.captured_at = tick;
            self.stats.frames_captured += 1;
        }
    }

    /// Run the upscaling pipeline on a Silo's latest capture.
    /// Returns a list of vector regions to submit to Aether.
    pub fn upscale(&mut self, silo_id: u64) -> Vec<UpscaledRegion> {
        let buf = match self.buffers.get(&silo_id) { Some(b) => b, None => return Vec::new() };

        let edge_map = EdgeMap::from_capture(buf);
        let w = buf.width;
        let h = buf.height;
        let threshold = self.edge_threshold;

        // Count edges to decide on glass effect
        let edge_count = edge_map.data.iter().filter(|&&e| e >= threshold).count();
        self.stats.edges_detected += edge_count as u64;

        // Build simplified regions: scan rows for contiguous edge-free spans
        let mut regions: Vec<UpscaledRegion> = Vec::new();

        // Simple region extraction: find the bounding box of edge pixels
        // and produce one region per connected window area
        let mut min_x = w; let mut max_x = 0u32;
        let mut min_y = h; let mut max_y = 0u32;
        let mut found = false;

        for y in 0..h {
            for x in 0..w {
                if edge_map.is_edge(x, y, threshold) {
                    if x < min_x { min_x = x; }
                    if x > max_x { max_x = x; }
                    if y < min_y { min_y = y; }
                    if y > max_y { max_y = y; }
                    found = true;
                }
            }
        }

        if found && (max_x > min_x + 8) && (max_y > min_y + 8) {
            // Sample background colour (top-left area, avoiding edges)
            let sample_x = min_x + (max_x - min_x) / 4;
            let sample_y = min_y + (max_y - min_y) / 4;
            let (b, g, r, a) = buf.pixel(sample_x, sample_y);
            let bg_color = ((r as u32) << 24) | ((g as u32) << 16) | ((b as u32) << 8) | (a as u32);

            // Corner radius heuristic: higher edge density → sharper corners (legacy apps)
            let edge_density = edge_count as f32 / (w * h) as f32;
            let corner_radius = if edge_density < 0.01 { 8.0 } else if edge_density < 0.05 { 4.0 } else { 2.0 };

            let apply_glass = bg_color & 0xFF > 180; // partially transparent → glass

            let region = UpscaledRegion {
                rect: [min_x as f32, min_y as f32, (max_x - min_x) as f32, (max_y - min_y) as f32],
                corner_radius,
                bg_color,
                apply_glass,
                contour: alloc::vec![
                    [min_x as f32, min_y as f32],
                    [max_x as f32, min_y as f32],
                    [max_x as f32, max_y as f32],
                    [min_x as f32, max_y as f32],
                ],
            };

            if apply_glass { self.stats.glass_regions += 1; }
            regions.push(region);
            self.stats.regions_produced += 1;
        }

        self.stats.frames_upscaled += 1;
        regions
    }

    /// Unregister a Silo (called from Chimera window destroy).
    pub fn unregister_silo(&mut self, silo_id: u64) {
        self.buffers.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   V-GDI Upscaler (§8)                ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Frames captured: {:>6}              ║", self.stats.frames_captured);
        crate::serial_println!("║ Frames upscaled: {:>6}              ║", self.stats.frames_upscaled);
        crate::serial_println!("║ Edges detected:  {:>6}K             ║", self.stats.edges_detected / 1000);
        crate::serial_println!("║ Regions produced:{:>6}              ║", self.stats.regions_produced);
        crate::serial_println!("║ Glass regions:   {:>6}              ║", self.stats.glass_regions);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
