//! # Aether Live Theme System
//!
//! Runtime-switchable theme engine with accent colors,
//! dark/light modes, transparency levels, and animation
//! speed controls. Supports custom user themes.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Color representation (RGBA).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Color { r, g, b, a } }
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self { Color { r, g, b, a: 255 } }

    pub const fn with_alpha(self, a: u8) -> Self { Color { r: self.r, g: self.g, b: self.b, a } }

    pub const WHITE: Color = Color::rgb(255, 255, 255);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);

    /// Blend two colors (self over other).
    pub fn blend(self, other: Color) -> Color {
        let sa = self.a as u16;
        let da = other.a as u16;
        let inv_sa = 255 - sa;
        Color {
            r: ((self.r as u16 * sa + other.r as u16 * inv_sa) / 255) as u8,
            g: ((self.g as u16 * sa + other.g as u16 * inv_sa) / 255) as u8,
            b: ((self.b as u16 * sa + other.b as u16 * inv_sa) / 255) as u8,
            a: ((sa + da * inv_sa / 255).min(255)) as u8,
        }
    }

    /// Lighten by a factor (0.0 = no change, 1.0 = white).
    pub fn lighten(self, factor: f32) -> Color {
        let f = (factor * 255.0) as u16;
        Color {
            r: (self.r as u16 + (255 - self.r as u16) * f / 255).min(255) as u8,
            g: (self.g as u16 + (255 - self.g as u16) * f / 255).min(255) as u8,
            b: (self.b as u16 + (255 - self.b as u16) * f / 255).min(255) as u8,
            a: self.a,
        }
    }

    /// Darken by a factor (0.0 = no change, 1.0 = black).
    pub fn darken(self, factor: f32) -> Color {
        let inv = ((1.0 - factor) * 255.0) as u16;
        Color {
            r: (self.r as u16 * inv / 255) as u8,
            g: (self.g as u16 * inv / 255) as u8,
            b: (self.b as u16 * inv / 255) as u8,
            a: self.a,
        }
    }

    /// Pack to u32 (0xAARRGGBB).
    pub fn to_u32(self) -> u32 {
        (self.a as u32) << 24 | (self.r as u32) << 16 | (self.g as u32) << 8 | self.b as u32
    }
}

/// Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    HighContrast,
    Auto, // Follow system schedule
}

/// Transparency level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transparency {
    None,
    Subtle,      // ~10% transparency
    Medium,      // ~30%
    Full,        // ~60%
    Glassmorphism, // Blur + transparency
}

/// Animation speed preset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimSpeed {
    Off,
    Reduced,
    Normal,
    Fast,
}

/// A complete theme definition.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub mode: ThemeMode,

    // Primary palette
    pub accent: Color,
    pub accent_hover: Color,
    pub accent_pressed: Color,

    // Backgrounds
    pub bg_primary: Color,
    pub bg_secondary: Color,
    pub bg_elevated: Color,
    pub bg_overlay: Color,

    // Text
    pub text_primary: Color,
    pub text_secondary: Color,
    pub text_disabled: Color,
    pub text_on_accent: Color,

    // Borders
    pub border_default: Color,
    pub border_focus: Color,
    pub border_error: Color,

    // Status
    pub success: Color,
    pub warning: Color,
    pub error: Color,
    pub info: Color,

    // Controls
    pub transparency: Transparency,
    pub anim_speed: AnimSpeed,
    pub corner_radius: u8,  // px
    pub shadow_intensity: u8, // 0-100

    // Custom tokens
    pub tokens: BTreeMap<String, String>,
}

impl Theme {
    /// The default Qindows dark theme.
    pub fn qindows_dark() -> Self {
        Theme {
            name: String::from("Qindows Dark"),
            mode: ThemeMode::Dark,
            accent: Color::rgb(0, 120, 215),
            accent_hover: Color::rgb(30, 144, 235),
            accent_pressed: Color::rgb(0, 90, 180),
            bg_primary: Color::rgb(30, 30, 30),
            bg_secondary: Color::rgb(45, 45, 48),
            bg_elevated: Color::rgb(55, 55, 60),
            bg_overlay: Color::rgba(0, 0, 0, 180),
            text_primary: Color::rgb(240, 240, 240),
            text_secondary: Color::rgb(160, 160, 160),
            text_disabled: Color::rgb(90, 90, 90),
            text_on_accent: Color::WHITE,
            border_default: Color::rgb(60, 60, 65),
            border_focus: Color::rgb(0, 120, 215),
            border_error: Color::rgb(232, 17, 35),
            success: Color::rgb(16, 185, 129),
            warning: Color::rgb(245, 158, 11),
            error: Color::rgb(232, 17, 35),
            info: Color::rgb(59, 130, 246),
            transparency: Transparency::Subtle,
            anim_speed: AnimSpeed::Normal,
            corner_radius: 8,
            shadow_intensity: 40,
            tokens: BTreeMap::new(),
        }
    }

