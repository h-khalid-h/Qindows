//! # Aether Accessibility Layer — A11y Support (Phase 91)
//!
//! ARCHITECTURE.md §4 — Aether Compositor: Accessibility:
//! > "Aether maintains a11y tree for assistive tech (screen readers, switch access)"
//! > "Every SDF node has an optional A11yRole"
//! > "Screen magnification is zero-cost: just apply a uniform SDF scale transform"
//!
//! ## Architecture Guardian: Design
//! Traditional accessibility in GUIs:
//! - Platform maintains a parallel "accessibility tree" (DOM-like)
//! - Screen readers query this tree via OS accessibility APIs
//! - Problem: the tree often gets out of sync with the visual render tree
//!
//! Qindows / Aether advantage:
//! - The scene graph **IS** the accessibility tree — every SDF node has an A11yRole
//! - No parallel tree, no sync issues
//! - Screen magnification: apply SDF `scale(2.0)` — geometrically perfect, no pixelation
//! - The same vector descriptions that drive the visual compositor drive TTS descriptions
//!
//! ## Law Compliance
//! - **Law 4 (Vector-Native UI)**: accessibility is vector-native by definition
//!   (SDF scale = free, zero quality loss)
//! - **Law 1 (Zero-Ambient Authority)**: screen reader requires A11Y_READ CapToken
//!   (other Silos cannot read your private accessible labels without your permission)
//! - **Law 6 (Silo Sandbox)**: the a11y tree is per-Silo, scoped to its window

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── A11y Role ─────────────────────────────────────────────────────────────────

/// Semantic role of an SDF node, following ARIA-like semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum A11yRole {
    /// Container with no semantic meaning
    None,
    /// Window root (corresponds to a Silo window)
    Window,
    /// Main content region
    Main,
    /// Navigation region
    Nav,
    /// Button (clickable action element)
    Button,
    /// Text label
    Label,
    /// Text input field
    TextInput,
    /// Checkbox (two-state control)
    Checkbox,
    /// Radio button (mutually exclusive option)
    Radio,
    /// List container
    List,
    /// List item
    ListItem,
    /// Link / hypertext anchor
    Link,
    /// Progress indicator
    ProgressBar,
    /// Slider / range input
    Slider,
    /// Image with content
    Image,
    /// Decorative image (screen reader skips this)
    DecorativeImage,
    /// Live region (auto-announced when content changes — alerts, notifications)
    LiveRegion,
    /// Status bar
    Status,
    /// Alert (urgent live region)
    Alert,
    /// Dialog/modal (traps keyboard focus)
    Dialog,
    /// Tab in a tabpanel
    Tab,
    /// Tabpanel content region
    TabPanel,
    /// Menu
    Menu,
    /// Menu item
    MenuItem,
    /// Separator / divider
    Separator,
    /// Tooltip
    Tooltip,
    /// Custom role with ARIA string
    AriaCustom(String),
}

impl A11yRole {
    pub fn is_interactive(&self) -> bool {
        matches!(self,
            Self::Button | Self::TextInput | Self::Checkbox | Self::Radio |
            Self::Link | Self::Slider | Self::Tab | Self::MenuItem
        )
    }

    pub fn is_landmark(&self) -> bool {
        matches!(self, Self::Window | Self::Main | Self::Nav | Self::Dialog)
    }

    pub fn aria_name(&self) -> &str {
        match self {
            Self::None              => "none",
            Self::Window            => "dialog",
            Self::Main              => "main",
            Self::Nav               => "navigation",
            Self::Button            => "button",
            Self::Label             => "label",
            Self::TextInput         => "textbox",
            Self::Checkbox          => "checkbox",
            Self::Radio             => "radio",
            Self::List              => "list",
            Self::ListItem          => "listitem",
            Self::Link              => "link",
            Self::ProgressBar       => "progressbar",
            Self::Slider            => "slider",
            Self::Image             => "img",
            Self::DecorativeImage   => "presentation",
            Self::LiveRegion        => "region",
            Self::Status            => "status",
            Self::Alert             => "alert",
            Self::Dialog            => "dialog",
            Self::Tab               => "tab",
            Self::TabPanel          => "tabpanel",
            Self::Menu              => "menu",
            Self::MenuItem          => "menuitem",
            Self::Separator         => "separator",
            Self::Tooltip           => "tooltip",
            Self::AriaCustom(s)     => s.as_str(),
        }
    }
}

// ── A11y State ────────────────────────────────────────────────────────────────

/// Computed state of an A11y node (changes while Silo runs).
#[derive(Debug, Clone, Default)]
pub struct A11yState {
    /// Is the node focused?
    pub focused: bool,
    /// Is the node checked? (for Checkbox/Radio)
    pub checked: Option<bool>,
    /// Is the node pressed? (for Button)
    pub pressed: bool,
    /// Is the node disabled?
    pub disabled: bool,
    /// Is the node expanded? (for menus, disclosures)
    pub expanded: Option<bool>,
    /// Current value (for Slider/ProgressBar, 0.0-1.0)
    pub value: Option<f32>,
    /// Current text (for TextInput)
    pub text_value: Option<String>,
    /// For LiveRegion: has this changed since last a11y poll?
    pub live_changed: bool,
}

