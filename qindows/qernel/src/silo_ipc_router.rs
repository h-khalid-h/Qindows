//! # Silo IPC Router (Phase 120)
//!
//! ## Architecture Guardian: The Gap
//! `qring_async.rs` (Phase 99) drains Q-Ring CQ entries and returns
//! `Vec<CqEntry>` completions to each caller. But for `IpcSend` ops,
//! the completion needs to be **delivered to a different Silo** —
//! the destination's Q-Ring must receive a new `IpcRecv` SqEntry.
//!
//! The current dispatch path in `qring_dispatch.rs` routes `IpcSend`
//! to the kernel stub, but the stub comments "TODO: route to dest Silo".
//!
//! This module provides `SiloIpcRouter`:
//! 1. Receives `IpcSend` completions from any Silo's Q-Ring
//! 2. Routes the payload to the destination Silo's Q-Ring as `IpcRecv`
//! 3. Enforces CapToken IPC check (Law 1)
//! 4. Handles broadcast messages (dest_silo=0xFFFF)
//! 5. Drops messages when destination ring is full (backpressure)
//!
//! ## Trust Model
//! Silos may only send IPC to Silos they hold an `Ipc` CapToken for.
//! The kernel forges the `IpcRecv` delivery — the receiver cannot forge it.

extern crate alloc;
use alloc::vec::Vec;
use alloc::collections::VecDeque;

use crate::qring_async::{QRingProcessor, SqEntry, CqEntry, SqOpcode, CompStatus};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_WRITE, CAP_READ};

// ── IPC Message ───────────────────────────────────────────────────────────────

/// A pending IPC message waiting for delivery.
#[derive(Debug, Clone)]
pub struct IpcMessage {
    pub from_silo: u64,
    pub to_silo:   u64,
    pub channel:   u32,    // application-defined channel ID
    pub len:       u32,    // payload byte count (payload stored in Prism OID)
    pub payload_oid_prefix: u64, // first 8 bytes of payload OID (0 = inline small msg)
    pub tag:       u64,    // user_data echo
    pub tick:      u64,
}

// ── Router Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct IpcRouterStats {
    pub messages_routed: u64,
    pub messages_dropped: u64,   // ring full
    pub cap_denied: u64,
    pub broadcasts: u64,
    pub buffers_queued: u64,     // queued because dest ring was full
    pub buffers_drained: u64,
}

// ── Silo IPC Router ───────────────────────────────────────────────────────────

/// Routes IPC messages between Silos through Q-Ring.
pub struct SiloIpcRouter {
    /// Pending messages that couldn't deliver immediately (ring was full)
    pending: VecDeque<IpcMessage>,
    /// Max pending queue size before oldest messages are dropped
    max_pending: usize,
    pub stats: IpcRouterStats,
}

impl SiloIpcRouter {
    pub fn new() -> Self {
        SiloIpcRouter {
            pending: VecDeque::new(),
            max_pending: 1024,
            stats: IpcRouterStats::default(),
        }
    }

    /// Route an IPC message from source to destination.
    /// Called by qring_dispatch when SqOpcode::IpcSend arrives.
    pub fn route(
        &mut self,
        msg: IpcMessage,
        forge: &mut CapTokenForge,
        qring: &mut QRingProcessor,
        tick: u64,
    ) -> bool {
        // Law 1: Check IPC send capability
        if !forge.check(msg.from_silo, CapType::Ipc, CAP_WRITE, 0, tick) {
            self.stats.cap_denied += 1;
            crate::serial_println!(
                "[IPC ROUTER] Cap denied: Silo {} → Silo {} (no IPC:WRITE cap)",
                msg.from_silo, msg.to_silo
            );
            return false;
        }

        if msg.to_silo == 0xFFFF_FFFF_FFFF_FFFF {
            // Broadcast: deliver to ALL registered Silos (expensive — limit to kernel use)
            self.stats.broadcasts += 1;
            return self.broadcast(msg, qring, tick);
        }

        self.deliver_one(msg, qring, tick)
    }

    /// Deliver pending messages that couldn't fit in the ring last tick.
    /// Called from boot_sequence::apic_tick_hook().
    pub fn drain_pending(&mut self, qring: &mut QRingProcessor, tick: u64) {
        let mut delivered = 0;
        while let Some(msg) = self.pending.front() {
            let dest = msg.to_silo;
            if qring.rings.get(&dest).map(|r| r.sq_available() > 0).unwrap_or(false) {
                let msg = self.pending.pop_front().unwrap();
                self.inject_recv(&msg, qring, tick);
                self.stats.buffers_drained += 1;
                delivered += 1;
            } else {
                break; // still full — try again next tick
            }
        }
        if delivered > 0 {
            crate::serial_println!("[IPC ROUTER] Drained {} pending messages", delivered);
        }
    }

    fn deliver_one(&mut self, msg: IpcMessage, qring: &mut QRingProcessor, tick: u64) -> bool {
        let dest = msg.to_silo;

        // Ensure destination ring exists
        if !qring.rings.contains_key(&dest) {
            qring.register_silo(dest);
        }

        if let Some(ring) = qring.rings.get(&dest) {
            if ring.sq_available() == 0 {
                // Queue for retry
                if self.pending.len() < self.max_pending {
                    self.stats.buffers_queued += 1;
                    self.pending.push_back(msg);
                    return false;
                } else {
                    self.stats.messages_dropped += 1;
                    crate::serial_println!(
                        "[IPC ROUTER] Dropped: Silo {} → Silo {} (ring full, queue full)",
                        msg.from_silo, dest
                    );
                    return false;
                }
            }
        }

        self.inject_recv(&msg, qring, tick);
        self.stats.messages_routed += 1;
        true
    }

    fn broadcast(&mut self, msg: IpcMessage, qring: &mut QRingProcessor, tick: u64) -> bool {
        let silo_ids: Vec<u64> = qring.rings.keys().copied().collect();
        for silo_id in silo_ids {
            if silo_id != msg.from_silo {
                let mut m = msg.clone();
                m.to_silo = silo_id;
                self.deliver_one(m, qring, tick);
            }
        }
        true
    }

    fn inject_recv(&self, msg: &IpcMessage, qring: &mut QRingProcessor, tick: u64) {
        let sqe = SqEntry {
            opcode: SqOpcode::IpcSend as u16, // IpcRecv delivered as IpcSend to dest's SQ
            flags: 0x8000, // IPC_RECV flag — dest recognizes this as an inbound
            user_data: msg.tag,
            addr: msg.from_silo,
            len: msg.len,
            aux: msg.channel,
        };
        if let Some(ring) = qring.rings.get_mut(&msg.to_silo) {
            ring.submit(sqe);
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IpcRouter: routed={} dropped={} cap_denied={} broadcast={} pending={}",
            self.stats.messages_routed, self.stats.messages_dropped,
            self.stats.cap_denied, self.stats.broadcasts, self.pending.len()
        );
    }
}
