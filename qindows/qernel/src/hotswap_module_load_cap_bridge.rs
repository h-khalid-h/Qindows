//! # Hot-Swap Module Load Cap Bridge (Phase 286)
//!
//! ## Architecture Guardian: The Gap
//! `hotswap.rs` implements `HotSwapManager`:
//! - Module loading/unloading without downtime
//! - `HotSwapManager::apply(module_name, new_code)` — hot-swap a module
//!
//! **Missing link**: Hot-swap operations had no per-session rate cap.
//! Rapid hot-swap could be used as a reliability attack, continuously
//! cycling module states and causing init/teardown race conditions.
//!
//! This module provides `HotSwapModuleLoadCapBridge`:
//! Max 8 hot-swap operations per session (Admin:EXEC required each time).

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

const MAX_HOTSWAPS_PER_SESSION: u64 = 8;

#[derive(Debug, Default, Clone)]
pub struct HotSwapCapStats {
    pub swaps_allowed: u64,
    pub swaps_denied:  u64,
}

pub struct HotSwapModuleLoadCapBridge {
    session_swap_count: u64,
    pub stats:          HotSwapCapStats,
}

impl HotSwapModuleLoadCapBridge {
    pub fn new() -> Self {
        HotSwapModuleLoadCapBridge { session_swap_count: 0, stats: HotSwapCapStats::default() }
    }

    pub fn authorize_hotswap(&mut self, silo_id: u64, forge: &mut CapTokenForge, tick: u64) -> bool {
        if self.session_swap_count >= MAX_HOTSWAPS_PER_SESSION {
            self.stats.swaps_denied += 1;
            crate::serial_println!(
                "[HOTSWAP] Session hot-swap cap reached ({}/{})", self.session_swap_count, MAX_HOTSWAPS_PER_SESSION
            );
            return false;
        }
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.swaps_denied += 1;
            crate::serial_println!("[HOTSWAP] Silo {} denied — Admin:EXEC required", silo_id);
            return false;
        }
        self.session_swap_count += 1;
        self.stats.swaps_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  HotSwapCapBridge: allowed={} denied={}", self.stats.swaps_allowed, self.stats.swaps_denied
        );
    }
}
