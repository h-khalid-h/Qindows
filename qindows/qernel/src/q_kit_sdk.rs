//! # Q-Kit SDK — Declarative Native UI Framework (Phase 98)
//!
//! ARCHITECTURE.md §4.6 — Q-Kit SDK:
//! > "Declarative, shader-native UI framework. Developers describe *state*, GPU computes layout."
//! > "button! { label: format!(\"Clicked {} times\", count), style: ButtonStyle::GlassMorph, hover_effect: Physics::Elastic(strength: 0.5) }"
//!
//! ## Architecture Guardian: Design rationale
//! Aether (Phase 59) is the **compositor** — it receives Q-Kit command streams and renders.
//! Q-View WM (Phase 92) is the **layout engine** for windows.
//! Q-Fonts (Phase 95) provides **text rasterization**.
//! A11y (Phase 91) provides **accessibility**.
//!
//! Q-Kit SDK is the **application-facing bridge**: Silo code calls Q-Kit builders,
//! and Q-Kit translates state descriptions into Aether `QKitCmd` command streams.
//!
//! ## Design Principles
//! 1. **State-driven**: developer declares what should be visible given app state
//! 2. **Diff-based**: Q-Kit computes minimal diff → sends only changed Aether commands
//! 3. **Physics-baked**: hover/spring/elastic effects defined in the widget tree,
//!    computed by Aether — app logic never touches animation state
//! 4. **Q-Glass first**: built-in `ButtonStyle::GlassMorph` → real-time refraction
//!
//! ## Widget Types
//! Corresponds to the `QKitCmd` enum in `aether.rs` — this SDK is the **build-time API**,
//! Aether is the **render-time sink**.
//!
//! ## Law Compliance
//! - **Law 3 (Async)**: no blocking widget state mutations; all updates via Q-Ring
//! - **Law 4 (Vector)**: all widgets ultimately emit SDF Aether commands (no bitmaps)
//! - **Law 6 (Sandbox)**: each Silo has its own widget tree; no cross-Silo widget access

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Widget Style ──────────────────────────────────────────────────────────────

/// Visual style preset for buttons/panels.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetStyle {
    /// Flat solid fill
    Solid { color: u32 },
    /// Q-Glass morphic: real-time frosted-glass refraction
    GlassMorph { tint: u32, blur_radius: f32 },
    /// Outlined border only
    Outline { color: u32, width: f32 },
    /// Accent gradient (two-stop linear)
    Gradient { start: u32, end: u32, angle_deg: u16 },
    /// No background
    Transparent,
}

impl Default for WidgetStyle {
    fn default() -> Self { WidgetStyle::Solid { color: 0x2E3440FF } }
}

// ── Hover Effect ──────────────────────────────────────────────────────────────

/// Physics animation played when cursor enters a widget.
#[derive(Debug, Clone, PartialEq)]
pub enum HoverEffect {
    None,
    /// Elastic spring: widget grows slightly then settles
    Elastic { strength: f32 },
    /// Fade brightness
    Dim { alpha: f32 },
    /// Glow (SDF outline expansion)
    Glow { radius: f32, color: u32 },
    /// Underline for links
    Underline,
}

impl Default for HoverEffect { fn default() -> Self { HoverEffect::None } }

// ── Press Effect ──────────────────────────────────────────────────────────────

/// Animation played on click/tap.
#[derive(Debug, Clone, PartialEq)]
pub enum PressEffect {
    None,
    /// Compress: scale to 0.95 then spring back
    Compress,
    /// Color flash
    Flash { color: u32, duration_ms: u32 },
    /// Ripple from click point (Material-style)
    Ripple { color: u32 },
}

impl Default for PressEffect { fn default() -> Self { PressEffect::Compress } }

// ── Widget Events ─────────────────────────────────────────────────────────────

/// Event types that a widget can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetEvent {
    Clicked,
    Hovered,
    Unhovered,
    FocusGained,
    FocusLost,
    TextChanged,
    ValueChanged,
    Scrolled,
    Submitted, // Enter key in text input
}

// ── Widget Descriptor ─────────────────────────────────────────────────────────

/// Declared widget state — what the app describes to Q-Kit.
#[derive(Debug, Clone)]
pub struct WidgetDesc {
    /// Stable ID (used for diff — same ID = same widget across state changes)
    pub id: u32,
    /// Widget kind
    pub kind: WidgetKind,
    /// Layout constraints
    pub layout: LayoutConstraint,
    /// Visual style
    pub style: WidgetStyle,
    /// Hover effect
    pub hover_effect: HoverEffect,
    /// Press effect
    pub press_effect: PressEffect,
    /// Opacity (0.0-1.0)
    pub opacity: f32,
    /// Accessibility label
    pub a11y_label: Option<String>,
    /// Is this widget enabled?
    pub enabled: bool,
    /// Child widgets (ordered)
    pub children: Vec<WidgetDesc>,
}

