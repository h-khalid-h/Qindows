//! # Accessibility Engine — Universal Access Layer
//!
//! Ensures Qindows is usable by everyone, including users with
//! visual, motor, auditory, or cognitive disabilities (Section 4.6).
//!
//! Features:
//! - Screen reader with semantic tree traversal
//! - Dynamic magnification (zoom lens follows gaze/cursor)
//! - High-contrast mode with customizable color filters
//! - Reduced motion mode (disables animations)
//! - Switch control (single-button scanning input)
//! - Haptic feedback patterns for non-visual cues

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Accessibility feature flags.
#[derive(Debug, Clone)]
pub struct A11yFlags {
    pub screen_reader: bool,
    pub magnification: bool,
    pub high_contrast: bool,
    pub reduced_motion: bool,
    pub switch_control: bool,
    pub haptic_feedback: bool,
    pub large_text: bool,
    pub mono_audio: bool,
}

impl Default for A11yFlags {
    fn default() -> Self {
        A11yFlags {
            screen_reader: false, magnification: false,
            high_contrast: false, reduced_motion: false,
            switch_control: false, haptic_feedback: false,
            large_text: false, mono_audio: false,
        }
    }
}

/// Semantic role of a UI element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SemanticRole {
    Window,
    Button,
    TextField,
    Label,
    Image,
    List,
    ListItem,
    Slider,
    Checkbox,
    Menu,
    MenuItem,
    Heading,
    Link,
    Container,
}

/// A node in the accessibility tree.
#[derive(Debug, Clone)]
pub struct A11yNode {
    pub id: u64,
    pub role: SemanticRole,
    pub label: String,
    pub value: String,
    pub focused: bool,
    pub enabled: bool,
    pub children: Vec<u64>,
    pub parent: Option<u64>,
    pub bounds_x: i32,
    pub bounds_y: i32,
    pub bounds_w: u32,
    pub bounds_h: u32,
}

/// Magnification state.
#[derive(Debug, Clone)]
pub struct MagState {
    pub enabled: bool,
    pub zoom: f32,
    pub center_x: i32,
    pub center_y: i32,
    pub follow_cursor: bool,
}

/// Accessibility statistics.
#[derive(Debug, Clone, Default)]
pub struct A11yStats {
    pub nodes_registered: u64,
    pub announcements: u64,
    pub focus_changes: u64,
    pub scans_completed: u64,
}

/// The Accessibility Engine.
pub struct AccessibilityEngine {
    pub flags: A11yFlags,
    pub tree: BTreeMap<u64, A11yNode>,
    pub focus_id: Option<u64>,
    pub mag: MagState,
    /// Announcement queue (screen reader speaks these)
    pub announce_queue: Vec<String>,
    /// Text scale multiplier
    pub text_scale: f32,
    next_id: u64,
    pub stats: A11yStats,
}

impl AccessibilityEngine {
    pub fn new() -> Self {
        AccessibilityEngine {
            flags: A11yFlags::default(),
            tree: BTreeMap::new(),
            focus_id: None,
            mag: MagState { enabled: false, zoom: 2.0, center_x: 0, center_y: 0, follow_cursor: true },
            announce_queue: Vec::new(),
            text_scale: 1.0,
            next_id: 1,
            stats: A11yStats::default(),
        }
    }

    /// Register a UI element in the accessibility tree.
    pub fn register(&mut self, role: SemanticRole, label: &str, parent: Option<u64>, x: i32, y: i32, w: u32, h: u32) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        if let Some(pid) = parent {
            if let Some(p) = self.tree.get_mut(&pid) {
                p.children.push(id);
            }
        }

        self.tree.insert(id, A11yNode {
            id, role, label: String::from(label), value: String::new(),
            focused: false, enabled: true, children: Vec::new(), parent,
            bounds_x: x, bounds_y: y, bounds_w: w, bounds_h: h,
        });

        self.stats.nodes_registered += 1;
        id
    }

    /// Move focus to a node.
    pub fn focus(&mut self, id: u64) {
        // Unfocus current
        if let Some(old_id) = self.focus_id {
            if let Some(node) = self.tree.get_mut(&old_id) {
                node.focused = false;
            }
        }

        if let Some(node) = self.tree.get_mut(&id) {
            node.focused = true;
            self.focus_id = Some(id);
            self.stats.focus_changes += 1;

            // Announce for screen reader
            if self.flags.screen_reader {
                let role_str = match node.role {
                    SemanticRole::Button => "Button",
                    SemanticRole::TextField => "Text field",
                    SemanticRole::Checkbox => "Checkbox",
                    SemanticRole::Heading => "Heading",
                    SemanticRole::Link => "Link",
                    SemanticRole::Slider => "Slider",
                    SemanticRole::MenuItem => "Menu item",
                    _ => "Element",
                };
                let mut announcement = String::from(role_str);
                announcement.push_str(": ");
                announcement.push_str(&node.label);
                self.announce_queue.push(announcement);
                self.stats.announcements += 1;
            }

            // Update magnification center
            if self.mag.enabled && self.mag.follow_cursor {
                self.mag.center_x = node.bounds_x + (node.bounds_w as i32 / 2);
                self.mag.center_y = node.bounds_y + (node.bounds_h as i32 / 2);
            }
        }
    }

    /// Navigate to next focusable sibling.
    pub fn focus_next(&mut self) {
        let current = match self.focus_id {
            Some(id) => id,
            None => {
                if let Some(&first) = self.tree.keys().next() {
                    self.focus(first);
                }
                return;
            }
        };

        let keys: Vec<u64> = self.tree.keys().copied().collect();
        if let Some(pos) = keys.iter().position(|&k| k == current) {
            let next = if pos + 1 < keys.len() { keys[pos + 1] } else { keys[0] };
            self.focus(next);
        }
    }

    /// Enable/disable features.
    pub fn set_flags(&mut self, flags: A11yFlags) {
        if flags.large_text {
            self.text_scale = 1.5;
        } else {
            self.text_scale = 1.0;
        }
        self.mag.enabled = flags.magnification;
        self.flags = flags;
    }

    /// Drain announcements for the screen reader.
    pub fn drain_announcements(&mut self) -> Vec<String> {
        core::mem::take(&mut self.announce_queue)
    }
}
