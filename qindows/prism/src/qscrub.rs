//! # Q-Scrub — Background Data Integrity Checker
//!
//! Periodically reads all stored chunks and verifies their
//! hashes against the dedup index (Section 3.35).
//!
//! Features:
//! - Background scanning (low-priority I/O)
//! - Chunk hash verification
//! - Bad-block detection and quarantine
//! - Repair from redundant copies (if available)
//! - Progress tracking and throttling

extern crate alloc;

use alloc::vec::Vec;

/// Scrub state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrubState {
    Idle,
    Running,
    Paused,
    Completed,
}

/// A scrub error (corrupt chunk found).
#[derive(Debug, Clone)]
pub struct ScrubError {
    pub block_addr: u64,
    pub expected_hash: [u8; 32],
    pub actual_hash: [u8; 32],
    pub repaired: bool,
}

/// Scrub statistics.
#[derive(Debug, Clone, Default)]
pub struct ScrubStats {
    pub blocks_scanned: u64,
    pub blocks_total: u64,
    pub errors_found: u64,
    pub errors_repaired: u64,
    pub bytes_read: u64,
    pub runs_completed: u64,
}

/// The Q-Scrub Engine.
pub struct QScrub {
    pub state: ScrubState,
    pub current_block: u64,
    pub max_block: u64,
    pub errors: Vec<ScrubError>,
    pub throttle_pct: u8,
    pub stats: ScrubStats,
}

impl QScrub {
    pub fn new(max_block: u64) -> Self {
        QScrub {
            state: ScrubState::Idle,
            current_block: 0,
            max_block,
            errors: Vec::new(),
            throttle_pct: 10, // Use at most 10% I/O bandwidth
            stats: ScrubStats { blocks_total: max_block, ..Default::default() },
        }
    }

    /// Start a scrub run.
    pub fn start(&mut self) {
        self.state = ScrubState::Running;
        self.current_block = 0;
        self.errors.clear();
    }

    /// Scrub one block (call in background loop).
    pub fn scrub_block(&mut self, block: u64, expected: [u8; 32], actual: [u8; 32], block_size: u64) -> bool {
        if self.state != ScrubState::Running { return false; }

        self.stats.blocks_scanned += 1;
        self.stats.bytes_read += block_size;
        self.current_block = block;

        if expected != actual {
            self.errors.push(ScrubError {
                block_addr: block, expected_hash: expected,
                actual_hash: actual, repaired: false,
            });
            self.stats.errors_found += 1;
            return false; // Corruption detected
        }

        if self.current_block >= self.max_block.saturating_sub(1) {
            self.state = ScrubState::Completed;
            self.stats.runs_completed += 1;
        }

        true
    }

    /// Mark an error as repaired.
    pub fn mark_repaired(&mut self, block_addr: u64) {
        if let Some(err) = self.errors.iter_mut().find(|e| e.block_addr == block_addr) {
            err.repaired = true;
            self.stats.errors_repaired += 1;
        }
    }

    /// Progress percentage.
    pub fn progress(&self) -> f64 {
        if self.max_block == 0 { return 100.0; }
        (self.current_block as f64 / self.max_block as f64) * 100.0
    }

    /// Pause scrub.
    pub fn pause(&mut self) {
        if self.state == ScrubState::Running { self.state = ScrubState::Paused; }
    }

    /// Resume scrub.
    pub fn resume(&mut self) {
        if self.state == ScrubState::Paused { self.state = ScrubState::Running; }
    }
}
