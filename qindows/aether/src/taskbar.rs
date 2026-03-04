//! # Aether Taskbar
//!
//! The bottom bar of the Qindows desktop — shows running apps,
//! system tray, Q-Shell launcher, and virtual desktop switcher.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Taskbar position on screen.
#[derive(Debug, Clone, Copy)]
pub enum TaskbarPosition {
    Bottom,
    Top,
    Left,
    Right,
}

/// A single item in the taskbar.
#[derive(Debug, Clone)]
pub struct TaskbarItem {
    /// Window ID this item represents
    pub window_id: u64,
    /// Display label (truncated app title)
    pub label: String,
    /// Is this app currently focused?
    pub active: bool,
    /// Is this app requesting attention (flashing)?
    pub attention: bool,
    /// App icon OID (Prism object)
    pub icon_oid: Option<u64>,
    /// Badge count (notifications, unread messages)
    pub badge: u32,
    /// Silo health status
    pub healthy: bool,
}

/// System tray indicators.
#[derive(Debug, Clone)]
pub struct SystemTray {
    /// Network status
    pub network: NetworkIndicator,
    /// Battery/power status
    pub power: PowerIndicator,
    /// Volume level (0-100)
    pub volume: u8,
    /// Bluetooth connected devices count
    pub bluetooth_devices: u8,
    /// Is Do Not Disturb active?
    pub dnd: bool,
    /// Current Q-Space label
    pub space_label: String,
}

/// Network indicator states
#[derive(Debug, Clone, Copy)]
pub enum NetworkIndicator {
    Disconnected,
    Wifi { strength: u8 },       // signal 0-4
    Ethernet,
    Mesh { peers: u32 },
}

/// Power indicator states
#[derive(Debug, Clone, Copy)]
pub enum PowerIndicator {
    Battery { level: u8, charging: bool },
    AcPower,
}

/// The Taskbar state.
pub struct Taskbar {
    /// Position on screen
    pub position: TaskbarPosition,
    /// Height in logical pixels
    pub height: f32,
    /// Running app items
    pub items: Vec<TaskbarItem>,
    /// System tray
    pub tray: SystemTray,
    /// Pinned app icons (always visible)
    pub pinned: Vec<String>,
    /// Is the taskbar auto-hidden?
    pub auto_hide: bool,
    /// Is the taskbar currently visible?
    pub visible: bool,
    /// Clock display text
    pub clock_text: String,
}

impl Taskbar {
    pub fn new(height: f32) -> Self {
        Taskbar {
            position: TaskbarPosition::Bottom,
            height,
            items: Vec::new(),
            tray: SystemTray {
                network: NetworkIndicator::Disconnected,
                power: PowerIndicator::AcPower,
                volume: 75,
                bluetooth_devices: 0,
                dnd: false,
                space_label: String::from("Space 1"),
            },
            pinned: alloc::vec![
                String::from("Q-Shell"),
                String::from("Files"),
                String::from("Browser"),
                String::from("Settings"),
            ],
            auto_hide: false,
            visible: true,
            clock_text: String::from("03:35"),
        }
    }

    /// Add a running app to the taskbar.
    pub fn add_item(&mut self, window_id: u64, label: String) {
        // Don't add duplicates
        if self.items.iter().any(|i| i.window_id == window_id) {
            return;
        }

        self.items.push(TaskbarItem {
            window_id,
            label,
            active: false,
            attention: false,
            icon_oid: None,
            badge: 0,
            healthy: true,
        });
    }

    /// Remove an app from the taskbar (window closed).
    pub fn remove_item(&mut self, window_id: u64) {
        self.items.retain(|i| i.window_id != window_id);
    }

    /// Set the active (focused) item.
    pub fn set_active(&mut self, window_id: u64) {
        for item in &mut self.items {
            item.active = item.window_id == window_id;
        }
    }

    /// Flash an item to request attention.
    pub fn request_attention(&mut self, window_id: u64) {
        if let Some(item) = self.items.iter_mut().find(|i| i.window_id == window_id) {
            item.attention = true;
        }
    }

    /// Set badge count (e.g., unread messages).
    pub fn set_badge(&mut self, window_id: u64, count: u32) {
        if let Some(item) = self.items.iter_mut().find(|i| i.window_id == window_id) {
            item.badge = count;
        }
    }

    /// Mark a Silo as unhealthy (Sentinel flagged it).
    pub fn mark_unhealthy(&mut self, window_id: u64) {
        if let Some(item) = self.items.iter_mut().find(|i| i.window_id == window_id) {
            item.healthy = false;
        }
    }

    /// Update the Q-Space label.
    pub fn set_space(&mut self, label: String) {
        self.tray.space_label = label;
    }

    /// Toggle auto-hide.
    pub fn toggle_auto_hide(&mut self) {
        self.auto_hide = !self.auto_hide;
    }
}
