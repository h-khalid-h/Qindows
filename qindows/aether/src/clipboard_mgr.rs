//! # Aether Clipboard Manager
//!
//! System-wide clipboard with multi-format support,
//! clipboard history, and cross-Silo paste permissions.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Clipboard data formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClipboardFormat {
    /// Plain text (UTF-8)
    PlainText,
    /// Rich text (HTML)
    RichText,
    /// Image (raw RGBA pixels)
    ImageRgba,
    /// Image (PNG encoded)
    ImagePng,
    /// File list (paths)
    Files,
    /// URL
    Url,
    /// Custom MIME type
    Custom(u32),
}

/// A single clipboard entry.
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    /// Unique entry ID
    pub id: u64,
    /// Data per format (one entry can have multiple representations)
    pub formats: BTreeMap<ClipboardFormat, Vec<u8>>,
    /// Source Silo that wrote this entry
    pub source_silo: u64,
    /// Timestamp when copied
    pub copied_at: u64,
    /// Has this been pasted?
    pub pasted: bool,
    /// Number of times pasted
    pub paste_count: u32,
    /// Is this entry pinned (won't be evicted from history)?
    pub pinned: bool,
    /// Preview text (first 100 chars for UI display)
    pub preview: String,
}

impl ClipboardEntry {
    /// Get data in a specific format.
    pub fn get(&self, format: ClipboardFormat) -> Option<&[u8]> {
        self.formats.get(&format).map(|v| v.as_slice())
    }

    /// Get as plain text.
    pub fn as_text(&self) -> Option<&str> {
        self.formats.get(&ClipboardFormat::PlainText)
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
    }

    /// Available formats.
    pub fn available_formats(&self) -> Vec<ClipboardFormat> {
        self.formats.keys().copied().collect()
    }
}

/// Clipboard paste permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PastePermission {
    /// Any Silo can paste
    Public,
    /// Only the source Silo can paste
    Private,
    /// Only specified Silos can paste
    Restricted,
}

/// The System Clipboard Manager.
pub struct ClipboardManager {
    /// Current clipboard contents (latest copy)
    pub current: Option<ClipboardEntry>,
    /// Clipboard history
    pub history: Vec<ClipboardEntry>,
    /// Maximum history size
    pub max_history: usize,
    /// Next entry ID
    next_id: u64,
    /// Paste permission mode
    pub permission: PastePermission,
    /// Allowed Silos for restricted mode
    pub allowed_silos: Vec<u64>,
    /// Stats
    pub stats: ClipboardStats,
}

/// Clipboard statistics.
#[derive(Debug, Clone, Default)]
pub struct ClipboardStats {
    pub total_copies: u64,
    pub total_pastes: u64,
    pub total_formats_converted: u64,
    pub denied_pastes: u64,
}

impl ClipboardManager {
    pub fn new() -> Self {
        ClipboardManager {
            current: None,
            history: Vec::new(),
            max_history: 50,
            next_id: 1,
            permission: PastePermission::Public,
            allowed_silos: Vec::new(),
            stats: ClipboardStats::default(),
        }
    }

    /// Copy data to the clipboard.
    pub fn copy(&mut self, source_silo: u64, data: Vec<u8>, format: ClipboardFormat, now: u64) {
        // Generate preview
        let preview = if format == ClipboardFormat::PlainText {
            let text = core::str::from_utf8(&data).unwrap_or("");
            let truncated: String = text.chars().take(100).collect();
            truncated
        } else {
            alloc::format!("[{:?} — {} bytes]", format, data.len())
        };

        let mut formats = BTreeMap::new();
        formats.insert(format, data);

        let entry = ClipboardEntry {
            id: self.next_id,
            formats,
            source_silo,
            copied_at: now,
            pasted: false,
            paste_count: 0,
            pinned: false,
            preview,
        };
        self.next_id += 1;

        // Move old current to history
        if let Some(old) = self.current.take() {
            self.history.insert(0, old);
            // Trim history (keep pinned items)
            while self.history.len() > self.max_history {
                // Remove the oldest non-pinned item
                if let Some(pos) = self.history.iter().rposition(|e| !e.pinned) {
                    self.history.remove(pos);
                } else {
                    break; // All pinned
                }
            }
        }

        self.current = Some(entry);
        self.stats.total_copies += 1;
    }

    /// Copy with multiple format representations.
    pub fn copy_multi(&mut self, source_silo: u64, formats: BTreeMap<ClipboardFormat, Vec<u8>>, now: u64) {
        let preview = if let Some(text_bytes) = formats.get(&ClipboardFormat::PlainText) {
            let text = core::str::from_utf8(text_bytes).unwrap_or("");
            text.chars().take(100).collect()
        } else {
            alloc::format!("[{} formats]", formats.len())
        };

        let entry = ClipboardEntry {
            id: self.next_id,
            formats,
            source_silo,
            copied_at: now,
            pasted: false,
            paste_count: 0,
            pinned: false,
            preview,
        };
        self.next_id += 1;

        if let Some(old) = self.current.take() {
            self.history.insert(0, old);
        }
        self.current = Some(entry);
        self.stats.total_copies += 1;
    }

    /// Paste from the clipboard.
    pub fn paste(&mut self, requesting_silo: u64, format: ClipboardFormat) -> Option<Vec<u8>> {
        // Check permission
        if !self.can_paste(requesting_silo) {
            self.stats.denied_pastes += 1;
            return None;
        }

        if let Some(ref mut entry) = self.current {
            entry.pasted = true;
            entry.paste_count += 1;
            self.stats.total_pastes += 1;
            entry.get(format).map(|data| data.to_vec())
        } else {
            None
        }
    }

    /// Paste from history by index.
    pub fn paste_from_history(&mut self, requesting_silo: u64, index: usize, format: ClipboardFormat) -> Option<Vec<u8>> {
        if !self.can_paste(requesting_silo) {
            self.stats.denied_pastes += 1;
            return None;
        }

        if let Some(entry) = self.history.get_mut(index) {
            entry.pasted = true;
            entry.paste_count += 1;
            self.stats.total_pastes += 1;
            entry.get(format).map(|data| data.to_vec())
        } else {
            None
        }
    }

    /// Check if a Silo is allowed to paste.
    fn can_paste(&self, silo_id: u64) -> bool {
        match self.permission {
            PastePermission::Public => true,
            PastePermission::Private => {
                self.current.as_ref().map(|e| e.source_silo == silo_id).unwrap_or(false)
            }
            PastePermission::Restricted => {
                self.allowed_silos.contains(&silo_id)
            }
        }
    }

    /// Pin a history entry (prevent eviction).
    pub fn pin(&mut self, entry_id: u64) {
        if let Some(entry) = self.history.iter_mut().find(|e| e.id == entry_id) {
            entry.pinned = true;
        }
    }

    /// Clear clipboard and history.
    pub fn clear(&mut self) {
        self.current = None;
        self.history.clear();
    }

    /// Search history by text content.
    pub fn search(&self, query: &str) -> Vec<&ClipboardEntry> {
        let q = query.to_lowercase();
        self.history.iter()
            .filter(|e| e.preview.to_lowercase().contains(&q))
            .collect()
    }
}
