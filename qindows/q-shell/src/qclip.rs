//! # Q-Clipboard — Cross-Silo Clipboard with Capability Gates
//!
//! Clipboard data cannot leak between Silos without explicit
//! capability grants (Section 2.3). Each paste requires a
//! visible capability token.
//!
//! Features:
//! - Per-Silo clipboard isolation by default
//! - Cross-Silo paste requires CapabilityToken::ClipboardShare
//! - Content types: text, rich text, image, file references
//! - Clipboard history (last N entries)
//! - Sensitive data auto-expiry (passwords clear after 30s)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Clipboard content type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipType {
    PlainText,
    RichText,
    Image,
    FileRef,
    Binary,
}

/// Sensitivity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    Normal,
    Sensitive,  // Auto-clears after timeout (e.g., passwords)
    Restricted, // Cannot leave the originating Silo
}

/// A clipboard entry.
#[derive(Debug, Clone)]
pub struct ClipEntry {
    pub id: u64,
    pub silo_id: u64,
    pub clip_type: ClipType,
    pub data: Vec<u8>,
    pub label: String,
    pub sensitivity: Sensitivity,
    pub created_at: u64,
    pub expires_at: u64, // 0 = no expiry
    pub access_count: u64,
}

/// Clipboard statistics.
#[derive(Debug, Clone, Default)]
pub struct ClipStats {
    pub copies: u64,
    pub pastes: u64,
    pub cross_silo_pastes: u64,
    pub blocked_pastes: u64,
    pub expired_entries: u64,
}

/// The Q-Clipboard Manager.
pub struct QClipboard {
    /// Per-Silo clipboards
    pub boards: BTreeMap<u64, Vec<ClipEntry>>,
    /// Max history per Silo
    pub max_history: usize,
    /// Sensitive data expiry (seconds)
    pub sensitive_expiry: u64,
    /// Next entry ID
    next_id: u64,
    /// Statistics
    pub stats: ClipStats,
}

impl QClipboard {
    pub fn new() -> Self {
        QClipboard {
            boards: BTreeMap::new(),
            max_history: 50,
            sensitive_expiry: 30,
            next_id: 1,
            stats: ClipStats::default(),
        }
    }

    /// Copy data to a Silo's clipboard.
    pub fn copy(&mut self, silo_id: u64, clip_type: ClipType, data: Vec<u8>, label: &str, sensitivity: Sensitivity, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let expires_at = if sensitivity == Sensitivity::Sensitive {
            now + self.sensitive_expiry
        } else {
            0
        };

        let board = self.boards.entry(silo_id).or_insert_with(Vec::new);

        board.push(ClipEntry {
            id, silo_id, clip_type, data, label: String::from(label),
            sensitivity, created_at: now, expires_at, access_count: 0,
        });

        // Trim history
        while board.len() > self.max_history {
            board.remove(0);
        }

        self.stats.copies += 1;
        id
    }

    /// Paste from the same Silo's clipboard (most recent).
    pub fn paste(&mut self, silo_id: u64, now: u64) -> Option<&ClipEntry> {
        self.expire(silo_id, now);

        let board = self.boards.get_mut(&silo_id)?;
        let entry = board.last_mut()?;
        entry.access_count += 1;
        self.stats.pastes += 1;
        // Return immutable ref
        self.boards.get(&silo_id)?.last()
    }

    /// Cross-Silo paste (requires capability check).
    pub fn cross_paste(&mut self, from_silo: u64, to_silo: u64, has_cap: bool, now: u64) -> Result<ClipEntry, &'static str> {
        if !has_cap {
            self.stats.blocked_pastes += 1;
            return Err("No ClipboardShare capability");
        }

        self.expire(from_silo, now);

        let board = self.boards.get(&from_silo).ok_or("Source Silo has no clipboard")?;
        let entry = board.last().ok_or("Clipboard empty")?;

        if entry.sensitivity == Sensitivity::Restricted {
            self.stats.blocked_pastes += 1;
            return Err("Restricted content cannot leave originating Silo");
        }

        self.stats.cross_silo_pastes += 1;
        Ok(entry.clone())
    }

    /// Expire sensitive entries.
    fn expire(&mut self, silo_id: u64, now: u64) {
        if let Some(board) = self.boards.get_mut(&silo_id) {
            let before = board.len();
            board.retain(|e| e.expires_at == 0 || now < e.expires_at);
            let removed = before - board.len();
            self.stats.expired_entries += removed as u64;
        }
    }

    /// Clear a Silo's clipboard.
    pub fn clear(&mut self, silo_id: u64) {
        self.boards.remove(&silo_id);
    }
}
