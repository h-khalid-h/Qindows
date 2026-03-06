//! # Chimera Virtual Display Adapter
//!
//! Provides a virtual GPU to legacy Win32 applications
//! running inside Chimera Silos. GDI and DirectX calls
//! are captured and tunneled to Aether's vector renderer
//! (Section 5.5 / 4.3).
//!
//! Features:
//! - Virtual display modes (resolution, refresh rate)
//! - GDI → SDF upscaling bridge
//! - DX9/11 surface capture → Aether compositor
//! - Multi-monitor emulation
//! - VSync event injection

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Virtual display mode.
#[derive(Debug, Clone)]
pub struct DisplayMode {
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub bpp: u8,
}

/// A virtual display adapter.
#[derive(Debug, Clone)]
pub struct VirtualAdapter {
    pub id: u64,
    pub name: String,
    pub modes: Vec<DisplayMode>,
    pub current_mode: usize,
    pub enabled: bool,
    pub silo_id: u64,
}

/// A captured frame from the virtual adapter.
#[derive(Debug, Clone)]
pub struct CapturedFrame {
    pub adapter_id: u64,
    pub frame_number: u64,
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data_size: u64,
    pub timestamp: u64,
}

/// Pixel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Bgra8,
    Rgba8,
    Rgb565,
    Argb2101010,
}

/// Display adapter statistics.
#[derive(Debug, Clone, Default)]
pub struct DisplayStats {
    pub frames_captured: u64,
    pub frames_dropped: u64,
    pub mode_changes: u64,
    pub vsync_events: u64,
    pub bytes_transferred: u64,
}

/// The Virtual Display Manager.
pub struct VirtualDisplay {
    pub adapters: BTreeMap<u64, VirtualAdapter>,
    pub frame_queue: Vec<CapturedFrame>,
    pub max_queue: usize,
    next_id: u64,
    pub stats: DisplayStats,
}

impl VirtualDisplay {
    pub fn new() -> Self {
        VirtualDisplay {
            adapters: BTreeMap::new(),
            frame_queue: Vec::new(),
            max_queue: 4, // Triple-buffering + 1
            next_id: 1,
            stats: DisplayStats::default(),
        }
    }

    /// Create a virtual adapter for a Silo.
    pub fn create_adapter(&mut self, silo_id: u64, name: &str) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let modes = alloc::vec![
            DisplayMode { width: 1920, height: 1080, refresh_hz: 60, bpp: 32 },
            DisplayMode { width: 2560, height: 1440, refresh_hz: 60, bpp: 32 },
            DisplayMode { width: 3840, height: 2160, refresh_hz: 60, bpp: 32 },
            DisplayMode { width: 1280, height: 720, refresh_hz: 60, bpp: 32 },
        ];

        self.adapters.insert(id, VirtualAdapter {
            id, name: String::from(name), modes, current_mode: 0,
            enabled: true, silo_id,
        });
        id
    }

    /// Set display mode.
    pub fn set_mode(&mut self, adapter_id: u64, mode_index: usize) -> bool {
        if let Some(adapter) = self.adapters.get_mut(&adapter_id) {
            if mode_index < adapter.modes.len() {
                adapter.current_mode = mode_index;
                self.stats.mode_changes += 1;
                return true;
            }
        }
        false
    }

    /// Submit a captured frame.
    pub fn submit_frame(&mut self, frame: CapturedFrame) {
        self.stats.bytes_transferred += frame.data_size;
        if self.frame_queue.len() >= self.max_queue {
            self.frame_queue.remove(0);
            self.stats.frames_dropped += 1;
        }
        self.frame_queue.push(frame);
        self.stats.frames_captured += 1;
    }

    /// Pop the next frame for Aether composition.
    pub fn next_frame(&mut self) -> Option<CapturedFrame> {
        if self.frame_queue.is_empty() { None } else { Some(self.frame_queue.remove(0)) }
    }

    /// Signal VSync to the Silo.
    pub fn vsync(&mut self, adapter_id: u64, _now: u64) {
        if self.adapters.contains_key(&adapter_id) {
            self.stats.vsync_events += 1;
        }
    }

    /// Get adapter info.
    pub fn get_adapter(&self, adapter_id: u64) -> Option<&VirtualAdapter> {
        self.adapters.get(&adapter_id)
    }
}