// ── A11y Node ─────────────────────────────────────────────────────────────────

/// An accessibility node mirroring one SDF scene node.
#[derive(Debug, Clone)]
pub struct A11yNode {
    /// The SDF node ID this mirrors
    pub sdf_node_id: u32,
    /// Role
    pub role: A11yRole,
    /// Accessible name (read by screen reader as label)
    pub name: String,
    /// Accessible description (extra context)
    pub description: String,
    /// Current state
    pub state: A11yState,
    /// Parent node ID (0 = root)
    pub parent_id: u32,
    /// Child node IDs in document order
    pub children: Vec<u32>,
    /// Tab order index (-1 = not focusable)
    pub tab_index: i32,
    /// Keyboard shortcut hint (e.g. "Ctrl+S")
    pub keyboard_shortcut: Option<String>,
    /// Bounding box in screen coordinates [x, y, w, h] for magnification/pointer
    pub bounds: [f32; 4],
}

impl A11yNode {
    /// Compute a TTS announcement string for this node (when focused).
    pub fn tts_label(&self) -> String {
        let mut s = self.name.clone();
        s.push(' ');
        s.push_str(self.role.aria_name());
        if self.state.disabled { s.push_str(", dimmed"); }
        if let Some(checked) = self.state.checked {
            s.push_str(if checked { ", checked" } else { ", unchecked" });
        }
        if let Some(exp) = self.state.expanded {
            s.push_str(if exp { ", expanded" } else { ", collapsed" });
        }
        if let Some(v) = self.state.value {
            s.push_str(&alloc::format!(", {}%", (v * 100.0) as u32));
        }
        s
    }
}

// ── A11y Tree ─────────────────────────────────────────────────────────────────

/// The full accessibility tree for one Silo window.
#[derive(Debug, Clone, Default)]
pub struct A11yTree {
    /// Silo ID this tree belongs to
    pub silo_id: u64,
    /// All nodes: sdf_node_id → A11yNode
    pub nodes: BTreeMap<u32, A11yNode>,
    /// Currently focused node ID
    pub focused_node: Option<u32>,
    /// Tab order: ordered list of focusable node IDs
    pub tab_order: Vec<u32>,
    /// Current tab position (index into tab_order)
    pub tab_position: usize,
    /// Live announcements pending for screen reader (FIFO)
    pub pending_announcements: Vec<String>,
}

impl A11yTree {
    pub fn new(silo_id: u64) -> Self {
        A11yTree { silo_id, ..Default::default() }
    }

    /// Insert or update a node in the tree.
    pub fn upsert(&mut self, node: A11yNode) {
        let id = node.sdf_node_id;
        // Update parent's child list if needed
        if node.parent_id != 0 {
            if let Some(parent) = self.nodes.get_mut(&node.parent_id) {
                if !parent.children.contains(&id) {
                    parent.children.push(id);
                }
            }
        }
        // Rebuild tab order if needed
        if node.tab_index >= 0 {
            if !self.tab_order.contains(&id) {
                self.tab_order.push(id);
                self.tab_order.sort_by_key(|nid| {
                    self.nodes.get(nid).map(|n| n.tab_index).unwrap_or(i32::MAX)
                });
            }
        }
        self.nodes.insert(id, node);
    }

    /// Remove a node and clean up references.
    pub fn remove(&mut self, sdf_node_id: u32) {
        self.nodes.remove(&sdf_node_id);
        self.tab_order.retain(|&id| id != sdf_node_id);
        if self.focused_node == Some(sdf_node_id) { self.focused_node = None; }
    }

    /// Move focus to next Tab target.
    pub fn tab_next(&mut self) -> Option<&A11yNode> {
        if self.tab_order.is_empty() { return None; }
        self.tab_position = (self.tab_position + 1) % self.tab_order.len();
        let id = self.tab_order[self.tab_position];
        self.focused_node = Some(id);
        if let Some(node) = self.nodes.get_mut(&id) {
            node.state.focused = true;
        }
        let label = self.nodes.get(&id).map(|n| n.tts_label());
        if let Some(l) = label { self.pending_announcements.push(l); }
        self.nodes.get(&id)
    }

    /// Move focus to previous Tab target.
    pub fn tab_prev(&mut self) -> Option<&A11yNode> {
        if self.tab_order.is_empty() { return None; }
        self.tab_position = if self.tab_position == 0 {
            self.tab_order.len() - 1
        } else { self.tab_position - 1 };
        let id = self.tab_order[self.tab_position];
        self.focused_node = Some(id);
        let label = self.nodes.get(&id).map(|n| n.tts_label());
        if let Some(l) = label { self.pending_announcements.push(l); }
        self.nodes.get(&id)
    }

