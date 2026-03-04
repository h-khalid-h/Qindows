//! # Clipboard Manager
//!
//! System-wide clipboard with multi-item history and cross-Silo
//! copy/paste. Each clipboard entry is a Prism object for
//! persistence and deduplication.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Clipboard content types.
#[derive(Debug, Clone)]
pub enum ClipboardContent {
    /// Plain text
    Text(String),
    /// Rich text (formatted)
    RichText { text: String, format: String },
    /// Image data (OID reference in Prism)
    Image { width: u32, height: u32, oid: u64 },
    /// File reference(s) (Prism OIDs)
    Files(Vec<u64>),
    /// Raw binary data
    Binary { mime_type: String, data: Vec<u8> },
}

/// A clipboard entry.
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    /// Entry ID
    pub id: u64,
    /// Content
    pub content: ClipboardContent,
    /// Source Silo ID
    pub source_silo: u64,
    /// Source app name
    pub source_app: String,
    /// Timestamp (ticks)
    pub timestamp: u64,
    /// Is this entry pinned (won't be evicted)?
    pub pinned: bool,
}

/// The Clipboard Manager.
pub struct ClipboardManager {
    /// Clipboard history (most recent first)
    pub entries: Vec<ClipboardEntry>,
    /// Next entry ID
    next_id: u64,
    /// Max history size
    pub max_history: usize,
    /// Is clipboard sync enabled across devices (via Nexus)?
    pub sync_enabled: bool,
}

impl ClipboardManager {
    pub fn new() -> Self {
        ClipboardManager {
            entries: Vec::new(),
            next_id: 1,
            max_history: 50,
            sync_enabled: false,
        }
    }

    /// Copy content to the clipboard.
    pub fn copy(&mut self, content: ClipboardContent, silo_id: u64, app_name: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.entries.insert(0, ClipboardEntry {
            id,
            content,
            source_silo: silo_id,
            source_app: app_name,
            timestamp: 0, // Would be set to current tick
            pinned: false,
        });

        // Trim history (keep pinned entries)
        while self.entries.len() > self.max_history {
            // Remove oldest non-pinned entry
            if let Some(pos) = self.entries.iter().rposition(|e| !e.pinned) {
                self.entries.remove(pos);
            } else {
                break; // All pinned — can't evict
            }
        }

        id
    }

    /// Paste (get the most recent entry).
    pub fn paste(&self) -> Option<&ClipboardEntry> {
        self.entries.first()
    }

    /// Paste a specific history entry by ID.
    pub fn paste_by_id(&self, id: u64) -> Option<&ClipboardEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Pin an entry (prevent eviction).
    pub fn pin(&mut self, id: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = true;
        }
    }

    /// Unpin an entry.
    pub fn unpin(&mut self, id: u64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            e.pinned = false;
        }
    }

    /// Delete a specific entry.
    pub fn delete(&mut self, id: u64) {
        self.entries.retain(|e| e.id != id);
    }

    /// Clear all non-pinned entries.
    pub fn clear(&mut self) {
        self.entries.retain(|e| e.pinned);
    }

    /// Get a text preview of the most recent entry.
    pub fn preview(&self) -> Option<String> {
        self.entries.first().map(|e| match &e.content {
            ClipboardContent::Text(s) => {
                if s.len() > 80 {
                    alloc::format!("{}...", &s[..80])
                } else {
                    s.clone()
                }
            }
            ClipboardContent::RichText { text, .. } => {
                if text.len() > 80 {
                    alloc::format!("{}...", &text[..80])
                } else {
                    text.clone()
                }
            }
            ClipboardContent::Image { width, height, .. } => {
                alloc::format!("Image ({}×{})", width, height)
            }
            ClipboardContent::Files(oids) => {
                alloc::format!("{} file(s)", oids.len())
            }
            ClipboardContent::Binary { mime_type, data } => {
                alloc::format!("{} ({} bytes)", mime_type, data.len())
            }
        })
    }
}
