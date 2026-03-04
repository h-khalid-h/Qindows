//! # Aether Accessibility Engine
//!
//! Provides screen reader support, keyboard navigation,
//! high-contrast mode, and semantic labeling for all UI elements.
//! Every Aether widget exposes an accessibility tree node.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Accessibility role — what kind of element this is.
#[derive(Debug, Clone, Copy)]
pub enum Role {
    Window,
    Button,
    TextInput,
    Label,
    Checkbox,
    RadioButton,
    Slider,
    ProgressBar,
    MenuBar,
    Menu,
    MenuItem,
    Tab,
    TabPanel,
    List,
    ListItem,
    Tree,
    TreeItem,
    Dialog,
    Alert,
    Tooltip,
    ScrollBar,
    Image,
    Link,
    Heading,
    Separator,
    Group,
    Toolbar,
    StatusBar,
}

/// Accessibility state flags.
#[derive(Debug, Clone, Copy, Default)]
pub struct A11yState {
    pub focused: bool,
    pub selected: bool,
    pub checked: bool,
    pub expanded: bool,
    pub disabled: bool,
    pub hidden: bool,
    pub required: bool,
    pub readonly: bool,
    pub busy: bool,
}

/// A node in the accessibility tree.
#[derive(Debug, Clone)]
pub struct A11yNode {
    /// Unique ID (matches widget ID)
    pub id: u64,
    /// Role
    pub role: Role,
    /// Human-readable name (announced by screen reader)
    pub name: String,
    /// Longer description
    pub description: String,
    /// State
    pub state: A11yState,
    /// Value (for sliders, progress bars, text inputs)
    pub value: Option<String>,
    /// Value range (min, max, current) for numeric inputs
    pub value_range: Option<(f32, f32, f32)>,
    /// Keyboard shortcut
    pub shortcut: Option<String>,
    /// Children in the a11y tree
    pub children: Vec<u64>,
    /// Parent node ID
    pub parent: Option<u64>,
    /// Live region (for dynamic content updates)
    pub live: LiveRegion,
    /// Bounding rectangle (for screen reader spatial info)
    pub bounds: (f32, f32, f32, f32), // x, y, w, h
}

/// Live region behavior (for dynamic content announcements).
#[derive(Debug, Clone, Copy, Default)]
pub enum LiveRegion {
    /// No automatic announcements
    #[default]
    Off,
    /// Polite: announce when user is idle
    Polite,
    /// Assertive: announce immediately
    Assertive,
}

/// Actions that can be performed by assistive technology.
#[derive(Debug, Clone)]
pub enum A11yAction {
    /// Click/activate the element
    Click,
    /// Focus the element
    Focus,
    /// Scroll into view
    ScrollIntoView,
    /// Expand (for tree items, accordions)
    Expand,
    /// Collapse
    Collapse,
    /// Set value (for text inputs, sliders)
    SetValue(String),
    /// Select
    Select,
    /// Dismiss (for dialogs, alerts)
    Dismiss,
}

/// The Accessibility Engine.
pub struct AccessibilityEngine {
    /// The accessibility tree
    pub nodes: alloc::collections::BTreeMap<u64, A11yNode>,
    /// Currently focused node ID
    pub focus_id: Option<u64>,
    /// Screen reader enabled?
    pub screen_reader_active: bool,
    /// High contrast mode
    pub high_contrast: bool,
    /// Reduced motion
    pub reduced_motion: bool,
    /// Font scale multiplier
    pub font_scale: f32,
    /// Announcement queue (strings to be spoken)
    pub announcements: Vec<String>,
    /// Tab order (focus traversal sequence)
    pub tab_order: Vec<u64>,
}

impl AccessibilityEngine {
    pub fn new() -> Self {
        AccessibilityEngine {
            nodes: alloc::collections::BTreeMap::new(),
            focus_id: None,
            screen_reader_active: false,
            high_contrast: false,
            reduced_motion: false,
            font_scale: 1.0,
            announcements: Vec::new(),
            tab_order: Vec::new(),
        }
    }

