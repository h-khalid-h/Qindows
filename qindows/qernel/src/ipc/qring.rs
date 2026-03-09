//! # Q-Ring — Lock-Free SPSC Ring Buffer
//!
//! Zero-copy, single-producer/single-consumer (SPSC) ring buffer
//! for fiber-to-fiber inter-process communication within a Silo.
//!
//! ## Design
//!
//! - **Lock-free**: Uses `AtomicUsize` for head/tail cursors with
//!   `Acquire`/`Release` ordering. No spinlocks, no mutexes.
//! - **Inline storage**: `[u8; N]` is embedded in the struct —
//!   no heap allocation on the hot path.
//! - **Non-blocking**: `send()` and `recv()` never block. Callers
//!   get `Full`/`Empty` errors and decide whether to yield or retry.
//! - **Interrupt-safe**: Safe to call from timer IRQ handlers because
//!   there are no locks to deadlock on.
//!
//! ## Usage
//!
//! ```ignore
//! let ring = QRing::<4096>::new();
//!
//! // Producer fiber
//! ring.send(b"hello").unwrap();
//!
//! // Consumer fiber
//! let mut buf = [0u8; 64];
//! let n = ring.recv(&mut buf).unwrap();
//! // buf[..n] == b"hello"
//! ```

use core::sync::atomic::{AtomicUsize, Ordering};

/// Errors returned by Q-Ring operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QRingError {
    /// Ring buffer is full — producer must wait or drop data.
    Full,
    /// Ring buffer is empty — consumer must wait or yield.
    Empty,
    /// Tried to send more bytes than the ring can ever hold.
    TooLarge,
    /// Invalid handle (ring doesn't exist or caller is wrong fiber).
    InvalidHandle,
    /// The ring has been closed.
    Closed,
}

/// Lock-free SPSC ring buffer with compile-time fixed capacity.
///
/// `N` **must** be a power of 2 for correct masking. The effective
/// capacity is `N - 1` bytes (one slot is a sentinel to distinguish
/// full from empty).
///
/// ## Memory Ordering
///
/// - **Producer** (`send`): reads `tail` with `Acquire`, writes `head` with `Release`
/// - **Consumer** (`recv`): reads `head` with `Acquire`, writes `tail` with `Release`
///
/// This guarantees that data written before `head` advances is visible
/// to the consumer after it observes the new `head`.
pub struct QRing<const N: usize> {
    /// Write cursor (owned by producer, read by consumer).
    head: AtomicUsize,
    /// Read cursor (owned by consumer, read by producer).
    tail: AtomicUsize,
    /// Inline ring buffer storage.
    buffer: [u8; N],
}

impl<const N: usize> QRing<N> {
    /// Create a new, empty Q-Ring.
    ///
    /// # Panics
    ///
    /// Debug-asserts that `N` is a power of 2 and at least 2.
    pub const fn new() -> Self {
        // Power-of-2 check: N & (N-1) == 0  (and N > 0)
        assert!(N >= 2 && (N & (N - 1)) == 0, "QRing capacity must be a power of 2");
        QRing {
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            buffer: [0u8; N],
        }
    }

    /// Mask an index to wrap around the ring.
    #[inline(always)]
    const fn mask(idx: usize) -> usize {
        idx & (N - 1)
    }

    /// Non-blocking send: copy `data` into the ring buffer.
    ///
    /// Returns `Ok(len)` on success, or `Err(Full)` if there isn't
    /// enough contiguous space for the entire message.
    ///
    /// **Thread safety**: Only ONE producer fiber may call `send()`.
    pub fn send(&mut self, data: &[u8]) -> Result<usize, QRingError> {
        if data.len() >= N {
            return Err(QRingError::TooLarge);
        }
        if data.is_empty() {
            return Ok(0);
        }

        let head = self.head.load(Ordering::Relaxed);
        let tail = self.tail.load(Ordering::Acquire);

        let free = if head >= tail {
            N - 1 - (head - tail)
        } else {
            tail - head - 1
        };

        if data.len() > free {
            return Err(QRingError::Full);
        }

        // Copy data into the ring, handling wrap-around
        for (i, &byte) in data.iter().enumerate() {
            let idx = Self::mask(head + i);
            // SAFETY: We verified there's enough space and idx < N.
            self.buffer[idx] = byte;
        }

        // Advance head — makes the data visible to the consumer
        self.head.store(head + data.len(), Ordering::Release);
        Ok(data.len())
    }

    /// Non-blocking receive: copy up to `buf.len()` bytes from the ring.
    ///
    /// Returns `Ok(n)` where `n` is the number of bytes actually read,
    /// or `Err(Empty)` if the ring is empty.
    ///
    /// **Thread safety**: Only ONE consumer fiber may call `recv()`.
    pub fn recv(&mut self, buf: &mut [u8]) -> Result<usize, QRingError> {
        let tail = self.tail.load(Ordering::Relaxed);
        let head = self.head.load(Ordering::Acquire);

        let available = head.wrapping_sub(tail);
        if available == 0 {
            return Err(QRingError::Empty);
        }

        let to_read = buf.len().min(available);

        // Copy data out, handling wrap-around
        for i in 0..to_read {
            let idx = Self::mask(tail + i);
            buf[i] = self.buffer[idx];
        }

        // Advance tail — frees space for the producer
        self.tail.store(tail + to_read, Ordering::Release);
        Ok(to_read)
    }

    /// Number of bytes available to read.
    pub fn available(&self) -> usize {
        let head = self.head.load(Ordering::Acquire);
        let tail = self.tail.load(Ordering::Relaxed);
        head.wrapping_sub(tail)
    }

    /// Number of bytes of free space for writing.
    pub fn free_space(&self) -> usize {
        N - 1 - self.available()
    }

    /// Is the ring empty?
    pub fn is_empty(&self) -> bool {
        self.available() == 0
    }

    /// Is the ring full?
    pub fn is_full(&self) -> bool {
        self.available() >= N - 1
    }

    /// Total capacity (usable bytes = N - 1).
    pub const fn capacity(&self) -> usize {
        N - 1
    }

    /// Reset the ring to empty state.
    ///
    /// **Not thread-safe** — caller must ensure neither producer nor
    /// consumer is active.
    pub fn reset(&mut self) {
        self.head.store(0, Ordering::Relaxed);
        self.tail.store(0, Ordering::Relaxed);
    }
}
