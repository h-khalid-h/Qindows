//! # Silo IPC Router Cap Bridge (Phase 167)
//!
//! ## Architecture Guardian: The Gap
//! `silo_ipc_router.rs` implements `SiloIpcRouter`:
//! - `route(msg: IpcMessage, forge: &mut CapTokenForge, qring, tick)` → bool
//!   (already checks Ipc cap internally with CAP_WRITE — but does NOT block
//!    messages to kernel Silos with ID < 16)
//! - `drain_pending(qring, tick)`
//!
//! **Missing link**: SiloIpcRouter::route() already gates Ipc:WRITE,
//! but does NOT block access to kernel-reserved Silo IDs (< 16).
//! Message spoofing to kernel Silo 0 (the init Silo) was possible.
//!
//! This module provides `SiloIpcRouterCapBridge`:
//! 1. `route_guarded()` — pre-checks kernel Silo protection (Admin:EXEC required)
//!    then delegates to router.route() for the standard Ipc:WRITE check
//! 2. `drain()` — forward to drain_pending()

extern crate alloc;

use crate::silo_ipc_router::{SiloIpcRouter, IpcMessage};
use crate::qring_async::QRingProcessor;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct IpcRouterBridgeStats {
    pub kernel_blocked: u64,
    pub routed:         u64,
}

pub struct SiloIpcRouterCapBridge {
    pub router: SiloIpcRouter,
    pub stats:  IpcRouterBridgeStats,
}

impl SiloIpcRouterCapBridge {
    pub fn new() -> Self {
        SiloIpcRouterCapBridge { router: SiloIpcRouter::new(), stats: IpcRouterBridgeStats::default() }
    }

    /// Route with kernel Silo protection. Blocking kernel Silo (ID < 16) access
    /// requires Admin:EXEC cap on top of the router's Ipc:WRITE check.
    pub fn route_guarded(
        &mut self,
        msg: IpcMessage,
        forge: &mut CapTokenForge,
        qring: &mut QRingProcessor,
        tick: u64,
    ) -> bool {
        if msg.to_silo < 16 && !forge.check(msg.from_silo, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.kernel_blocked += 1;
            crate::serial_println!(
                "[IPC ROUTER] Silo {} → kernel Silo {} BLOCKED (no Admin:EXEC)",
                msg.from_silo, msg.to_silo
            );
            return false;
        }
        self.stats.routed += 1;
        self.router.route(msg, forge, qring, tick)
    }

    /// Drain pending IPC messages from Q-Ring.
    pub fn drain(&mut self, qring: &mut QRingProcessor, tick: u64) {
        self.router.drain_pending(qring, tick);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IpcRouterBridge: routed={} kernel_blocked={}",
            self.stats.routed, self.stats.kernel_blocked
        );
    }
}
