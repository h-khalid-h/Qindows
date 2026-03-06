//! # Aether Display Profiles
//!
//! Manages multi-monitor display configurations, color profiles,
//! scaling, and refresh rate settings for the Aether compositor
//! (Section 4.7).
//!
//! Features:
//! - Multi-monitor layout (position, orientation, primary)
//! - Per-display scaling (HiDPI aware)
//! - Color profiles (sRGB, DCI-P3, HDR10)
//! - Adaptive refresh rate (VRR)
//! - Display hotplug handling
//! - Night mode color temperature

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Display orientation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Landscape,
    Portrait,
    LandscapeFlipped,
    PortraitFlipped,
}

/// Color profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorProfile {
    Srgb,
    DciP3,
    Hdr10,
    AdobeRgb,
    Custom(u32),
}

/// A display.
#[derive(Debug, Clone)]
pub struct Display {
    pub id: u32,
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub refresh_hz: u32,
    pub vrr_capable: bool,
    pub vrr_enabled: bool,
    pub scale_factor: u32, // 100 = 1x, 200 = 2x
    pub orientation: Orientation,
    pub color_profile: ColorProfile,
    pub primary: bool,
    /// Position in virtual desktop (x, y)
    pub pos_x: i32,
    pub pos_y: i32,
    pub connected: bool,
    pub enabled: bool,
}

/// Night mode settings.
#[derive(Debug, Clone)]
pub struct NightMode {
    pub enabled: bool,
    pub color_temp_k: u32, // 2700-6500
    pub strength: u8, // 0-100
    pub schedule_start_h: u8,
    pub schedule_end_h: u8,
}

/// Display manager statistics.
#[derive(Debug, Clone, Default)]
pub struct DisplayStats {
    pub hotplug_connects: u64,
    pub hotplug_disconnects: u64,
    pub mode_changes: u64,
    pub profile_changes: u64,
}

/// The Display Profile Manager.
pub struct DisplayManager {
    pub displays: BTreeMap<u32, Display>,
    pub night_mode: NightMode,
    next_id: u32,
    pub stats: DisplayStats,
}

impl DisplayManager {
    pub fn new() -> Self {
        DisplayManager {
            displays: BTreeMap::new(),
            night_mode: NightMode {
                enabled: false, color_temp_k: 6500,
                strength: 50, schedule_start_h: 22,
                schedule_end_h: 6,
            },
            next_id: 1,
            stats: DisplayStats::default(),
        }
    }

    /// Connect a display.
    pub fn connect(
        &mut self, name: &str, width: u32, height: u32,
        refresh: u32, vrr: bool,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let primary = self.displays.is_empty();

        self.displays.insert(id, Display {
            id, name: String::from(name), width, height,
            refresh_hz: refresh, vrr_capable: vrr, vrr_enabled: vrr,
            scale_factor: if width >= 3840 { 200 } else { 100 },
            orientation: Orientation::Landscape,
            color_profile: ColorProfile::Srgb,
            primary, pos_x: 0, pos_y: 0,
            connected: true, enabled: true,
        });

        // Auto-position: place new display to the right
        self.auto_layout();
        self.stats.hotplug_connects += 1;
        id
    }

    /// Disconnect a display.
    pub fn disconnect(&mut self, id: u32) {
        if let Some(d) = self.displays.get_mut(&id) {
            d.connected = false;
            d.enabled = false;
            self.stats.hotplug_disconnects += 1;
        }
    }

    /// Set scaling factor.
    pub fn set_scale(&mut self, id: u32, factor: u32) {
        if let Some(d) = self.displays.get_mut(&id) {
            d.scale_factor = factor.max(100).min(400);
        }
    }

    /// Set color profile.
    pub fn set_color_profile(&mut self, id: u32, profile: ColorProfile) {
        if let Some(d) = self.displays.get_mut(&id) {
            d.color_profile = profile;
            self.stats.profile_changes += 1;
        }
    }

    /// Set night mode.
    pub fn set_night_mode(&mut self, enabled: bool, temp_k: u32) {
        self.night_mode.enabled = enabled;
        self.night_mode.color_temp_k = temp_k.max(2700).min(6500);
    }

    /// Auto-layout displays left-to-right.
    fn auto_layout(&mut self) {
        let mut x = 0i32;
        let ids: Vec<u32> = self.displays.keys().copied().collect();
        for id in ids {
            if let Some(d) = self.displays.get_mut(&id) {
                if d.connected && d.enabled {
                    d.pos_x = x;
                    d.pos_y = 0;
                    x += d.width as i32;
                }
            }
        }
    }

    /// Get virtual desktop bounds.
    pub fn virtual_bounds(&self) -> (i32, i32, u32, u32) {
        let mut min_x = 0i32;
        let mut max_x = 0i32;
        let mut max_y = 0u32;
        for d in self.displays.values().filter(|d| d.enabled) {
            if d.pos_x < min_x { min_x = d.pos_x; }
            if d.pos_x + d.width as i32 > max_x { max_x = d.pos_x + d.width as i32; }
            if d.height > max_y { max_y = d.height; }
        }
        (min_x, 0, (max_x - min_x) as u32, max_y)
    }
}
