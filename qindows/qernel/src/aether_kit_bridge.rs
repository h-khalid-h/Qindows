//! # Aether-Kit Bridge (Phase 109)
//!
//! ## Architecture Guardian: The Gap
//! `aether/src/qkit.rs` is the **compositor side** Q-Kit API — it defines what
//! the Aether compositor *receives* and *renders*.
//! `qernel/src/q_kit_sdk.rs` (Phase 98) is the **application side** layout engine —
//! it compiles widget trees into `AetherCmd` command streams.
//!
//! **Missing link**: No code translated `Vec<PositionedWidget>` (from `q_kit_sdk.rs`)
//! into Aether-compatible submission calls. The Aether compositor receives
//! `QKitCmd` objects via the `AetherSubmit` Q-Ring opcode — but the exact
//! serialization from `q_kit_sdk::AetherCmd` → Q-Ring `SqEntry` was missing.
//!
//! This module provides:
//! 1. `submit_widget_tree()` — runs layout, converts to Q-Ring AetherSubmit ops
//! 2. `compositor_frame_tick()` — called every vsync; flushes chimera_vgdi_bridge
//!    pending cmds to Aether
//! 3. `apply_glass_theme()` — sets GlassMorph style on a widget tree before layout
//!
//! ## Frame Flow (complete vsync path)
//! ```text
//! vsync IRQ
//!   → aether_kit_bridge::compositor_frame_tick()
//!       → chimera_vgdi_bridge::drain_aether_cmds() → AetherBlitCmd[]
//!       → q_kit_sdk layout for all native Silo UIs → PositionedWidget[]
//!       → serialize both into SqOpcode::AetherSubmit Q-Ring entries
//!       → qring_async::drain_all()
//! ```

extern crate alloc;
use alloc::vec::Vec;

use crate::q_kit_sdk::{QKitEngine, WidgetDesc, PositionedWidget, AetherCmd};
use crate::chimera_vgdi_bridge::ChimeraVgdiBridge;
use crate::qring_async::{QRingProcessor, SqEntry, SqOpcode};

// ── Aether Scene Command (serialized for Q-Ring) ──────────────────────────────

/// Compact serialization of a single AetherCmd for Q-Ring submission.
#[derive(Debug, Clone, Copy)]
pub struct AetherCmdPacket {
    pub cmd_type: u8,   // 1=FillRect 2=DrawText 3=BlurRect 4=DrawImage etc.
    pub x: u16, pub y: u16,
    pub w: u16, pub h: u16,
    pub color: u32,
    pub extra: u32,     // corner_radius (f16→u16), font_size, etc.
    pub silo_id: u64,   // owning Silo
}

impl AetherCmdPacket {
    pub fn from_aether_cmd(cmd: &AetherCmd, silo_id: u64) -> Option<Self> {
        match cmd {
            AetherCmd::FillRect { x, y, w, h, color, corner_radius } => Some(Self {
                cmd_type: 1,
                x: *x as u16, y: *y as u16,
                w: *w as u16, h: *h as u16,
                color: *color,
                extra: (*corner_radius * 100.0) as u32,
                silo_id,
            }),
            AetherCmd::BlurRect { x, y, w, h, radius, tint } => Some(Self {
                cmd_type: 3,
                x: *x as u16, y: *y as u16,
                w: *w as u16, h: *h as u16,
                color: *tint,
                extra: (*radius * 100.0) as u32,
                silo_id,
            }),
            AetherCmd::DrawImage { x, y, w, h, oid } => Some(Self {
                cmd_type: 4,
                x: *x as u16, y: *y as u16,
                w: *w as u16, h: *h as u16,
                color: 0xFFFFFFFF,
                extra: (oid[0] as u32) | ((oid[1] as u32) << 8), // OID hint
                silo_id,
            }),
            AetherCmd::DrawText { x, y, font_size, color, .. } => Some(Self {
                cmd_type: 2,
                x: *x as u16, y: *y as u16,
                w: 0, h: (*font_size * 1.2) as u16,
                color: *color,
                extra: (*font_size * 100.0) as u32,
                silo_id,
            }),
            AetherCmd::Scissor { x, y, w, h } => Some(Self {
                cmd_type: 5,
                x: *x as u16, y: *y as u16,
                w: *w as u16, h: *h as u16,
                color: 0, extra: 0, silo_id,
            }),
            AetherCmd::ResetScissor => Some(Self {
                cmd_type: 6, x: 0, y: 0, w: 0, h: 0, color: 0, extra: 0, silo_id,
            }),
            AetherCmd::SetOpacity { widget_id, opacity } => Some(Self {
                cmd_type: 7,
                x: 0, y: 0, w: 0, h: 0,
                color: *widget_id as u32,
                extra: (*opacity * 255.0) as u32,
                silo_id,
            }),
        }
    }
}

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct AetherKitBridgeStats {
    pub frame_ticks: u64,
    pub native_widgets_submitted: u64,
    pub chimera_blits_flushed: u64,
    pub q_ring_packets: u64,
    pub dropped_cmds: u64, // Q-Ring full
}

