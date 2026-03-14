//! # Fiber IPC — Q-Ring Manager + Channel API
//!
//! Manages both:
//! 1. **Q-Ring lock-free SPSC buffers** (Phase 11) for zero-copy fiber IPC
//! 2. **IPC Channels** (backward compat) for Silo-to-Silo message passing
//!
//! ## Handle Model
//!
//! A `QRingHandle` is an opaque `u64` that maps to an internal ring buffer.
//! Handles are per-Silo — a fiber cannot access rings belonging to another Silo.

#![allow(dead_code)]

pub mod qring;
pub mod batch;

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use qring::{QRing, QRingError};

// ── Message Types (used by syscall/mod.rs) ───────────────────────────

/// IPC message type tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Raw data transfer
    Data,
    /// Capability token transfer
    CapTransfer,
    /// One-way notification
    Notification,
    /// Filesystem request
    FsRequest,
    /// Graphics compositor command
    GfxCommand,
    /// Aether UI composition event
    AetherEvent,
    /// Shutdown signal
    Shutdown,
}

/// Message payload variants.
#[derive(Debug, Clone)]
pub enum MessagePayload {
    /// No payload
    Empty,
    /// Raw bytes
    Bytes(Vec<u8>),
    /// String payload
    Text(String),
}

/// An IPC message sent between Silos.
#[derive(Debug, Clone)]
pub struct QMessage {
    /// Message type tag
    pub msg_type: MessageType,
    /// Sender Silo ID
    pub sender: u64,
    /// Payload
    pub payload: MessagePayload,
    /// Timestamp (boot ticks)
    pub timestamp: u64,
}

// ── IPC Channel (Silo-to-Silo bidirectional) ─────────────────────────

/// One direction of an IPC channel ring.
pub struct ChannelRing {
    /// Silo that writes to this ring
    pub producer_silo: u64,
    /// Silo that reads from this ring
    pub consumer_silo: u64,
    /// Message queue (FIFO)
    messages: Vec<QMessage>,
}

impl ChannelRing {
    fn new(producer: u64, consumer: u64) -> Self {
        ChannelRing {
            producer_silo: producer,
            consumer_silo: consumer,
            messages: Vec::new(),
        }
    }

    fn push(&mut self, msg: QMessage) -> bool {
        if self.messages.len() >= 256 { return false; } // backpressure
        self.messages.push(msg);
        true
    }

    fn drain(&mut self, max: usize) -> Vec<QMessage> {
        let n = max.min(self.messages.len());
        self.messages.drain(..n).collect()
    }
}

/// A bidirectional IPC channel between two Silos.
pub struct IpcChannel {
    /// Channel ID
    pub id: u64,
    /// A→B direction ring
    pub ring_ab: ChannelRing,
    /// B→A direction ring
    pub ring_ba: ChannelRing,
}

impl IpcChannel {
    /// Send a message from A to B.
    pub fn send_to_b(&mut self, msg: QMessage) -> bool {
        self.ring_ab.push(msg)
    }

    /// Send a message from B to A.
    pub fn send_to_a(&mut self, msg: QMessage) -> bool {
        self.ring_ba.push(msg)
    }

    /// Receive messages destined for B (sent by A).
    pub fn recv_for_b(&mut self, max: usize) -> Vec<QMessage> {
        self.ring_ab.drain(max)
    }

    /// Receive messages destined for A (sent by B).
    pub fn recv_for_a(&mut self, max: usize) -> Vec<QMessage> {
        self.ring_ba.drain(max)
    }
}

// ── Q-Ring Lock-Free Buffer Manager ──────────────────────────────────

/// Default ring buffer size: 4 KiB (one page).
const DEFAULT_RING_SIZE: usize = 4096;

/// Opaque handle to a Q-Ring instance.
pub type QRingHandle = u64;

/// Metadata for a managed Q-Ring.
struct ManagedRing {
    ring: QRing<DEFAULT_RING_SIZE>,
    producer_fiber: u64,
    consumer_fiber: u64,
    silo_id: u64,
    open: bool,
    bytes_sent: u64,
    bytes_recv: u64,
}

