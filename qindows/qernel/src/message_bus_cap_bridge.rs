//! # Message Bus CapToken Bridge (Phase 148)
//!
//! ## Architecture Guardian: The Gap
//! `ipc/message_bus.rs` implements `MessageBus` with `send()` and `request()`
//! that never checked IPC CapTokens — any Silo could message any other.
//!
//! **Note**: `message_bus.rs` is not re-exported from `ipc/mod.rs`.
//! This bridge provides the CapToken guard logic as a standalone checker
//! that can be called before dispatching to MessageBus, and also documents
//! the path needed to properly integrate MessageBus in the future.
//!
//! **Law 1**: Silo-to-Silo IPC requires Ipc:EXEC CapToken.

extern crate alloc;

use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct MsgBusBridgeStats {
    pub checks_passed: u64,
    pub checks_denied: u64,
    pub total_checked: u64,
}

// ── Message Bus Cap Bridge ────────────────────────────────────────────────────

/// CapToken gate for IPC MessageBus sends. Call before MessageBus::send()/request().
pub struct MessageBusCapBridge {
    pub stats: MsgBusBridgeStats,
}

impl MessageBusCapBridge {
    pub fn new() -> Self {
        MessageBusCapBridge { stats: MsgBusBridgeStats::default() }
    }

    /// Check if a Silo may send an IPC message (Law 1: Ipc:EXEC required).
    /// Returns true if allowed, false if blocked.
    pub fn check_send(
        &mut self,
        from_silo: u64,
        to_silo: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        self.stats.total_checked += 1;
        if forge.check(from_silo, CapType::Ipc, CAP_EXEC, 0, tick) {
            self.stats.checks_passed += 1;
            true
        } else {
            self.stats.checks_denied += 1;
            crate::serial_println!(
                "[MSGBUS CAP] Silo {} denied IPC → {} (no Ipc:EXEC, Law 1)",
                from_silo, to_silo
            );
            false
        }
    }

    /// Check if a Silo may make an IPC request (same cap required).
    pub fn check_request(
        &mut self,
        from_silo: u64,
        to_silo: u64,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        self.check_send(from_silo, to_silo, forge, tick)
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  MsgBusBridge: total={} passed={} denied={}",
            self.stats.total_checked, self.stats.checks_passed, self.stats.checks_denied
        );
    }
}
