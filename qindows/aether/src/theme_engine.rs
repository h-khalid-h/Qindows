//! # Aether Dynamic Theme Engine
//!
//! Generates and applies UI themes dynamically.
//! Supports accent color extraction from wallpaper, dark/light modes,
//! custom palettes, and smooth theme transitions.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// RGBA color (0.0 - 1.0 per channel).
#[derive(Debug, Clone, Copy)]
pub struct ThemeColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl ThemeColor {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        ThemeColor { r, g, b, a }
    }
    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        ThemeColor { r, g, b, a: 1.0 }
    }

    /// Blend this color toward another by t (0.0 - 1.0).
    pub fn lerp(&self, other: &ThemeColor, t: f32) -> ThemeColor {
        ThemeColor {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    /// Compute perceived luminance.
    pub fn luminance(&self) -> f32 {
        0.2126 * self.r + 0.7152 * self.g + 0.0722 * self.b
    }

    /// Darken by factor.
    pub fn darken(&self, factor: f32) -> ThemeColor {
        ThemeColor {
            r: self.r * (1.0 - factor),
            g: self.g * (1.0 - factor),
            b: self.b * (1.0 - factor),
            a: self.a,
        }
    }

    /// Lighten by factor.
    pub fn lighten(&self, factor: f32) -> ThemeColor {
        ThemeColor {
            r: self.r + (1.0 - self.r) * factor,
            g: self.g + (1.0 - self.g) * factor,
            b: self.b + (1.0 - self.b) * factor,
            a: self.a,
        }
    }
}

/// Theme mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeMode {
    Light,
    Dark,
    Auto, // Based on time of day
}

/// A complete UI theme.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Theme name
    pub name: String,
    /// Mode
    pub mode: ThemeMode,
    /// Primary accent color
    pub accent: ThemeColor,
    /// Secondary accent
    pub accent_secondary: ThemeColor,
    /// Background colors
    pub bg_primary: ThemeColor,
    pub bg_secondary: ThemeColor,
    pub bg_tertiary: ThemeColor,
    /// Surface colors (cards, panels)
    pub surface: ThemeColor,
    pub surface_hover: ThemeColor,
    pub surface_active: ThemeColor,
    /// Text colors
    pub text_primary: ThemeColor,
    pub text_secondary: ThemeColor,
    pub text_disabled: ThemeColor,
    /// Border color
    pub border: ThemeColor,
    /// Error / warning / success / info
    pub error: ThemeColor,
    pub warning: ThemeColor,
    pub success: ThemeColor,
    pub info: ThemeColor,
    /// Shadow opacity
    pub shadow_opacity: f32,
    /// Corner radius (dp)
    pub corner_radius: f32,
    /// Glass blur amount
    pub glass_blur: f32,
    /// Glass opacity
    pub glass_opacity: f32,
}

impl Theme {
    /// Default Qindows dark theme.
    pub fn qindows_dark() -> Self {
        let accent = ThemeColor::rgb(0.024, 0.839, 0.627); // Qindows teal

        Theme {
            name: String::from("Qindows Dark"),
            mode: ThemeMode::Dark,
            accent,
            accent_secondary: ThemeColor::rgb(0.016, 0.576, 0.867),
            bg_primary: ThemeColor::rgb(0.067, 0.067, 0.082),
            bg_secondary: ThemeColor::rgb(0.098, 0.098, 0.118),
            bg_tertiary: ThemeColor::rgb(0.137, 0.137, 0.157),
            surface: ThemeColor::rgba(0.118, 0.118, 0.141, 0.9),
            surface_hover: ThemeColor::rgba(0.157, 0.157, 0.180, 0.9),
            surface_active: ThemeColor::rgba(0.196, 0.196, 0.220, 0.9),
            text_primary: ThemeColor::rgb(0.949, 0.949, 0.969),
            text_secondary: ThemeColor::rgb(0.650, 0.650, 0.690),
            text_disabled: ThemeColor::rgb(0.400, 0.400, 0.440),
            border: ThemeColor::rgba(0.300, 0.300, 0.350, 0.5),
            error: ThemeColor::rgb(0.937, 0.267, 0.267),
            warning: ThemeColor::rgb(0.980, 0.749, 0.176),
            success: ThemeColor::rgb(0.196, 0.843, 0.294),
            info: ThemeColor::rgb(0.259, 0.647, 0.961),
            shadow_opacity: 0.4,
            corner_radius: 8.0,
            glass_blur: 20.0,
            glass_opacity: 0.15,
        }
    }

