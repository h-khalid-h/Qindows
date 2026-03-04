//! # Q-Ring IPC — Async Ring Buffer Communication
//!
//! The backbone of microkernel communication. Every interaction
//! between Silos flows through Q-Rings — lock-free, asynchronous
//! ring buffers that avoid the traditional microkernel IPC tax.
//!
//! Design: Single-Producer Single-Consumer (SPSC) rings with
//! up to 50 messages batched in a single kernel trip.

use core::sync::atomic::{AtomicU64, Ordering};
use alloc::vec::Vec;
use crate::capability::{CapToken, Permissions};

/// Size of the ring buffer (must be power of 2).
const RING_SIZE: usize = 256;

/// A single message in the Q-Ring.
#[derive(Debug, Clone)]
pub struct QMessage {
    /// Message type discriminator
    pub msg_type: MessageType,
    /// Source Silo ID
    pub sender: u64,
    /// Payload — inline small data or a Prism OID for large payloads
    pub payload: MessagePayload,
    /// Timestamp (scheduler tick)
    pub timestamp: u64,
}

/// Message types flowing through Q-Rings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Raw data transfer
    Data,
    /// Capability token transfer (delegation)
    CapTransfer,
    /// Notification (no payload, just a signal)
    Notification,
    /// File system request (read/write/open)
    FsRequest,
    /// File system response
    FsResponse,
    /// Network packet
    NetPacket,
    /// Graphics command (for Aether)
    GfxCommand,
    /// Device driver request
    DeviceRequest,
    /// Shutdown / cleanup signal
    Shutdown,
}

/// Message payload — either inline data or a reference to larger data.
#[derive(Debug, Clone)]
pub enum MessagePayload {
    /// Small inline data (≤ 64 bytes)
    Inline([u8; 64]),
    /// Reference to a Prism Object by OID
    ObjectRef { oid: [u8; 32] },
    /// Capability token being transferred
    Capability(CapToken),
    /// No payload (for notifications)
    Empty,
}

/// Q-Ring: Lock-free SPSC ring buffer.
///
/// One Silo writes (producer), another reads (consumer).
/// No locks — uses atomic head/tail pointers.
pub struct QRing {
    /// Message buffer
    buffer: Vec<Option<QMessage>>,
    /// Write position (owned by producer)
    head: AtomicU64,
    /// Read position (owned by consumer)
    tail: AtomicU64,
    /// Producer's Silo ID
    pub producer_silo: u64,
    /// Consumer's Silo ID
    pub consumer_silo: u64,
}

impl QRing {
    /// Create a new Q-Ring between two Silos.
    pub fn new(producer: u64, consumer: u64) -> Self {
        let mut buffer = Vec::with_capacity(RING_SIZE);
        for _ in 0..RING_SIZE {
            buffer.push(None);
        }

        QRing {
            buffer,
            head: AtomicU64::new(0),
            tail: AtomicU64::new(0),
            producer_silo: producer,
            consumer_silo: consumer,
        }
    }

    /// Push a message into the ring (producer side).
    ///
    /// Returns false if the ring is full (consumer is too slow).
    pub fn push(&mut self, msg: QMessage) -> bool {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);
        let next_head = (head + 1) % RING_SIZE as u64;

        if next_head == tail {
            return false; // Ring full
        }

        self.buffer[head as usize] = Some(msg);
        self.head.store(next_head, Ordering::Release);
        true
    }

    /// Pop a message from the ring (consumer side).
    pub fn pop(&mut self) -> Option<QMessage> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        if tail == head {
            return None; // Ring empty
        }

        let msg = self.buffer[tail as usize].take();
        self.tail.store((tail + 1) % RING_SIZE as u64, Ordering::Release);
        msg
    }

    /// Batch pop: drain up to `max` messages at once.
    ///
    /// This is the Q-Ring's main advantage: 50 messages
    /// in a single kernel trip, avoiding per-message syscall overhead.
    pub fn drain(&mut self, max: usize) -> Vec<QMessage> {
        let mut batch = Vec::with_capacity(max);
        for _ in 0..max {
            match self.pop() {
                Some(msg) => batch.push(msg),
                None => break,
            }
        }
        batch
    }

    /// Check how many messages are pending.
    pub fn pending(&self) -> u64 {
        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Relaxed);
        if head >= tail {
            head - tail
        } else {
            RING_SIZE as u64 - tail + head
        }
    }

    /// Is the ring empty?
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Relaxed) == self.tail.load(Ordering::Relaxed)
    }
}

/// A bidirectional IPC channel — two Q-Rings (one in each direction).
pub struct QChannel {
    /// Messages from A → B
    pub ring_ab: QRing,
    /// Messages from B → A
    pub ring_ba: QRing,
    /// Channel identifier
    pub channel_id: u64,
}

impl QChannel {
    /// Create a new bidirectional channel between two Silos.
    pub fn create(silo_a: u64, silo_b: u64) -> Self {
        static NEXT_CHAN: core::sync::atomic::AtomicU64 =
            core::sync::atomic::AtomicU64::new(1);

        QChannel {
            ring_ab: QRing::new(silo_a, silo_b),
            ring_ba: QRing::new(silo_b, silo_a),
            channel_id: NEXT_CHAN.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// Send a message from Silo A to Silo B.
    pub fn send_to_b(&mut self, msg: QMessage) -> bool {
        self.ring_ab.push(msg)
    }

    /// Send a message from Silo B to Silo A.
    pub fn send_to_a(&mut self, msg: QMessage) -> bool {
        self.ring_ba.push(msg)
    }

    /// Receive messages for Silo B (from A).
    pub fn recv_for_b(&mut self, max: usize) -> Vec<QMessage> {
        self.ring_ab.drain(max)
    }

    /// Receive messages for Silo A (from B).
    pub fn recv_for_a(&mut self, max: usize) -> Vec<QMessage> {
        self.ring_ba.drain(max)
    }
}

/// IPC Manager — tracks all active channels.
pub struct IpcManager {
    pub channels: Vec<QChannel>,
}

impl IpcManager {
    pub const fn new() -> Self {
        IpcManager {
            channels: Vec::new(),
        }
    }

    /// Create a new channel between two Silos.
    ///
    /// Requires the calling Silo to hold SPAWN capability.
    pub fn create_channel(
        &mut self,
        silo_a: u64,
        silo_b: u64,
        cap: &CapToken,
    ) -> Result<u64, &'static str> {
        if !cap.has_permission(Permissions::SPAWN) {
            return Err("IPC channel creation requires SPAWN capability");
        }

        let channel = QChannel::create(silo_a, silo_b);
        let id = channel.channel_id;
        self.channels.push(channel);
        Ok(id)
    }

    /// Find a channel by ID.
    pub fn get_channel(&mut self, id: u64) -> Option<&mut QChannel> {
        self.channels.iter_mut().find(|c| c.channel_id == id)
    }
}
