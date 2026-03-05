//! # Theme Engine — Dynamic Theming with Time-of-Day Adaptation
//!
//! The Aether visual layer adapts its color palette, typography,
//! and material properties based on time of day, ambient light,
//! and user preferences (Section 4.2).
//!
//! Features:
//! - Time-based palette shifting (warm sunrise → cool midnight)
//! - Ambient light adaptation (via sensor data)
//! - Material variants (glass opacity, blur radius, tint)
//! - Per-Silo theme overrides
//! - Smooth transitions between theme states

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Time of day period.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimePeriod {
    Dawn,     // 5-8
    Morning,  // 8-12
    Afternoon,// 12-17
    Dusk,     // 17-20
    Evening,  // 20-23
    Night,    // 23-5
}

impl TimePeriod {
    pub fn from_hour(hour: u8) -> Self {
        match hour {
            5..=7 => TimePeriod::Dawn,
            8..=11 => TimePeriod::Morning,
            12..=16 => TimePeriod::Afternoon,
            17..=19 => TimePeriod::Dusk,
            20..=22 => TimePeriod::Evening,
            _ => TimePeriod::Night,
        }
    }
}

/// A color (RGBA).
#[derive(Debug, Clone, Copy, Default)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Color { r, g, b, a } }

    /// Lerp between two colors.
    pub fn lerp(a: Color, b: Color, t: f32) -> Color {
        let t = t.max(0.0).min(1.0);
        Color {
            r: (a.r as f32 * (1.0 - t) + b.r as f32 * t) as u8,
            g: (a.g as f32 * (1.0 - t) + b.g as f32 * t) as u8,
            b: (a.b as f32 * (1.0 - t) + b.b as f32 * t) as u8,
            a: (a.a as f32 * (1.0 - t) + b.a as f32 * t) as u8,
        }
    }
}

/// A theme palette.
#[derive(Debug, Clone)]
pub struct Palette {
    pub background: Color,
    pub surface: Color,
    pub primary: Color,
    pub secondary: Color,
    pub text: Color,
    pub text_dim: Color,
    pub accent: Color,
    pub error: Color,
}

/// Material properties.
#[derive(Debug, Clone)]
pub struct MaterialProps {
    pub blur_radius: f32,
    pub opacity: f32,
    pub tint: Color,
    pub shadow_radius: f32,
    pub corner_radius: f32,
}

/// A complete theme.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub palette: Palette,
    pub material: MaterialProps,
    pub font_family: String,
    pub font_scale: f32,
}

/// Theme engine statistics.
#[derive(Debug, Clone, Default)]
pub struct ThemeStats {
    pub transitions: u64,
    pub overrides_applied: u64,
    pub ambient_updates: u64,
}

/// The Theme Engine.
pub struct ThemeEngine {
    /// Time-period palettes
    pub palettes: BTreeMap<u8, Palette>,
    /// Current resolved theme
    pub current: Theme,
    /// Per-Silo overrides
    pub silo_overrides: BTreeMap<u64, Theme>,
    /// Current time period
    pub period: TimePeriod,
    /// Ambient light (0.0 = dark, 1.0 = bright)
    pub ambient: f32,
    /// Transition progress (0.0-1.0)
    pub transition: f32,
    /// Statistics
    pub stats: ThemeStats,
}

impl ThemeEngine {
    pub fn new() -> Self {
        let default_palette = Palette {
            background: Color::rgba(18, 18, 24, 255),
            surface: Color::rgba(28, 28, 38, 240),
            primary: Color::rgba(100, 140, 255, 255),
            secondary: Color::rgba(180, 100, 255, 255),
            text: Color::rgba(240, 240, 250, 255),
            text_dim: Color::rgba(140, 140, 160, 200),
            accent: Color::rgba(255, 180, 60, 255),
            error: Color::rgba(255, 80, 80, 255),
        };

        let default_theme = Theme {
            name: String::from("Qindows Default"),
            palette: default_palette,
            material: MaterialProps {
                blur_radius: 20.0,
                opacity: 0.85,
                tint: Color::rgba(20, 20, 30, 120),
                shadow_radius: 8.0,
                corner_radius: 12.0,
            },
            font_family: String::from("Qindows Sans"),
            font_scale: 1.0,
        };

        ThemeEngine {
            palettes: BTreeMap::new(),
            current: default_theme,
            silo_overrides: BTreeMap::new(),
            period: TimePeriod::Morning,
            ambient: 0.5,
            transition: 1.0,
            stats: ThemeStats::default(),
        }
    }

    /// Update time of day.
    pub fn set_time(&mut self, hour: u8) {
        let new_period = TimePeriod::from_hour(hour);
        if new_period != self.period {
            self.period = new_period;
            self.transition = 0.0;
            self.stats.transitions += 1;

            // Apply time-based palette if registered
            if let Some(palette) = self.palettes.get(&hour) {
                self.current.palette = palette.clone();
            }
        }
    }

    /// Update ambient light level.
    pub fn set_ambient(&mut self, level: f32) {
        self.ambient = level.max(0.0).min(1.0);
        // Adjust material opacity based on ambient
        self.current.material.opacity = 0.7 + self.ambient * 0.25;
        self.current.material.blur_radius = 15.0 + self.ambient * 10.0;
        self.stats.ambient_updates += 1;
    }

    /// Set a per-Silo theme override.
    pub fn set_override(&mut self, silo_id: u64, theme: Theme) {
        self.silo_overrides.insert(silo_id, theme);
        self.stats.overrides_applied += 1;
    }

    /// Get the theme for a specific Silo.
    pub fn theme_for(&self, silo_id: u64) -> &Theme {
        self.silo_overrides.get(&silo_id).unwrap_or(&self.current)
    }

    /// Advance transition animation.
    pub fn tick(&mut self, dt: f32) {
        if self.transition < 1.0 {
            self.transition = (self.transition + dt * 0.5).min(1.0);
        }
    }
}