    /// Default Qindows light theme.
    pub fn qindows_light() -> Self {
        let accent = ThemeColor::rgb(0.012, 0.663, 0.498);

        Theme {
            name: String::from("Qindows Light"),
            mode: ThemeMode::Light,
            accent,
            accent_secondary: ThemeColor::rgb(0.012, 0.459, 0.694),
            bg_primary: ThemeColor::rgb(0.969, 0.969, 0.976),
            bg_secondary: ThemeColor::rgb(0.941, 0.941, 0.953),
            bg_tertiary: ThemeColor::rgb(0.910, 0.910, 0.925),
            surface: ThemeColor::rgba(1.0, 1.0, 1.0, 0.95),
            surface_hover: ThemeColor::rgba(0.965, 0.965, 0.975, 0.95),
            surface_active: ThemeColor::rgba(0.930, 0.930, 0.945, 0.95),
            text_primary: ThemeColor::rgb(0.086, 0.086, 0.106),
            text_secondary: ThemeColor::rgb(0.380, 0.380, 0.420),
            text_disabled: ThemeColor::rgb(0.620, 0.620, 0.660),
            border: ThemeColor::rgba(0.700, 0.700, 0.740, 0.3),
            error: ThemeColor::rgb(0.863, 0.196, 0.196),
            warning: ThemeColor::rgb(0.929, 0.694, 0.125),
            success: ThemeColor::rgb(0.153, 0.682, 0.376),
            info: ThemeColor::rgb(0.208, 0.518, 0.894),
            shadow_opacity: 0.15,
            corner_radius: 8.0,
            glass_blur: 16.0,
            glass_opacity: 0.08,
        }
    }

    /// Generate a theme from an accent color.
    pub fn from_accent(accent: ThemeColor, mode: ThemeMode) -> Self {
        let is_dark = mode == ThemeMode::Dark
            || (mode == ThemeMode::Auto && true); // Would check time

        if is_dark {
            let mut theme = Self::qindows_dark();
            theme.accent = accent;
            theme.accent_secondary = accent.darken(0.3);
            theme.name = String::from("Custom Dark");
            theme
        } else {
            let mut theme = Self::qindows_light();
            theme.accent = accent;
            theme.accent_secondary = accent.darken(0.2);
            theme.name = String::from("Custom Light");
            theme
        }
    }
}

/// The Theme Engine — manages active themes and transitions.
pub struct ThemeEngine {
    /// Currently active theme
    pub active: Theme,
    /// Available themes
    pub themes: Vec<Theme>,
    /// Transition progress (0.0 = no transition, 1.0 = complete)
    pub transition_progress: f32,
    /// Theme we're transitioning to
    pub transition_target: Option<Theme>,
    /// Transition speed (progress per frame)
    pub transition_speed: f32,
}

impl ThemeEngine {
    pub fn new() -> Self {
        ThemeEngine {
            active: Theme::qindows_dark(),
            themes: alloc::vec![Theme::qindows_dark(), Theme::qindows_light()],
            transition_progress: 0.0,
            transition_target: None,
            transition_speed: 0.05,
        }
    }

    /// Switch to a new theme with smooth transition.
    pub fn switch_to(&mut self, theme: Theme) {
        self.transition_target = Some(theme);
        self.transition_progress = 0.0;
    }

    /// Toggle between dark and light mode.
    pub fn toggle_mode(&mut self) {
        let new_theme = if self.active.mode == ThemeMode::Dark {
            Theme::qindows_light()
        } else {
            Theme::qindows_dark()
        };
        self.switch_to(new_theme);
    }

    /// Tick the transition (call each frame).
    pub fn update(&mut self) {
        if let Some(ref target) = self.transition_target {
            self.transition_progress += self.transition_speed;

            if self.transition_progress >= 1.0 {
                self.active = target.clone();
                self.transition_target = None;
                self.transition_progress = 0.0;
            } else {
                // Lerp all colors
                self.active.accent = self.active.accent.lerp(&target.accent, self.transition_speed);
                self.active.bg_primary = self.active.bg_primary.lerp(&target.bg_primary, self.transition_speed);
                self.active.text_primary = self.active.text_primary.lerp(&target.text_primary, self.transition_speed);
                self.active.surface = self.active.surface.lerp(&target.surface, self.transition_speed);
                self.active.border = self.active.border.lerp(&target.border, self.transition_speed);
            }
        }
    }

    /// Get the current accent color for rendering.
    pub fn accent(&self) -> ThemeColor {
        self.active.accent
    }
}