    /// The default Qindows light theme.
    pub fn qindows_light() -> Self {
        Theme {
            name: String::from("Qindows Light"),
            mode: ThemeMode::Light,
            accent: Color::rgb(0, 103, 192),
            accent_hover: Color::rgb(0, 120, 215),
            accent_pressed: Color::rgb(0, 84, 153),
            bg_primary: Color::rgb(249, 249, 249),
            bg_secondary: Color::rgb(243, 243, 243),
            bg_elevated: Color::WHITE,
            bg_overlay: Color::rgba(255, 255, 255, 220),
            text_primary: Color::rgb(26, 26, 26),
            text_secondary: Color::rgb(96, 96, 96),
            text_disabled: Color::rgb(170, 170, 170),
            text_on_accent: Color::WHITE,
            border_default: Color::rgb(215, 215, 215),
            border_focus: Color::rgb(0, 103, 192),
            border_error: Color::rgb(196, 13, 29),
            success: Color::rgb(13, 148, 103),
            warning: Color::rgb(196, 127, 9),
            error: Color::rgb(196, 13, 29),
            info: Color::rgb(47, 104, 197),
            transparency: Transparency::None,
            anim_speed: AnimSpeed::Normal,
            corner_radius: 8,
            shadow_intensity: 20,
            tokens: BTreeMap::new(),
        }
    }
}

/// The Theme Manager (runtime switching).
pub struct ThemeManager {
    /// Current active theme
    pub active: Theme,
    /// Installed themes
    pub themes: Vec<Theme>,
    /// Accent color override
    pub accent_override: Option<Color>,
    /// Scheduled auto-switch times (dark_start_hour, dark_end_hour)
    pub auto_schedule: (u8, u8),
    /// Change listeners (silo IDs that want theme change notifications)
    pub listeners: Vec<u64>,
    /// Theme switch count
    pub switches: u64,
}

impl ThemeManager {
    pub fn new() -> Self {
        let dark = Theme::qindows_dark();
        let light = Theme::qindows_light();

        ThemeManager {
            active: dark.clone(),
            themes: alloc::vec![dark, light],
            accent_override: None,
            auto_schedule: (19, 7), // Dark from 7pm to 7am
            listeners: Vec::new(),
            switches: 0,
        }
    }

    /// Switch to a theme by name.
    pub fn switch_to(&mut self, name: &str) -> bool {
        if let Some(theme) = self.themes.iter().find(|t| t.name == name) {
            self.active = theme.clone();
            if let Some(accent) = self.accent_override {
                self.active.accent = accent;
            }
            self.switches += 1;
            true
        } else {
            false
        }
    }

    /// Toggle dark/light mode.
    pub fn toggle_mode(&mut self) {
        let target = if self.active.mode == ThemeMode::Dark { "Qindows Light" } else { "Qindows Dark" };
        self.switch_to(target);
    }

    /// Set accent color (applies to current and future themes).
    pub fn set_accent(&mut self, color: Color) {
        self.accent_override = Some(color);
        self.active.accent = color;
        self.active.accent_hover = color.lighten(0.15);
        self.active.accent_pressed = color.darken(0.15);
        self.active.border_focus = color;
    }

    /// Install a custom theme.
    pub fn install_theme(&mut self, theme: Theme) {
        self.themes.push(theme);
    }

    /// Auto-switch based on hour of day.
    pub fn auto_switch(&mut self, current_hour: u8) {
        let (dark_start, dark_end) = self.auto_schedule;
        let should_be_dark = if dark_start > dark_end {
            current_hour >= dark_start || current_hour < dark_end
        } else {
            current_hour >= dark_start && current_hour < dark_end
        };

        let is_dark = self.active.mode == ThemeMode::Dark;
        if should_be_dark && !is_dark {
            self.switch_to("Qindows Dark");
        } else if !should_be_dark && is_dark {
            self.switch_to("Qindows Light");
        }
    }

    /// Get a resolved color (with accent override applied).
    pub fn color(&self, name: &str) -> Color {
        match name {
            "accent" => self.active.accent,
            "bg" | "bg_primary" => self.active.bg_primary,
            "bg_secondary" => self.active.bg_secondary,
            "text" | "text_primary" => self.active.text_primary,
            "text_secondary" => self.active.text_secondary,
            "border" => self.active.border_default,
            "success" => self.active.success,
            "warning" => self.active.warning,
            "error" => self.active.error,
            _ => {
                self.active.tokens.get(name)
                    .and_then(|v| parse_color_hex(v))
                    .unwrap_or(Color::BLACK)
            }
        }
    }
}

/// Parse a hex color string (#RRGGBB or #RRGGBBAA).
fn parse_color_hex(hex: &str) -> Option<Color> {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 { return None; }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    let a = if hex.len() >= 8 {
        u8::from_str_radix(&hex[6..8], 16).ok()?
    } else { 255 };
    Some(Color::rgba(r, g, b, a))
}