    /// Update a node's state and queue a live announcement if role demands it.
    pub fn update_state(&mut self, sdf_node_id: u32, new_state: A11yState) {
        if let Some(node) = self.nodes.get_mut(&sdf_node_id) {
            let old_live = node.state.live_changed;
            node.state = new_state;
            if node.state.live_changed && !old_live {
                // Live region changed — queue announcement
                self.pending_announcements.push(node.tts_label());
            }
        }
    }

    /// Drain pending TTS announcements (called by screen reader Silo via poll).
    pub fn drain_announcements(&mut self) -> Vec<String> {
        core::mem::take(&mut self.pending_announcements)
    }
}

// ── Magnification State ───────────────────────────────────────────────────────

/// Current screen magnification state (applied by Aether compositor).
#[derive(Debug, Clone)]
pub struct MagnificationState {
    /// Scale factor (1.0 = no magnification, 4.0 = 4× zoom)
    pub scale: f32,
    /// Focus point in screen coordinates (magnifier centers here)
    pub focus_x: f32,
    pub focus_y: f32,
    /// Smooth tracking (follow keyboard focus automatically)
    pub follow_focus: bool,
    /// High-contrast mode
    pub high_contrast: bool,
    /// Reduced motion mode (disables fade/slide animations)
    pub reduced_motion: bool,
}

impl Default for MagnificationState {
    fn default() -> Self {
        MagnificationState {
            scale: 1.0,
            focus_x: 0.0,
            focus_y: 0.0,
            follow_focus: true,
            high_contrast: false,
            reduced_motion: false,
        }
    }
}

// ── A11y Layer Statistics ─────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct A11yStats {
    pub trees_registered: u64,
    pub nodes_total: u64,
    pub tab_navigations: u64,
    pub announcements_queued: u64,
    pub magnification_changes: u64,
}

// ── Aether A11y Layer ─────────────────────────────────────────────────────────

/// The Aether compositor accessibility layer.
pub struct AetherA11yLayer {
    /// Per-Silo accessibility trees
    pub trees: BTreeMap<u64, A11yTree>,
    /// Current magnification (global, affects all Silos)
    pub magnification: MagnificationState,
    /// Statistics
    pub stats: A11yStats,
    /// Which Silo's tree is currently active for keyboard a11y navigation
    pub active_silo: Option<u64>,
}

impl AetherA11yLayer {
    pub fn new() -> Self {
        AetherA11yLayer {
            trees: BTreeMap::new(),
            magnification: MagnificationState::default(),
            stats: A11yStats::default(),
            active_silo: None,
        }
    }

    /// Register a Silo's a11y tree (called when Silo window is created).
    pub fn register_silo(&mut self, silo_id: u64) {
        self.trees.insert(silo_id, A11yTree::new(silo_id));
        self.stats.trees_registered += 1;
        crate::serial_println!("[A11Y] Silo {} a11y tree registered.", silo_id);
    }

    /// Update a node in a Silo's a11y tree (called on every SDF scene update).
    pub fn upsert_node(&mut self, silo_id: u64, node: A11yNode) {
        if let Some(tree) = self.trees.get_mut(&silo_id) {
            tree.upsert(node);
            self.stats.nodes_total += 1;
        }
    }

    /// Screen reader poll: drain pending announcements from active Silo.
    pub fn poll_announcements(&mut self, silo_id: u64) -> Vec<String> {
        self.trees.get_mut(&silo_id)
            .map(|t| { let v = t.drain_announcements(); self.stats.announcements_queued += v.len() as u64; v })
            .unwrap_or_default()
    }

    /// Tab key press: move focus forward in active Silo.
    pub fn tab_next(&mut self) -> Option<String> {
        let silo_id = self.active_silo?;
        let tree = self.trees.get_mut(&silo_id)?;
        let node = tree.tab_next()?;
        self.stats.tab_navigations += 1;
        Some(node.tts_label())
    }

    /// Set magnification (user accessibility setting).
    pub fn set_magnification(&mut self, scale: f32, high_contrast: bool, reduced_motion: bool) {
        self.magnification.scale = scale.max(1.0).min(8.0);
        self.magnification.high_contrast = high_contrast;
        self.magnification.reduced_motion = reduced_motion;
        self.stats.magnification_changes += 1;
        crate::serial_println!(
            "[A11Y] Magnification: {:.1}× contrast={} reduced_motion={}",
            scale, high_contrast, reduced_motion
        );
    }

    /// Remove a Silo's tree (called when Silo window is vaporized).
    pub fn unregister_silo(&mut self, silo_id: u64) {
        self.trees.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Aether A11y Layer (§4)             ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Silo trees:   {:>6}                 ║", self.trees.len());
        crate::serial_println!("║ Tab navigations:{:>6}               ║", self.stats.tab_navigations);
        crate::serial_println!("║ Announcements:{:>6}                 ║", self.stats.announcements_queued);
        crate::serial_println!("║ Zoom:         {:>5.1}×               ║", self.magnification.scale);
        crate::serial_println!("║ High contrast:{:>6}                 ║", self.magnification.high_contrast);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
