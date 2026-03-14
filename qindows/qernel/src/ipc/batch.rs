//! # Q-Ring Batch Submission Drainer
//!
//! Handles batch processing of lock-free Q-Ring ring buffers.
//! Instead of triggering a context switch for every single message or capability token
//! transfer, apps write a batch of requests into a ring buffer and "kick" the Qernel once.
//! The Qernel then drains the entire batch in a single pass.

use crate::ipc::{QRingManager, QRingHandle};
use alloc::vec::Vec;

/// Drains messages/requests from a Q-Ring in batches to minimize overhead.
pub struct QRingDrainer;

impl QRingDrainer {
    /// Drain all pending bytes from a specific Q-Ring handle into a newly allocated vector.
    ///
    /// In a fully realized system, this would deserialize the byte stream into
    /// a series of structured requests (e.g., FS read, net send, cap transfer).
    pub fn drain_batch(
        rings: &mut QRingManager,
        handle: QRingHandle,
        caller_fiber: u64,
        max_bytes: usize,
    ) -> Vec<u8> {
        let mut buffer = alloc::vec![0u8; max_bytes];
        match rings.recv(handle, caller_fiber, &mut buffer) {
            Ok(bytes_read) => {
                buffer.truncate(bytes_read);
                buffer
            }
            Err(_) => Vec::new(),
        }
    }

    /// Submit a batch of bytes to a Q-Ring in a single operation.
    pub fn submit_batch(
        rings: &mut QRingManager,
        handle: QRingHandle,
        caller_fiber: u64,
        data: &[u8],
    ) -> usize {
        match rings.send(handle, caller_fiber, data) {
            Ok(bytes_written) => bytes_written,
            Err(_) => 0,
        }
    }
}