    /// Register an accessible element.
    pub fn register(&mut self, node: A11yNode) {
        let id = node.id;
        self.nodes.insert(id, node);
        // Add to tab order if focusable
        if !self.tab_order.contains(&id) {
            self.tab_order.push(id);
        }
    }

    /// Remove an element.
    pub fn unregister(&mut self, id: u64) {
        self.nodes.remove(&id);
        self.tab_order.retain(|&x| x != id);
        if self.focus_id == Some(id) {
            self.focus_id = None;
        }
    }

    /// Move focus to next element (Tab).
    pub fn focus_next(&mut self) -> Option<u64> {
        if self.tab_order.is_empty() { return None; }

        let current_idx = self.focus_id
            .and_then(|id| self.tab_order.iter().position(|&x| x == id))
            .unwrap_or(self.tab_order.len() - 1);

        let next_idx = (current_idx + 1) % self.tab_order.len();
        let next_id = self.tab_order[next_idx];

        self.set_focus(next_id);
        Some(next_id)
    }

    /// Move focus to previous element (Shift+Tab).
    pub fn focus_prev(&mut self) -> Option<u64> {
        if self.tab_order.is_empty() { return None; }

        let current_idx = self.focus_id
            .and_then(|id| self.tab_order.iter().position(|&x| x == id))
            .unwrap_or(0);

        let prev_idx = if current_idx == 0 { self.tab_order.len() - 1 } else { current_idx - 1 };
        let prev_id = self.tab_order[prev_idx];

        self.set_focus(prev_id);
        Some(prev_id)
    }

    /// Set focus to a specific element.
    pub fn set_focus(&mut self, id: u64) {
        // Clear old focus
        if let Some(old_id) = self.focus_id {
            if let Some(old_node) = self.nodes.get_mut(&old_id) {
                old_node.state.focused = false;
            }
        }

        // Set new focus
        if let Some(node) = self.nodes.get_mut(&id) {
            node.state.focused = true;
            self.focus_id = Some(id);

            // Announce via screen reader
            if self.screen_reader_active {
                self.announce_node(id);
            }
        }
    }

    /// Announce a node via the screen reader.
    fn announce_node(&mut self, id: u64) {
        if let Some(node) = self.nodes.get(&id) {
            let mut text = String::new();

            // Role
            text.push_str(&alloc::format!("{:?}: ", node.role));

            // Name
            text.push_str(&node.name);

            // Value
            if let Some(ref val) = node.value {
                text.push_str(&alloc::format!(", value: {}", val));
            }

            // State
            if node.state.disabled { text.push_str(", disabled"); }
            if node.state.checked { text.push_str(", checked"); }
            if node.state.expanded { text.push_str(", expanded"); }

            // Shortcut
            if let Some(ref shortcut) = node.shortcut {
                text.push_str(&alloc::format!(", shortcut: {}", shortcut));
            }

            self.announcements.push(text);
        }
    }

    /// Get next announcement to speak.
    pub fn next_announcement(&mut self) -> Option<String> {
        if self.announcements.is_empty() {
            None
        } else {
            Some(self.announcements.remove(0))
        }
    }

    /// Perform an accessibility action.
    pub fn perform_action(&mut self, id: u64, action: A11yAction) -> bool {
        match action {
            A11yAction::Focus => { self.set_focus(id); true }
            A11yAction::SetValue(ref val) => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.value = Some(val.clone());
                    true
                } else { false }
            }
            A11yAction::Expand => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.state.expanded = true;
                    true
                } else { false }
            }
            A11yAction::Collapse => {
                if let Some(node) = self.nodes.get_mut(&id) {
                    node.state.expanded = false;
                    true
                } else { false }
            }
            _ => false, // Other actions dispatched to widgets
        }
    }
}
