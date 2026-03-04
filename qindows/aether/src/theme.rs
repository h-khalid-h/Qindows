//! # Aether Theme System
//!
//! Defines the visual identity of Qindows — colors, typography,
//! spacing, and material properties. Every UI element in the OS
//! draws from this centralized theme.

/// The Qindows color palette.
pub mod colors {
    /// Primary background — the deep void
    pub const BG_VOID: u32 = 0xFF_06_06_0E;
    /// Surface background — slightly elevated
    pub const BG_SURFACE: u32 = 0xFF_0D_0D_1A;
    /// Card background
    pub const BG_CARD: u32 = 0xFF_14_14_28;
    /// Elevated card
    pub const BG_ELEVATED: u32 = 0xFF_1A_1A_33;

    /// Primary accent — Qindows Cyan
    pub const ACCENT_PRIMARY: u32 = 0xFF_06_D6_A0;
    /// Secondary accent — Electric Violet
    pub const ACCENT_SECONDARY: u32 = 0xFF_7B_2F_F7;
    /// Tertiary accent — Neon Pink
    pub const ACCENT_TERTIARY: u32 = 0xFF_EF_47_6F;
    /// Warning — Amber
    pub const WARNING: u32 = 0xFF_F0_AD_4E;
    /// Error — Crimson
    pub const ERROR: u32 = 0xFF_DC_35_45;
    /// Success — Emerald
    pub const SUCCESS: u32 = 0xFF_2E_CC_71;

    /// Text primary — near-white
    pub const TEXT_PRIMARY: u32 = 0xFF_E8_E8_F0;
    /// Text secondary — muted
    pub const TEXT_SECONDARY: u32 = 0xFF_A0_A0_B8;
    /// Text disabled
    pub const TEXT_DISABLED: u32 = 0xFF_50_50_68;

    /// Q-Glass overlay — semi-transparent
    pub const GLASS_OVERLAY: u32 = 0x40_0D_0D_1A;
    /// Window border
    pub const BORDER: u32 = 0x30_FF_FF_FF;
    /// Focused border
    pub const BORDER_FOCUSED: u32 = 0xFF_06_D6_A0;

    /// Extract ARGB components
    pub fn rgba(color: u32) -> (u8, u8, u8, u8) {
        let a = ((color >> 24) & 0xFF) as u8;
        let r = ((color >> 16) & 0xFF) as u8;
        let g = ((color >> 8) & 0xFF) as u8;
        let b = (color & 0xFF) as u8;
        (r, g, b, a)
    }

    /// Blend two colors with alpha
    pub fn blend(fg: u32, bg: u32, alpha: f32) -> u32 {
        let (fr, fg_c, fb, _) = rgba(fg);
        let (br, bg_c, bb, _) = rgba(bg);

        let r = (fr as f32 * alpha + br as f32 * (1.0 - alpha)) as u8;
        let g = (fg_c as f32 * alpha + bg_c as f32 * (1.0 - alpha)) as u8;
        let b = (fb as f32 * alpha + bb as f32 * (1.0 - alpha)) as u8;

        0xFF_00_00_00 | (r as u32) << 16 | (g as u32) << 8 | b as u32
    }
}

/// Spacing system (multiples of 4px base unit).
pub mod spacing {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 16.0;
    pub const LG: f32 = 24.0;
    pub const XL: f32 = 32.0;
    pub const XXL: f32 = 48.0;
}

/// Corner radii
pub mod radii {
    pub const SM: f32 = 4.0;
    pub const MD: f32 = 8.0;
    pub const LG: f32 = 12.0;
    pub const XL: f32 = 16.0;
    pub const FULL: f32 = 9999.0; // Fully circular
}

/// Shadow presets
pub mod shadows {
    /// Subtle elevation
    pub const SUBTLE: (f32, f32, u32) = (4.0, 0.1, 0x00_00_00_00);
    /// Medium elevation (cards)
    pub const MEDIUM: (f32, f32, u32) = (12.0, 0.2, 0x00_00_00_00);
    /// Strong elevation (dialogs, menus)
    pub const STRONG: (f32, f32, u32) = (24.0, 0.3, 0x00_00_00_00);
    /// Focused window glow
    pub const FOCUS_GLOW: (f32, f32, u32) = (16.0, 0.4, 0xFF_06_D6_A0);
}

/// Window decoration measurements
pub mod window_chrome {
    /// Title bar height
    pub const TITLEBAR_HEIGHT: f32 = 36.0;
    /// Window border width
    pub const BORDER_WIDTH: f32 = 1.0;
    /// Close/minimize/maximize button size
    pub const BUTTON_SIZE: f32 = 14.0;
    /// Button spacing
    pub const BUTTON_GAP: f32 = 8.0;
    /// Padding inside title bar
    pub const TITLEBAR_PADDING: f32 = 12.0;
}

/// Taskbar measurements
pub mod taskbar {
    /// Taskbar height
    pub const HEIGHT: f32 = 48.0;
    /// Icon size in the taskbar
    pub const ICON_SIZE: f32 = 32.0;
    /// Gap between taskbar items
    pub const ITEM_GAP: f32 = 4.0;
}
