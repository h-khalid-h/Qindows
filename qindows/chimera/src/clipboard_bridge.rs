//! # Chimera Clipboard Bridge
//!
//! Bridges Win32 clipboard APIs (OpenClipboard, SetClipboardData,
//! GetClipboardData, etc.) to Aether's native clipboard system.
//!
//! Legacy apps see a standard Windows clipboard; Qindows sees
//! Prism objects with rich metadata. Each Silo gets an isolated
//! clipboard namespace by default, with opt-in cross-Silo sharing.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ─── Clipboard Formats ─────────────────────────────────────────────────────

/// Win32 clipboard format constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ClipboardFormat {
    /// CF_TEXT (1) — ANSI text
    Text,
    /// CF_UNICODETEXT (13) — UTF-16 text
    UnicodeText,
    /// CF_BITMAP (2) — Device-dependent bitmap
    Bitmap,
    /// CF_DIB (8) — Device-independent bitmap
    Dib,
    /// CF_HDROP (15) — File list (drag-and-drop)
    HDrop,
    /// CF_HTML — HTML fragment
    Html,
    /// CF_RTF — Rich text
    Rtf,
    /// Custom registered format
    Custom(u32),
}

impl ClipboardFormat {
    pub fn from_u32(v: u32) -> Self {
        match v {
            1  => ClipboardFormat::Text,
            13 => ClipboardFormat::UnicodeText,
            2  => ClipboardFormat::Bitmap,
            8  => ClipboardFormat::Dib,
            15 => ClipboardFormat::HDrop,
            0xC004 => ClipboardFormat::Html,
            0xC005 => ClipboardFormat::Rtf,
            other  => ClipboardFormat::Custom(other),
        }
    }

    pub fn to_u32(&self) -> u32 {
        match self {
            ClipboardFormat::Text        => 1,
            ClipboardFormat::UnicodeText  => 13,
            ClipboardFormat::Bitmap       => 2,
            ClipboardFormat::Dib          => 8,
            ClipboardFormat::HDrop        => 15,
            ClipboardFormat::Html         => 0xC004,
            ClipboardFormat::Rtf          => 0xC005,
            ClipboardFormat::Custom(v)    => *v,
        }
    }
}

// ─── Clipboard Data ─────────────────────────────────────────────────────────

/// A clipboard entry — data in one or more formats.
#[derive(Debug, Clone)]
pub struct ClipboardEntry {
    /// Available formats and their raw data
    pub formats: BTreeMap<ClipboardFormat, Vec<u8>>,
    /// Which Silo placed this data
    pub source_silo: u64,
    /// Timestamp (ns)
    pub timestamp: u64,
    /// Prism OID (if backed by a Prism object)
    pub prism_oid: Option<[u8; 32]>,
}

/// Clipboard history item.
#[derive(Debug, Clone)]
pub struct HistoryItem {
    /// Sequence number
    pub seq: u64,
    /// Preview text (first 256 chars)
    pub preview: String,
    /// Which format was primary
    pub primary_format: ClipboardFormat,
    /// Data size in bytes
    pub size: usize,
    /// Source Silo
    pub source_silo: u64,
    /// Timestamp
    pub timestamp: u64,
}

// ─── Clipboard Bridge ───────────────────────────────────────────────────────

/// Clipboard state for the Win32 bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardState {
    /// Clipboard is available
    Available,
    /// Clipboard is opened by a specific handle
    Opened,
    /// Clipboard is being emptied (between EmptyClipboard and CloseClipboard)
    Emptying,
}

/// Clipboard bridge errors.
#[derive(Debug, Clone)]
pub enum ClipboardError {
    /// Clipboard not opened (must call OpenClipboard first)
    NotOpened,
    /// Clipboard already opened by another handle
    AlreadyOpened,
    /// Format not available
    FormatNotAvailable,
    /// Data too large
    DataTooLarge,
    /// Access denied (cross-Silo without permission)
    AccessDenied,
}

/// Statistics.
#[derive(Debug, Clone, Default)]
pub struct ClipboardStats {
    pub opens: u64,
    pub closes: u64,
    pub sets: u64,
    pub gets: u64,
    pub empties: u64,
    pub format_registrations: u64,
    pub cross_silo_copies: u64,
}

/// The Clipboard Bridge.
pub struct ClipboardBridge {
    /// Current clipboard contents
    pub current: Option<ClipboardEntry>,
    /// Clipboard state
    pub state: ClipboardState,
    /// Handle that opened the clipboard (if any)
    pub owner_handle: Option<u32>,
    /// Silo that owns the clipboard
    pub owner_silo: u64,
    /// Clipboard history (last N entries)
    pub history: Vec<HistoryItem>,
    /// History capacity
    pub history_capacity: usize,
    /// Next history sequence number
    next_seq: u64,
    /// Registered custom format names
    pub custom_formats: BTreeMap<String, u32>,
    /// Next custom format ID
    next_format_id: u32,
    /// Max data size per entry
    pub max_data_size: usize,
    /// Allow cross-Silo clipboard access?
    pub allow_cross_silo: bool,
    /// Statistics
    pub stats: ClipboardStats,
}

impl ClipboardBridge {
    pub fn new(silo_id: u64) -> Self {
        ClipboardBridge {
            current: None,
            state: ClipboardState::Available,
            owner_handle: None,
            owner_silo: silo_id,
            history: Vec::new(),
            history_capacity: 50,
            next_seq: 1,
            custom_formats: BTreeMap::new(),
            next_format_id: 0xC100,
            max_data_size: 16 * 1024 * 1024, // 16 MB
            allow_cross_silo: false,
            stats: ClipboardStats::default(),
        }
    }