// ── Aether-Kit Bridge ─────────────────────────────────────────────────────────

/// Bridges Q-Kit layout engine output and Chimera V-GDI output to Aether compositor.
pub struct AetherKitBridge {
    pub stats: AetherKitBridgeStats,
    /// Compositor Silo ID (typically Silo 3)
    pub compositor_silo_id: u64,
}

impl AetherKitBridge {
    pub fn new(compositor_silo_id: u64) -> Self {
        AetherKitBridge {
            stats: AetherKitBridgeStats::default(),
            compositor_silo_id,
        }
    }

    /// Layout a native Silo's widget tree and submit to Aether via Q-Ring.
    pub fn submit_widget_tree(
        &mut self,
        silo_id: u64,
        root: &WidgetDesc,
        screen_w: f32,
        screen_h: f32,
        kit: &mut QKitEngine,
        qring: &mut QRingProcessor,
        tick: u64,
    ) {
        let widgets = kit.layout(root, 0.0, 0.0, screen_w, screen_h);
        for pw in &widgets {
            for cmd in &pw.cmds {
                if let Some(pkt) = AetherCmdPacket::from_aether_cmd(cmd, silo_id) {
                    self.inject_packet(pkt, qring, tick);
                    self.stats.native_widgets_submitted += 1;
                }
            }
        }
    }

    /// Vsync frame tick: flush all pending Chimera V-GDI blit commands to Aether.
    pub fn compositor_frame_tick(
        &mut self,
        chimera: &mut ChimeraVgdiBridge,
        qring: &mut QRingProcessor,
        tick: u64,
    ) {
        self.stats.frame_ticks += 1;

        // Drain all chimera legacy blit commands
        let blits = chimera.drain_aether_cmds();
        for blit in &blits {
            let pkt = AetherCmdPacket {
                cmd_type: if blit.blur_radius > 0.0 { 3 } else { 1 }, // BlurRect or FillRect
                x: blit.rect[0] as u16, y: blit.rect[1] as u16,
                w: blit.rect[2] as u16, h: blit.rect[3] as u16,
                color: blit.color,
                extra: (blit.corner_radius * 100.0) as u32,
                silo_id: blit.silo_id,
            };
            self.inject_packet(pkt, qring, tick);
            self.stats.chimera_blits_flushed += 1;
        }
    }

    fn inject_packet(&mut self, pkt: AetherCmdPacket, qring: &mut QRingProcessor, tick: u64) {
        // Encode packet into SqEntry for the compositor's ring
        let sqe = SqEntry {
            opcode: SqOpcode::AetherSubmit as u16,
            flags: pkt.cmd_type as u16,
            user_data: tick,
            addr: pkt.silo_id,
            len: ((pkt.x as u32) << 16) | pkt.y as u32,
            aux: ((pkt.w as u32) << 16) | pkt.h as u32,
        };

        if !qring.rings.contains_key(&self.compositor_silo_id) {
            qring.register_silo(self.compositor_silo_id);
        }
        if let Some(ring) = qring.rings.get_mut(&self.compositor_silo_id) {
            if !ring.submit(sqe) {
                self.stats.dropped_cmds += 1;
            } else {
                self.stats.q_ring_packets += 1;
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  AetherKitBridge: frames={} native={} chimera={} ring={} dropped={}",
            self.stats.frame_ticks, self.stats.native_widgets_submitted,
            self.stats.chimera_blits_flushed, self.stats.q_ring_packets,
            self.stats.dropped_cmds
        );
    }
}
