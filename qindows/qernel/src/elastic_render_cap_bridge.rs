//! # Elastic Render Cap Bridge (Phase 203)
//!
//! ## Architecture Guardian: The Gap
//! `elastic_render.rs` implements Q-Server GPU offload:
//! - `SceneGraph` — GPU render scene, `wire_size_bytes()`, `heavy_node_count()`
//! - `GpuThermalState::from_temp_millideg(temp)` → state
//! - `GpuThermalState::should_offload()` → bool
//!
//! **Missing link**: Elastic rendering (GPU offload to Q-Server clusters)
//! could be initiated without a Network:EXEC cap. Q-Server offload sends
//! data to remote GPU nodes — any Silo could exfiltrate via render payload.
//!
//! This module provides `ElasticRenderCapBridge`:
//! Network:EXEC cap required before elastic render offload is allowed.

extern crate alloc;

use crate::elastic_render::SceneGraph;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct ElasticRenderCapStats {
    pub offloads_allowed: u64,
    pub offloads_denied:  u64,
    pub hot_throttled:    u64,
}

pub struct ElasticRenderCapBridge {
    pub stats: ElasticRenderCapStats,
}

impl ElasticRenderCapBridge {
    pub fn new() -> Self {
        ElasticRenderCapBridge { stats: ElasticRenderCapStats::default() }
    }

    /// Authorize an elastic render offload — requires Network:EXEC cap.
    pub fn authorize_offload(
        &mut self,
        silo_id: u64,
        scene: &SceneGraph,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Network, CAP_EXEC, 0, tick) {
            self.stats.offloads_denied += 1;
            crate::serial_println!("[ELASTIC RENDER] Silo {} offload denied — no Network:EXEC cap", silo_id);
            return false;
        }
        self.stats.offloads_allowed += 1;
        crate::serial_println!(
            "[ELASTIC RENDER] Silo {} offload authorized: {} bytes, {} heavy nodes",
            silo_id, scene.wire_size_bytes(), scene.heavy_node_count()
        );
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  ElasticRenderBridge: allowed={} denied={} throttled={}",
            self.stats.offloads_allowed, self.stats.offloads_denied, self.stats.hot_throttled
        );
    }
}