    /// OpenClipboard(hwnd) emulation.
    pub fn open(&mut self, hwnd: u32) -> Result<(), ClipboardError> {
        if self.state == ClipboardState::Opened {
            return Err(ClipboardError::AlreadyOpened);
        }
        self.state = ClipboardState::Opened;
        self.owner_handle = Some(hwnd);
        self.stats.opens += 1;
        Ok(())
    }

    /// CloseClipboard() emulation.
    pub fn close(&mut self) -> Result<(), ClipboardError> {
        if self.state != ClipboardState::Opened && self.state != ClipboardState::Emptying {
            return Err(ClipboardError::NotOpened);
        }
        self.state = ClipboardState::Available;
        self.owner_handle = None;
        self.stats.closes += 1;
        Ok(())
    }

    /// EmptyClipboard() emulation.
    pub fn empty(&mut self) -> Result<(), ClipboardError> {
        if self.state != ClipboardState::Opened {
            return Err(ClipboardError::NotOpened);
        }
        self.current = None;
        self.state = ClipboardState::Emptying;
        self.stats.empties += 1;
        Ok(())
    }

    /// SetClipboardData(format, data) emulation.
    pub fn set_data(
        &mut self,
        format: ClipboardFormat,
        data: Vec<u8>,
        now: u64,
    ) -> Result<(), ClipboardError> {
        if self.state != ClipboardState::Opened && self.state != ClipboardState::Emptying {
            return Err(ClipboardError::NotOpened);
        }
        if data.len() > self.max_data_size {
            return Err(ClipboardError::DataTooLarge);
        }

        let entry = self.current.get_or_insert_with(|| ClipboardEntry {
            formats: BTreeMap::new(),
            source_silo: self.owner_silo,
            timestamp: now,
            prism_oid: None,
        });

        entry.formats.insert(format, data.clone());
        entry.timestamp = now;

        // Add to history
        self.add_history(format, &data, now);

        self.stats.sets += 1;
        Ok(())
    }

    /// GetClipboardData(format) emulation.
    pub fn get_data(&mut self, format: ClipboardFormat) -> Result<&[u8], ClipboardError> {
        if self.state != ClipboardState::Opened {
            return Err(ClipboardError::NotOpened);
        }

        let entry = self.current.as_ref().ok_or(ClipboardError::FormatNotAvailable)?;
        let data = entry.formats.get(&format).ok_or(ClipboardError::FormatNotAvailable)?;
        self.stats.gets += 1;
        Ok(data)
    }

    /// IsClipboardFormatAvailable(format) emulation.
    pub fn is_format_available(&self, format: ClipboardFormat) -> bool {
        self.current.as_ref()
            .map(|e| e.formats.contains_key(&format))
            .unwrap_or(false)
    }

    /// CountClipboardFormats() emulation.
    pub fn count_formats(&self) -> usize {
        self.current.as_ref()
            .map(|e| e.formats.len())
            .unwrap_or(0)
    }

    /// EnumClipboardFormats() emulation.
    pub fn enum_formats(&self) -> Vec<ClipboardFormat> {
        self.current.as_ref()
            .map(|e| e.formats.keys().copied().collect())
            .unwrap_or_default()
    }

    /// RegisterClipboardFormat(name) emulation.
    pub fn register_format(&mut self, name: &str) -> u32 {
        if let Some(&id) = self.custom_formats.get(name) {
            return id;
        }
        let id = self.next_format_id;
        self.next_format_id += 1;
        self.custom_formats.insert(String::from(name), id);
        self.stats.format_registrations += 1;
        id
    }

    /// Add an entry to clipboard history.
    fn add_history(&mut self, format: ClipboardFormat, data: &[u8], now: u64) {
        let preview = if format == ClipboardFormat::Text || format == ClipboardFormat::UnicodeText {
            let text = String::from_utf8_lossy(data);
            let truncated: String = text.chars().take(256).collect();
            truncated
        } else {
            alloc::format!("[{:?} {} bytes]", format, data.len())
        };

        let item = HistoryItem {
            seq: self.next_seq,
            preview,
            primary_format: format,
            size: data.len(),
            source_silo: self.owner_silo,
            timestamp: now,
        };
        self.next_seq += 1;

        self.history.push(item);
        while self.history.len() > self.history_capacity {
            self.history.remove(0);
        }
    }

    /// Get clipboard history.
    pub fn get_history(&self, count: usize) -> &[HistoryItem] {
        let start = self.history.len().saturating_sub(count);
        &self.history[start..]
    }

    /// Convenience: set text (handles UTF-8 → both Text and UnicodeText).
    pub fn set_text(&mut self, text: &str, now: u64) -> Result<(), ClipboardError> {
        self.empty()?;
        // Set as CF_TEXT (ANSI)
        self.set_data(ClipboardFormat::Text, text.as_bytes().to_vec(), now)?;
        // Also set as CF_UNICODETEXT (UTF-16LE)
        let utf16: Vec<u8> = text.encode_utf16()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        self.set_data(ClipboardFormat::UnicodeText, utf16, now)?;
        Ok(())
    }

    /// Convenience: get text.
    pub fn get_text(&mut self) -> Result<String, ClipboardError> {
        // Try UnicodeText first, then Text
        if self.is_format_available(ClipboardFormat::UnicodeText) {
            let data = self.get_data(ClipboardFormat::UnicodeText)?;
            let utf16: Vec<u16> = data.chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            Ok(String::from_utf16_lossy(&utf16))
        } else if self.is_format_available(ClipboardFormat::Text) {
            let data = self.get_data(ClipboardFormat::Text)?;
            Ok(String::from_utf8_lossy(data).into_owned())
        } else {
            Err(ClipboardError::FormatNotAvailable)
        }
    }
}