/// The Q-Ring Manager — allocates and manages lock-free ring buffers.
pub struct QRingManager {
    rings: BTreeMap<QRingHandle, ManagedRing>,
    next_handle: QRingHandle,
    pub total_created: u64,
    pub total_open: u64,
}

impl QRingManager {
    pub fn new() -> Self {
        QRingManager {
            rings: BTreeMap::new(),
            next_handle: 1,
            total_created: 0,
            total_open: 0,
        }
    }

    pub fn create(&mut self, producer_fiber: u64, consumer_fiber: u64, silo_id: u64) -> QRingHandle {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.rings.insert(handle, ManagedRing {
            ring: QRing::new(), producer_fiber, consumer_fiber, silo_id,
            open: true, bytes_sent: 0, bytes_recv: 0,
        });
        self.total_created += 1;
        self.total_open += 1;
        handle
    }

    pub fn send(&mut self, handle: QRingHandle, caller: u64, data: &[u8]) -> Result<usize, QRingError> {
        let m = self.rings.get_mut(&handle).ok_or(QRingError::InvalidHandle)?;
        if !m.open { return Err(QRingError::Closed); }
        if m.producer_fiber != caller { return Err(QRingError::InvalidHandle); }
        let n = m.ring.send(data)?;
        m.bytes_sent += n as u64;
        Ok(n)
    }

    pub fn recv(&mut self, handle: QRingHandle, caller: u64, buf: &mut [u8]) -> Result<usize, QRingError> {
        let m = self.rings.get_mut(&handle).ok_or(QRingError::InvalidHandle)?;
        if !m.open { return Err(QRingError::Closed); }
        if m.consumer_fiber != caller { return Err(QRingError::InvalidHandle); }
        let n = m.ring.recv(buf)?;
        m.bytes_recv += n as u64;
        Ok(n)
    }

    pub fn close(&mut self, handle: QRingHandle) {
        if let Some(m) = self.rings.get_mut(&handle) {
            m.open = false;
            self.total_open = self.total_open.saturating_sub(1);
        }
    }

    pub fn close_silo(&mut self, silo_id: u64) {
        for m in self.rings.values_mut() {
            if m.silo_id == silo_id && m.open {
                m.open = false;
                self.total_open = self.total_open.saturating_sub(1);
            }
        }
    }

    pub fn close_fiber(&mut self, fiber_id: u64) {
        for m in self.rings.values_mut() {
            if m.open && (m.producer_fiber == fiber_id || m.consumer_fiber == fiber_id) {
                m.open = false;
                self.total_open = self.total_open.saturating_sub(1);
            }
        }
    }

    pub fn list_for_fiber(&self, fiber_id: u64) -> Vec<QRingHandle> {
        self.rings.iter()
            .filter(|(_, m)| m.open && (m.producer_fiber == fiber_id || m.consumer_fiber == fiber_id))
            .map(|(&h, _)| h)
            .collect()
    }
}

// ── IpcManager (unified API) ─────────────────────────────────────────

/// The IPC Manager — unified interface for channels + Q-Ring buffers.
pub struct IpcManager {
    /// Bidirectional Silo-to-Silo channels
    pub channels: BTreeMap<u64, IpcChannel>,
    /// Next channel ID
    next_channel_id: u64,
    /// Q-Ring lock-free buffer manager (Phase 11)
    pub rings: QRingManager,
}

impl IpcManager {
    pub fn new() -> Self {
        IpcManager {
            channels: BTreeMap::new(),
            next_channel_id: 1,
            rings: QRingManager::new(),
        }
    }

    /// Create a bidirectional IPC channel between two Silos.
    pub fn create_channel(
        &mut self,
        silo_a: u64,
        silo_b: u64,
        _cap: &crate::capability::CapToken,
    ) -> u64 {
        let id = self.next_channel_id;
        self.next_channel_id += 1;
        self.channels.insert(id, IpcChannel {
            id,
            ring_ab: ChannelRing::new(silo_a, silo_b),
            ring_ba: ChannelRing::new(silo_b, silo_a),
        });
        id
    }

    /// Get a mutable reference to a channel by ID.
    pub fn get_channel(&mut self, id: u64) -> Option<&mut IpcChannel> {
        self.channels.get_mut(&id)
    }
}
