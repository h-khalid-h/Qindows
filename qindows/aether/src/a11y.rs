//! # Aether Accessibility System
//!
//! Provides screen reader support, keyboard navigation,
//! high-contrast modes, and accessible element tree for
//! assistive technology integration.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Accessible role (WAI-ARIA inspired).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessRole {
    Window,
    Dialog,
    Button,
    Link,
    TextInput,
    Checkbox,
    RadioButton,
    Slider,
    ProgressBar,
    Menu,
    MenuItem,
    Tab,
    TabPanel,
    List,
    ListItem,
    TreeItem,
    Image,
    Heading,
    Label,
    Tooltip,
    Alert,
    Status,
    Separator,
    Group,
    Toolbar,
    ScrollBar,
    Grid,
    GridCell,
}

/// Accessible state flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct AccessState {
    pub focused: bool,
    pub selected: bool,
    pub checked: bool,
    pub disabled: bool,
    pub expanded: bool,
    pub hidden: bool,
    pub pressed: bool,
    pub required: bool,
    pub readonly: bool,
    pub invalid: bool,
}

/// An accessible element in the tree.
#[derive(Debug, Clone)]
pub struct AccessElement {
    /// Unique ID
    pub id: u64,
    /// Role
    pub role: AccessRole,
    /// Human-readable name (screen reader label)
    pub name: String,
    /// Description / help text
    pub description: Option<String>,
    /// State
    pub state: AccessState,
    /// Value (for sliders, progress bars, text inputs)
    pub value: Option<String>,
    /// Value range (min, max, current) for numeric controls
    pub value_range: Option<(f32, f32, f32)>,
    /// Keyboard shortcut
    pub shortcut: Option<String>,
    /// Children element IDs
    pub children: Vec<u64>,
    /// Parent element ID
    pub parent: Option<u64>,
    /// Bounds (x, y, width, height)
    pub bounds: (i32, i32, u32, u32),
    /// Live region (for dynamic content announcements)
    pub live: LiveRegion,
}

/// Live region behavior (for dynamic updates).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRegion {
    Off,
    Polite,      // Announce when idle
    Assertive,   // Announce immediately
}

impl Default for LiveRegion {
    fn default() -> Self { LiveRegion::Off }
}

/// Screen reader announcement.
#[derive(Debug, Clone)]
pub struct Announcement {
    pub text: String,
    pub priority: LiveRegion,
    pub timestamp: u64,
}

/// Accessibility configuration.
#[derive(Debug, Clone)]
pub struct AccessConfig {
    /// Screen reader enabled
    pub screen_reader: bool,
    /// High contrast mode
    pub high_contrast: bool,
    /// Reduce motion
    pub reduce_motion: bool,
    /// Larger text scale factor (1.0 = normal)
    pub text_scale: f32,
    /// Keyboard navigation mode
    pub keyboard_nav: bool,
    /// Caret browsing
    pub caret_browsing: bool,
    /// Sticky keys
    pub sticky_keys: bool,
    /// Focus highlight thickness (px)
    pub focus_ring_width: u8,
}

impl Default for AccessConfig {
    fn default() -> Self {
        AccessConfig {
            screen_reader: false,
            high_contrast: false,
            reduce_motion: false,
            text_scale: 1.0,
            keyboard_nav: true,
            caret_browsing: false,
            sticky_keys: false,
            focus_ring_width: 3,
        }
    }
}

/// The Accessibility Manager.
pub struct AccessManager {
    /// Element tree
    pub elements: BTreeMap<u64, AccessElement>,
    /// Root element ID
    pub root_id: u64,
    /// Currently focused element
    pub focus_id: Option<u64>,
    /// Configuration
    pub config: AccessConfig,
    /// Pending announcements
    pub announcements: Vec<Announcement>,
    /// Tab order (element IDs in tab sequence)
    pub tab_order: Vec<u64>,
    /// Next element ID
    next_id: u64,
}

impl AccessManager {
    pub fn new() -> Self {
        let mut mgr = AccessManager {
            elements: BTreeMap::new(),
            root_id: 0,
            focus_id: None,
            config: AccessConfig::default(),
            announcements: Vec::new(),
            tab_order: Vec::new(),
            next_id: 1,
        };

        // Create root element
        let root = AccessElement {
            id: 0,
            role: AccessRole::Window,
            name: String::from("Qindows Desktop"),
            description: None,
            state: AccessState::default(),
            value: None,
            value_range: None,
            shortcut: None,
            children: Vec::new(),
            parent: None,
            bounds: (0, 0, 1920, 1080),
            live: LiveRegion::Off,
        };
        mgr.elements.insert(0, root);

        mgr
    }