impl WidgetDesc {
    pub fn new(id: u32, kind: WidgetKind) -> Self {
        WidgetDesc {
            id,
            kind,
            layout: LayoutConstraint::default(),
            style: WidgetStyle::default(),
            hover_effect: HoverEffect::default(),
            press_effect: PressEffect::default(),
            opacity: 1.0,
            a11y_label: None,
            enabled: true,
            children: Vec::new(),
        }
    }

    pub fn with_style(mut self, s: WidgetStyle) -> Self { self.style = s; self }
    pub fn with_hover(mut self, h: HoverEffect) -> Self { self.hover_effect = h; self }
    pub fn with_press(mut self, p: PressEffect) -> Self { self.press_effect = p; self }
    pub fn with_a11y(mut self, label: &str) -> Self { self.a11y_label = Some(label.to_string()); self }
    pub fn with_layout(mut self, l: LayoutConstraint) -> Self { self.layout = l; self }
    pub fn add_child(mut self, child: WidgetDesc) -> Self { self.children.push(child); self }
    pub fn disabled(mut self) -> Self { self.enabled = false; self }
}

// ── Widget Kind ───────────────────────────────────────────────────────────────

/// What kind of widget this is.
#[derive(Debug, Clone, PartialEq)]
pub enum WidgetKind {
    /// Rectangular container
    Container,
    /// Text label (static)
    Label { text: String, font_size: f32, color: u32 },
    /// Clickable button
    Button { label: String, font_size: f32 },
    /// Text input field
    TextInput { placeholder: String, current_value: String, max_len: u32 },
    /// Checkbox
    Checkbox { checked: bool, label: String },
    /// Horizontal slider
    Slider { value: f32, min: f32, max: f32 },
    /// Progress bar
    ProgressBar { progress: f32 },
    /// Image (Prism OID)
    Image { oid: [u8; 32] },
    /// Scroll container
    ScrollView { scroll_x: f32, scroll_y: f32 },
    /// Horizontal stack
    HStack { spacing: f32 },
    /// Vertical stack
    VStack { spacing: f32 },
    /// Spacer (flexible gap)
    Spacer,
    /// Separator line
    Divider { color: u32 },
}

// ── Layout Constraint ─────────────────────────────────────────────────────────

/// CSS-like layout constraints for a widget.
#[derive(Debug, Clone)]
pub struct LayoutConstraint {
    pub width: SizeSpec,
    pub height: SizeSpec,
    pub padding: [f32; 4],  // top, right, bottom, left
    pub margin: [f32; 4],
    pub align: Alignment,
}

