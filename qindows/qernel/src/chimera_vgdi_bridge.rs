//! # Chimera → V-GDI Bridge (Phase 103)
//!
//! ARCHITECTURE.md §8 — Project Chimera:
//! > "V-GDI Upscaling: Legacy GDI/DirectX output captured → SDF-upscaling shader applied"
//! > "A 2005 XP app looks like a native 2026 Qindows app"
//!
//! ## Architecture Guardian: The Gap
//! - `chimera/src/screen_capture.rs` — the **userspace** Chimera crate captures GDI surfaces
//! - `chimera/src/gdi.rs` — routes Win32 `BitBlt()` / `StretchBlt()` calls
//! - `qernel/src/chimera.rs` (Phase 57) — **kernel-side** Win32 API translation
//! - `qernel/src/v_gdi_upscale.rs` (Phase 97) — SDF upscaling engine
//!
//! **Missing link**: kernel-side `chimera.rs` never called `v_gdi_upscale.rs`.
//! The `chimera_create_window()` stub calls `AetherRegister` but there is no
//! pixel-data path between GDI `BitBlt` and the upscaler.
//!
//! ## Data Flow (complete path after this module)
//! ```text
//! Win32 App calls BitBlt(hdc, pixels)
//!     │  (inside Chimera Silo userspace)
//!     ▼
//! chimera/src/gdi.rs: intercepts BitBlt
//!     │  IPC message: GDI_BLIT { silo_id, pixels, w, h }
//!     ▼
//! Q-Ring SqOpcode::GpuSubmit (kernel receives blit request)
//!     │  dispatched to chimera_vgdi_bridge.rs
//!     ▼
//! chimera_vgdi_bridge.rs:
//!   1. Routes pixels to v_gdi_upscale::VGdiUpscaler::capture_frame()
//!   2. Calls upscaler.upscale(silo_id) → Vec<UpscaledRegion>
//!   3. Converts UpscaledRegion → AetherWindow QKitCmd stream
//!   4. Submits to Aether compositor via Q-Ring AetherSubmit
//! ```
//!
//! ## Law 4 Compliance (maintained)
//! Raw `pixels` never reach Aether — `capture_frame()` + `upscale()` converts
//! them to SDF contour polygons before the compositor receives any data.

extern crate alloc;
use alloc::vec::Vec;

use crate::v_gdi_upscale::{VGdiUpscaler, UpscaledRegion};

// ── GDI Blit Request (from chimera Silo via Q-Ring) ──────────────────────────

/// A GDI surface capture blit sent from a Chimera Silo's `BitBlt` hook.
#[derive(Debug, Clone)]
pub struct GdiBlitRequest {
    /// Chimera Silo ID that issued the BitBlt
    pub silo_id: u64,
    /// Source surface pixels (BGRA32)
    pub pixels: Vec<u8>,
    /// Surface width in pixels
    pub width: u32,
    /// Surface height in pixels
    pub height: u32,
    /// Destination X on screen
    pub dst_x: i32,
    /// Destination Y on screen
    pub dst_y: i32,
    /// Kernel tick of the blit
    pub tick: u64,
}

// ── Aether Submit (upscaled region → Aether command) ─────────────────────────

/// An Aether-ready command generated from an upscaled GDI region.
#[derive(Debug, Clone)]
pub struct AetherBlitCmd {
    /// Chimera Silo owning this window
    pub silo_id: u64,
    /// Screen rect [x, y, w, h]
    pub rect: [f32; 4],
    /// SDF corner radius
    pub corner_radius: f32,
    /// Background colour
    pub color: u32,
    /// Blur radius for Q-Glass (0 = no blur)
    pub blur_radius: f32,
    /// Tick submitted
    pub tick: u64,
}

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct ChimeraVgdiBridgeStats {
    pub blits_received: u64,
    pub blits_upscaled: u64,
    pub regions_to_aether: u64,
    pub glass_regions: u64,
    pub silos_registered: u64,
}

