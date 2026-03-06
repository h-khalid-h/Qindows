//! # Widget Toolkit — Native UI Controls for Q-Shell
//!
//! Provides a set of native UI widgets that integrate with
//! the Aether compositor (Section 4.12).
//!
//! Features:
//! - Buttons, labels, text inputs, checkboxes, sliders
//! - Layout containers (stack, grid)
//! - Theme-aware styling
//! - Keyboard + mouse + touch input handling
//! - Per-Silo widget trees

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Widget type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetType {
    Button,
    Label,
    TextInput,
    Checkbox,
    Slider,
    Panel,
    StackLayout,
    GridLayout,
    Separator,
    Image,
}

/// Widget state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WidgetState {
    Normal,
    Hovered,
    Pressed,
    Focused,
    Disabled,
}

/// A UI widget.
#[derive(Debug, Clone)]
pub struct Widget {
    pub id: u64,
    pub widget_type: WidgetType,
    pub state: WidgetState,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub text: String,
    pub visible: bool,
    pub enabled: bool,
    pub parent_id: Option<u64>,
    pub children: Vec<u64>,
    /// For sliders: current value (0-100)
    pub value: i32,
    /// For checkboxes: checked state
    pub checked: bool,
}

/// Widget toolkit statistics.
#[derive(Debug, Clone, Default)]
pub struct ToolkitStats {
    pub widgets_created: u64,
    pub widgets_destroyed: u64,
    pub events_handled: u64,
    pub layouts_computed: u64,
}

/// The Widget Toolkit.
pub struct WidgetToolkit {
    pub widgets: BTreeMap<u64, Widget>,
    next_id: u64,
    pub focus_id: Option<u64>,
    pub stats: ToolkitStats,
}

impl WidgetToolkit {
    pub fn new() -> Self {
        WidgetToolkit {
            widgets: BTreeMap::new(),
            next_id: 1,
            focus_id: None,
            stats: ToolkitStats::default(),
        }
    }

    /// Create a widget.
    pub fn create(&mut self, wtype: WidgetType, x: i32, y: i32, w: u32, h: u32, text: &str, parent: Option<u64>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.widgets.insert(id, Widget {
            id, widget_type: wtype, state: WidgetState::Normal,
            x, y, width: w, height: h, text: String::from(text),
            visible: true, enabled: true, parent_id: parent,
            children: Vec::new(), value: 0, checked: false,
        });

        // Add to parent's children
        if let Some(pid) = parent {
            if let Some(p) = self.widgets.get_mut(&pid) {
                p.children.push(id);
            }
        }

        self.stats.widgets_created += 1;
        id
    }

    /// Destroy a widget and its children.
    pub fn destroy(&mut self, id: u64) {
        // Collect children to destroy recursively
        let children = self.widgets.get(&id)
            .map(|w| w.children.clone())
            .unwrap_or_default();

        for child_id in children {
            self.destroy(child_id);
        }

        // Remove from parent
        if let Some(w) = self.widgets.get(&id) {
            if let Some(pid) = w.parent_id {
                if let Some(parent) = self.widgets.get_mut(&pid) {
                    parent.children.retain(|&cid| cid != id);
                }
            }
        }

        self.widgets.remove(&id);
        self.stats.widgets_destroyed += 1;

        if self.focus_id == Some(id) {
            self.focus_id = None;
        }
    }

    /// Set focus to a widget.
    pub fn set_focus(&mut self, id: u64) {
        if let Some(old_id) = self.focus_id {
            if let Some(w) = self.widgets.get_mut(&old_id) {
                w.state = WidgetState::Normal;
            }
        }
        if let Some(w) = self.widgets.get_mut(&id) {
            if w.enabled {
                w.state = WidgetState::Focused;
                self.focus_id = Some(id);
            }
        }
    }

    /// Hit test: find widget at coordinates.
    pub fn hit_test(&self, x: i32, y: i32) -> Option<u64> {
        // Check from last to first (top-most widget)
        self.widgets.values().rev()
            .filter(|w| w.visible && w.enabled)
            .find(|w| {
                x >= w.x && x < w.x + w.width as i32 &&
                y >= w.y && y < w.y + w.height as i32
            })
            .map(|w| w.id)
    }

    /// Set text on a widget.
    pub fn set_text(&mut self, id: u64, text: &str) {
        if let Some(w) = self.widgets.get_mut(&id) {
            w.text = String::from(text);
        }
    }

    /// Toggle checkbox.
    pub fn toggle_check(&mut self, id: u64) {
        if let Some(w) = self.widgets.get_mut(&id) {
            if w.widget_type == WidgetType::Checkbox && w.enabled {
                w.checked = !w.checked;
                self.stats.events_handled += 1;
            }
        }
    }
}
