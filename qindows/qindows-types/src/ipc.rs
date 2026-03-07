//! # IPC Types
//!
//! Inter-Process Communication types for Q-Ring async message passing.
//! All Silo-to-Silo and Silo-to-Kernel communication uses these types.

use alloc::vec::Vec;

/// IPC channel identifier.
pub type ChannelId = u64;

/// IPC message header.
#[derive(Debug, Clone)]
pub struct QMessage {
    /// Source Silo
    pub from: super::silo::SiloId,
    /// Destination Silo (0 = kernel)
    pub to: super::silo::SiloId,
    /// Channel this message was sent on
    pub channel: ChannelId,
    /// Message type tag
    pub msg_type: MessageType,
    /// Payload bytes
    pub payload: Vec<u8>,
    /// Sequence number (for ordering)
    pub seq: u64,
}

/// Message type categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Request (expects a reply)
    Request,
    /// Reply to a request
    Reply,
    /// One-way notification
    Notify,
    /// Error response
    Error,
    /// Control message (open/close channel)
    Control,
}

/// Q-Ring buffer header (shared memory region for zero-copy IPC).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct QRingHeader {
    /// Total ring buffer size in bytes
    pub size: u32,
    /// Write cursor (producer advances this)
    pub head: u32,
    /// Read cursor (consumer advances this)
    pub tail: u32,
    /// Number of messages in the ring
    pub count: u32,
    /// Ring state flags
    pub flags: u32,
}

impl QRingHeader {
    /// Is the ring empty?
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    /// Is the ring full?
    pub fn is_full(&self) -> bool {
        ((self.head + 1) % self.size) == self.tail
    }

    /// Available space for writing.
    pub fn available(&self) -> u32 {
        if self.head >= self.tail {
            self.size - self.head + self.tail - 1
        } else {
            self.tail - self.head - 1
        }
    }
}
