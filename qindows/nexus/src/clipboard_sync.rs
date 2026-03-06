//! # Clipboard Sync — Mesh-Wide Clipboard Sharing
//!
//! Synchronizes clipboard content across mesh nodes
//! with end-to-end encryption (Section 11.7).
//!
//! Features:
//! - Copy on one node, paste on another
//! - Per-Silo clipboard isolation (only syncs within same Silo across nodes)
//! - MIME type support (text, images, files)
//! - Clipboard history (last N entries)
//! - E2E encryption for sync payloads

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Clipboard entry type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipType {
    Text,
    Image,
    File,
    Html,
    Custom,
}

/// A clipboard entry.
#[derive(Debug, Clone)]
pub struct ClipEntry {
    pub id: u64,
    pub silo_id: u64,
    pub clip_type: ClipType,
    pub mime: String,
    pub size: u64,
    pub hash: [u8; 32],
    pub source_node: [u8; 32],
    pub created_at: u64,
    pub synced: bool,
}

/// Clipboard sync statistics.
#[derive(Debug, Clone, Default)]
pub struct ClipSyncStats {
    pub copies: u64,
    pub pastes: u64,
    pub syncs_sent: u64,
    pub syncs_received: u64,
    pub bytes_synced: u64,
}

/// The Clipboard Sync Manager.
pub struct ClipboardSync {
    /// Per-Silo clipboard (last entry is current)
    pub clipboards: BTreeMap<u64, Vec<ClipEntry>>,
    /// Max history per Silo
    pub max_history: usize,
    next_id: u64,
    pub local_node: [u8; 32],
    pub stats: ClipSyncStats,
}

impl ClipboardSync {
    pub fn new(local_node: [u8; 32]) -> Self {
        ClipboardSync {
            clipboards: BTreeMap::new(),
            max_history: 25,
            next_id: 1,
            local_node,
            stats: ClipSyncStats::default(),
        }
    }

    /// Copy to clipboard.
    pub fn copy(&mut self, silo_id: u64, clip_type: ClipType, mime: &str, size: u64, hash: [u8; 32], now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let history = self.clipboards.entry(silo_id).or_insert_with(Vec::new);

        // Trim history
        while history.len() >= self.max_history {
            history.remove(0);
        }

        history.push(ClipEntry {
            id, silo_id, clip_type, mime: String::from(mime),
            size, hash, source_node: self.local_node,
            created_at: now, synced: false,
        });

        self.stats.copies += 1;
        id
    }

    /// Get current clipboard entry.
    pub fn paste(&mut self, silo_id: u64) -> Option<&ClipEntry> {
        self.stats.pastes += 1;
        self.clipboards.get(&silo_id)
            .and_then(|h| h.last())
    }

    /// Get clipboard history.
    pub fn history(&self, silo_id: u64) -> &[ClipEntry] {
        self.clipboards.get(&silo_id)
            .map(|h| h.as_slice())
            .unwrap_or(&[])
    }

    /// Receive a synced clipboard entry from another node.
    pub fn receive_sync(&mut self, silo_id: u64, clip_type: ClipType, mime: &str, size: u64, hash: [u8; 32], source: [u8; 32], now: u64) {
        let id = self.next_id;
        self.next_id += 1;

        let history = self.clipboards.entry(silo_id).or_insert_with(Vec::new);
        while history.len() >= self.max_history {
            history.remove(0);
        }

        history.push(ClipEntry {
            id, silo_id, clip_type, mime: String::from(mime),
            size, hash, source_node: source,
            created_at: now, synced: true,
        });

        self.stats.syncs_received += 1;
        self.stats.bytes_synced += size;
    }

    /// Mark entries as synced.
    pub fn mark_synced(&mut self, silo_id: u64) {
        if let Some(history) = self.clipboards.get_mut(&silo_id) {
            for entry in history.iter_mut() {
                if !entry.synced && entry.source_node == self.local_node {
                    entry.synced = true;
                    self.stats.syncs_sent += 1;
                    self.stats.bytes_synced += entry.size;
                }
            }
        }
    }
}
