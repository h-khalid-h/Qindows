//! # V-GDI Upscale Silo Cap Bridge (Phase 248)
//!
//! ## Architecture Guardian: The Gap
//! `v_gdi_upscale.rs` implements the Win32 GDI → SDF upscaler:
//! - `CaptureBuffer::new(width, height, silo_id)` — framebuffer for GDI→SDF
//! - `CaptureBuffer::pixel(x, y)` → (r,g,b,a)
//! - `EdgeMap::from_capture(buf)` — extract edges for SDF rendering
//!
//! **Missing link**: `CaptureBuffer` had `silo_id` metadata but pixel
//! reads were not checked against it. A Silo could read pixels from
//! another Silo's `CaptureBuffer` (framebuffer pixel leak).
//!
//! This module provides `VGdiUpscaleSiloCapBridge`:
//! Silo ownership check before pixel reads from CaptureBuffer.

extern crate alloc;

use crate::v_gdi_upscale::CaptureBuffer;
use crate::qaudit_kernel::QAuditKernel;

#[derive(Debug, Default, Clone)]
pub struct VGdiCapStats {
    pub pixel_reads_allowed: u64,
    pub pixel_reads_denied:  u64,
}

pub struct VGdiUpscaleSiloCapBridge {
    pub stats: VGdiCapStats,
}

impl VGdiUpscaleSiloCapBridge {
    pub fn new() -> Self {
        VGdiUpscaleSiloCapBridge { stats: VGdiCapStats::default() }
    }

    /// Authorize pixel read from a CaptureBuffer — must be owned by reader.
    pub fn authorize_pixel_read(
        &mut self,
        reader_silo: u64,
        buf: &CaptureBuffer,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> bool {
        if reader_silo != buf.silo_id {
            self.stats.pixel_reads_denied += 1;
            audit.log_law_violation(6u8, reader_silo, tick);
            crate::serial_println!(
                "[V-GDI] Silo {} denied reading Silo {} capture buffer (Law 6)", reader_silo, buf.silo_id
            );
            return false;
        }
        self.stats.pixel_reads_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  VGdiCapBridge: allowed={} denied={}",
            self.stats.pixel_reads_allowed, self.stats.pixel_reads_denied
        );
    }
}