    /// Register an accessible element.
    pub fn register(&mut self, role: AccessRole, name: &str, parent_id: u64, bounds: (i32, i32, u32, u32)) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let elem = AccessElement {
            id,
            role,
            name: String::from(name),
            description: None,
            state: AccessState::default(),
            value: None,
            value_range: None,
            shortcut: None,
            children: Vec::new(),
            parent: Some(parent_id),
            bounds,
            live: LiveRegion::Off,
        };

        self.elements.insert(id, elem);

        if let Some(parent) = self.elements.get_mut(&parent_id) {
            parent.children.push(id);
        }

        // Add focusable elements to tab order
        match role {
            AccessRole::Button | AccessRole::Link | AccessRole::TextInput
            | AccessRole::Checkbox | AccessRole::RadioButton | AccessRole::Slider
            | AccessRole::MenuItem | AccessRole::Tab => {
                self.tab_order.push(id);
            }
            _ => {}
        }

        id
    }

    /// Remove an element.
    pub fn unregister(&mut self, id: u64) {
        if let Some(elem) = self.elements.remove(&id) {
            if let Some(parent_id) = elem.parent {
                if let Some(parent) = self.elements.get_mut(&parent_id) {
                    parent.children.retain(|&c| c != id);
                }
            }
        }
        self.tab_order.retain(|&t| t != id);
        if self.focus_id == Some(id) {
            self.focus_id = None;
        }
    }

    /// Move focus to the next element in tab order.
    pub fn focus_next(&mut self) -> Option<u64> {
        if self.tab_order.is_empty() { return None; }

        let current_idx = self.focus_id
            .and_then(|fid| self.tab_order.iter().position(|&id| id == fid))
            .unwrap_or(self.tab_order.len() - 1);

        let next_idx = (current_idx + 1) % self.tab_order.len();
        let next_id = self.tab_order[next_idx];

        // Skip disabled elements
        if let Some(elem) = self.elements.get(&next_id) {
            if elem.state.disabled {
                self.focus_id = Some(next_id);
                return self.focus_next(); // Recurse (risky if all disabled)
            }
        }

        self.set_focus(next_id);
        Some(next_id)
    }

    /// Move focus to the previous element.
    pub fn focus_prev(&mut self) -> Option<u64> {
        if self.tab_order.is_empty() { return None; }

        let current_idx = self.focus_id
            .and_then(|fid| self.tab_order.iter().position(|&id| id == fid))
            .unwrap_or(1);

        let prev_idx = if current_idx == 0 { self.tab_order.len() - 1 } else { current_idx - 1 };
        let prev_id = self.tab_order[prev_idx];

        self.set_focus(prev_id);
        Some(prev_id)
    }

    /// Set focus to a specific element.
    pub fn set_focus(&mut self, id: u64) {
        // Unfocus current
        if let Some(old_id) = self.focus_id {
            if let Some(elem) = self.elements.get_mut(&old_id) {
                elem.state.focused = false;
            }
        }

        // Focus new
        if let Some(elem) = self.elements.get_mut(&id) {
            elem.state.focused = true;
            self.focus_id = Some(id);

            // Announce if screen reader is active
            if self.config.screen_reader {
                let text = alloc::format!("{}: {}", format_role(elem.role), elem.name);
                self.announce(&text, LiveRegion::Polite, 0);
            }
        }
    }

    /// Queue an announcement for the screen reader.
    pub fn announce(&mut self, text: &str, priority: LiveRegion, now: u64) {
        self.announcements.push(Announcement {
            text: String::from(text),
            priority,
            timestamp: now,
        });
    }

    /// Pop pending announcements.
    pub fn pop_announcements(&mut self) -> Vec<Announcement> {
        let anns = self.announcements.clone();
        self.announcements.clear();
        anns
    }

    /// Get the focused element's description for screen reader.
    pub fn describe_focus(&self) -> Option<String> {
        let id = self.focus_id?;
        let elem = self.elements.get(&id)?;

        let mut desc = alloc::format!("{}", elem.name);
        if let Some(ref d) = elem.description {
            desc = alloc::format!("{}, {}", desc, d);
        }
        if elem.state.disabled { desc = alloc::format!("{}, disabled", desc); }
        if elem.state.checked { desc = alloc::format!("{}, checked", desc); }
        if let Some(ref v) = elem.value { desc = alloc::format!("{}, value: {}", desc, v); }

        Some(desc)
    }
}

fn format_role(role: AccessRole) -> &'static str {
    match role {
        AccessRole::Button => "Button",
        AccessRole::Link => "Link",
        AccessRole::TextInput => "Text field",
        AccessRole::Checkbox => "Checkbox",
        AccessRole::Slider => "Slider",
        AccessRole::Menu => "Menu",
        AccessRole::MenuItem => "Menu item",
        AccessRole::Heading => "Heading",
        AccessRole::Alert => "Alert",
        AccessRole::Dialog => "Dialog",
        _ => "Element",
    }
}
