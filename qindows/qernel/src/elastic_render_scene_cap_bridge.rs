//! # Elastic Render Scene Cap Bridge (Phase 284)
//!
//! ## Architecture Guardian: The Gap
//! `elastic_render.rs` implements `SceneGraph`:
//! - `SceneGraph::wire_size_bytes()` → usize — scene serialization size
//! - `SdfNode { sdf_id, bounds, ... }` — Signed Distance Field node
//! - `GpuThermalState` — from_temp_millideg, should_offload()
//!
//! **Missing link**: SceneGraph serialization had no size cap. A
//! complex scene with millions of SdfNode instances could generate a
//! multi-GB wire representation, saturating the GPU command buffer.
//!
//! This module provides `ElasticRenderSceneCapBridge`:
//! Max 64 MiB SceneGraph wire size per frame.

extern crate alloc;

const MAX_SCENE_WIRE_BYTES: usize = 64 * 1024 * 1024; // 64 MiB

#[derive(Debug, Default, Clone)]
pub struct ElasticSceneCapStats {
    pub frames_ok:    u64,
    pub frames_capped: u64,
}

pub struct ElasticRenderSceneCapBridge {
    pub stats: ElasticSceneCapStats,
}

impl ElasticRenderSceneCapBridge {
    pub fn new() -> Self {
        ElasticRenderSceneCapBridge { stats: ElasticSceneCapStats::default() }
    }

    pub fn authorize_render(&mut self, wire_size: usize, silo_id: u64) -> bool {
        if wire_size > MAX_SCENE_WIRE_BYTES {
            self.stats.frames_capped += 1;
            crate::serial_println!(
                "[ELASTIC RENDER] Silo {} wire_size {} exceeds {} MiB scene cap",
                silo_id, wire_size, MAX_SCENE_WIRE_BYTES / (1024*1024)
            );
            return false;
        }
        self.stats.frames_ok += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ElasticSceneCapBridge: ok={} capped={}", self.stats.frames_ok, self.stats.frames_capped
        );
    }
}