impl Default for LayoutConstraint {
    fn default() -> Self {
        LayoutConstraint {
            width: SizeSpec::FillParent,
            height: SizeSpec::WrapContent,
            padding: [8.0, 8.0, 8.0, 8.0],
            margin: [0.0, 0.0, 0.0, 0.0],
            align: Alignment::Start,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SizeSpec {
    Fixed(f32),
    FillParent,
    WrapContent,
    Fraction(f32), // fraction of parent
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alignment { Start, Center, End, Stretch }

// ── Aether Command Stream (output of Q-Kit layout engine) ─────────────────────

/// A compiled Aether command ready for compositor submission.
#[derive(Debug, Clone)]
pub enum AetherCmd {
    FillRect  { x: f32, y: f32, w: f32, h: f32, color: u32, corner_radius: f32 },
    DrawText  { x: f32, y: f32, text: String, font_size: f32, color: u32 },
    BlurRect  { x: f32, y: f32, w: f32, h: f32, radius: f32, tint: u32 },
    DrawImage { x: f32, y: f32, w: f32, h: f32, oid: [u8; 32] },
    SetOpacity{ widget_id: u32, opacity: f32 },
    Scissor   { x: f32, y: f32, w: f32, h: f32 },
    ResetScissor,
}

// ── Computed Layout ───────────────────────────────────────────────────────────

/// The positioned result of one widget in the layout pass.
#[derive(Debug, Clone)]
pub struct PositionedWidget {
    pub id: u32,
    pub x: f32, pub y: f32,
    pub w: f32, pub h: f32,
    pub cmds: Vec<AetherCmd>,
}

// ── SDK Statistics ────────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct QKitStats {
    pub layout_passes: u64,
    pub widgets_laid_out: u64,
    pub aether_commands_emitted: u64,
    pub diffs_applied: u64,
}

// ── Q-Kit SDK Engine ──────────────────────────────────────────────────────────

/// Q-Kit declarative UI layout and compilation engine.
pub struct QKitEngine {
    /// Previous widget tree (for diff)
    pub last_tree: BTreeMap<u32, PositionedWidget>,
    pub stats: QKitStats,
}

impl QKitEngine {
    pub fn new() -> Self {
        QKitEngine { last_tree: BTreeMap::new(), stats: QKitStats::default() }
    }

    /// Layout a WidgetDesc tree and produce Aether commands.
    /// `x0, y0` = top-left origin, `parent_w, parent_h` = available space.
    pub fn layout(&mut self, root: &WidgetDesc, x0: f32, y0: f32, parent_w: f32, parent_h: f32)
        -> Vec<PositionedWidget>
    {
        let mut result = Vec::new();
        self.layout_widget(root, x0, y0, parent_w, parent_h, &mut result);
        self.stats.layout_passes += 1;
        self.stats.widgets_laid_out += result.len() as u64;
        result
    }

    fn layout_widget(
        &mut self,
        desc: &WidgetDesc,
        x: f32, y: f32,
        avail_w: f32, avail_h: f32,
        out: &mut Vec<PositionedWidget>,
    ) -> (f32, f32) // (consumed_w, consumed_h)
    {
        let pad = desc.layout.padding;
        let mar = desc.layout.margin;
        let x = x + mar[3]; let y = y + mar[0];

        let w = match desc.layout.width {
            SizeSpec::Fixed(v)    => v,
            SizeSpec::FillParent  => avail_w - mar[1] - mar[3],
            SizeSpec::Fraction(f) => avail_w * f,
            SizeSpec::WrapContent => avail_w - mar[1] - mar[3], // approximate
        };

        let h_hint = match desc.layout.height {
            SizeSpec::Fixed(v)    => v,
            SizeSpec::WrapContent => 32.0, // default line height estimate
            SizeSpec::FillParent  => avail_h - mar[0] - mar[2],
            SizeSpec::Fraction(f) => avail_h * f,
        };

        // Build Aether commands for this widget
        let mut cmds: Vec<AetherCmd> = Vec::new();

        match &desc.style {
            WidgetStyle::Solid { color } =>
                cmds.push(AetherCmd::FillRect { x, y, w, h: h_hint, color: *color, corner_radius: 4.0 }),
            WidgetStyle::GlassMorph { tint, blur_radius } =>
                cmds.push(AetherCmd::BlurRect { x, y, w, h: h_hint, radius: *blur_radius, tint: *tint }),
            WidgetStyle::Outline { color, width: bw } =>
                cmds.push(AetherCmd::FillRect { x, y, w, h: h_hint, color: *color, corner_radius: *bw }),
            WidgetStyle::Gradient { start, .. } =>
                cmds.push(AetherCmd::FillRect { x, y, w, h: h_hint, color: *start, corner_radius: 0.0 }),
            WidgetStyle::Transparent => {}
        }

        if desc.opacity < 1.0 {
            cmds.push(AetherCmd::SetOpacity { widget_id: desc.id, opacity: desc.opacity });
        }

        // Emit content command based on widget kind
        let content_x = x + pad[3];
        let content_y = y + pad[0];
        let mut content_h = h_hint;

        match &desc.kind {
            WidgetKind::Label { text, font_size, color } => {
                cmds.push(AetherCmd::DrawText {
                    x: content_x, y: content_y,
                    text: text.clone(), font_size: *font_size, color: *color,
                });
            }
            WidgetKind::Button { label, font_size } => {
                cmds.push(AetherCmd::DrawText {
                    x: content_x, y: content_y,
                    text: label.clone(), font_size: *font_size, color: 0xFFFFFFFF,
                });
            }
            WidgetKind::Image { oid } => {
                cmds.push(AetherCmd::DrawImage {
                    x: content_x, y: content_y,
                    w: w - pad[1] - pad[3], h: h_hint - pad[0] - pad[2],
                    oid: *oid,
                });
            }
            WidgetKind::ScrollView { .. } => {
                cmds.push(AetherCmd::Scissor { x, y, w, h: h_hint });
            }
            _ => {}
        }

        // Layout children (VStack/HStack)
        let mut child_y = content_y;
        let mut child_x = content_x;
        let avail_inner_w = w - pad[1] - pad[3];
        let avail_inner_h = h_hint - pad[0] - pad[2];

        let spacing = match &desc.kind {
            WidgetKind::VStack { spacing } | WidgetKind::HStack { spacing } => *spacing,
            _ => 0.0,
        };
        let horizontal = matches!(&desc.kind, WidgetKind::HStack { .. });

        for child in &desc.children {
            let (cw, ch) = self.layout_widget(child, child_x, child_y, avail_inner_w, avail_inner_h, out);
            if horizontal { child_x += cw + spacing; } else { child_y += ch + spacing; }
        }

        if matches!(&desc.kind, WidgetKind::ScrollView { .. }) {
            cmds.push(AetherCmd::ResetScissor);
        }

        // Dynamic height: grow to fit children for VStack
        if !horizontal && !desc.children.is_empty() {
            content_h = child_y - content_y + pad[2];
        }

        let total_h = content_h.max(h_hint);
        self.stats.aether_commands_emitted += cmds.len() as u64;

        out.push(PositionedWidget { id: desc.id, x, y, w, h: total_h, cmds });
        (w + mar[1] + mar[3], total_h + mar[0] + mar[2])
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Q-Kit SDK Engine (§4.6)            ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Layout passes: {:>6}                ║", self.stats.layout_passes);
        crate::serial_println!("║ Widgets laid:  {:>6}K               ║", self.stats.widgets_laid_out / 1000);
        crate::serial_println!("║ Aether cmds:   {:>6}K               ║", self.stats.aether_commands_emitted / 1000);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