// ── Chimera V-GDI Bridge ──────────────────────────────────────────────────────

/// Connects the Chimera GDI capture path to the V-GDI SDF upscaler.
pub struct ChimeraVgdiBridge {
    pub stats: ChimeraVgdiBridgeStats,
    /// Pending Aether commands ready for compositor submission
    pub pending_cmds: Vec<AetherBlitCmd>,
}

impl ChimeraVgdiBridge {
    pub fn new() -> Self {
        ChimeraVgdiBridge {
            stats: ChimeraVgdiBridgeStats::default(),
            pending_cmds: Vec::new(),
        }
    }

    /// Register a new Chimera Silo window with the V-GDI upscaler.
    /// Called when `chimera_create_window()` succeeds.
    pub fn register_window(&self, silo_id: u64, width: u32, height: u32, upscaler: &mut VGdiUpscaler) {
        upscaler.register_silo(silo_id, width, height);
        crate::serial_println!(
            "[CHIMERA-VGDI] Registered Silo {} window ({}×{}) with V-GDI upscaler",
            silo_id, width, height
        );
    }

    /// Handle a GDI BitBlt capture from a Chimera Silo.
    /// 1. Forwards pixels to V-GDI upscaler capture buffer
    /// 2. Runs upscale pass
    /// 3. Converts regions to Aether commands
    /// Returns the number of Aether commands generated.
    pub fn handle_blit(
        &mut self,
        req: GdiBlitRequest,
        upscaler: &mut VGdiUpscaler,
    ) -> usize {
        self.stats.blits_received += 1;

        // 1. Feed pixels into the capture buffer
        upscaler.capture_frame(req.silo_id, &req.pixels, req.tick);

        // 2. Run SDF upscale pipeline
        let regions: Vec<UpscaledRegion> = upscaler.upscale(req.silo_id);

        if regions.is_empty() { return 0; }

        self.stats.blits_upscaled += 1;
        let region_count = regions.len();

        // 3. Convert UpscaledRegions → AetherBlitCmd (Law 4: SDF only)
        for region in regions {
            let blur = if region.apply_glass { 8.0 } else { 0.0 };
            if region.apply_glass { self.stats.glass_regions += 1; }

            // Offset region by destination position from screen
            let cmd = AetherBlitCmd {
                silo_id: req.silo_id,
                rect: [
                    region.rect[0] + req.dst_x as f32,
                    region.rect[1] + req.dst_y as f32,
                    region.rect[2],
                    region.rect[3],
                ],
                corner_radius: region.corner_radius,
                color: region.bg_color,
                blur_radius: blur,
                tick: req.tick,
            };
            self.pending_cmds.push(cmd);
            self.stats.regions_to_aether += 1;
        }

        crate::serial_println!(
            "[CHIMERA-VGDI] Silo {} blit → {} SDF regions (glass={})",
            req.silo_id, region_count, self.stats.glass_regions
        );

        region_count
    }

    /// Drain and return all pending Aether compositor commands.
    /// Called by the Aether compositor on vsync to consume legacy window updates.
    pub fn drain_aether_cmds(&mut self) -> Vec<AetherBlitCmd> {
        let cmds = self.pending_cmds.clone();
        self.pending_cmds.clear();
        cmds
    }

    /// Unregister a Chimera window (on Win32 `DestroyWindow`).
    pub fn unregister_window(&self, silo_id: u64, upscaler: &mut VGdiUpscaler) {
        upscaler.unregister_silo(silo_id);
        crate::serial_println!("[CHIMERA-VGDI] Silo {} window unregistered", silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!("  ChimeraVGDI blits={} upscaled={} aether_cmds={} glass={}",
            self.stats.blits_received, self.stats.blits_upscaled,
            self.stats.regions_to_aether, self.stats.glass_regions
        );
    }
}
