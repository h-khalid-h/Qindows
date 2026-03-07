//! # Aether Context Menu System
//!
//! Right-click context menus with nested submenus,
//! keyboard shortcuts, icons, separators, and dividers.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// A menu item.
#[derive(Debug, Clone)]
pub enum MenuItem {
    /// A clickable action
    Action {
        id: u64,
        label: String,
        icon: Option<String>,
        shortcut: Option<String>,
        enabled: bool,
        checked: Option<bool>,
    },
    /// A separator line
    Separator,
    /// A submenu
    Submenu {
        label: String,
        icon: Option<String>,
        items: Vec<MenuItem>,
    },
    /// A section header (non-clickable label)
    Header(String),
}

impl MenuItem {
    /// Create an action item.
    pub fn action(id: u64, label: &str) -> Self {
        MenuItem::Action {
            id,
            label: String::from(label),
            icon: None,
            shortcut: None,
            enabled: true,
            checked: None,
        }
    }

    /// With keyboard shortcut.
    pub fn with_shortcut(self, shortcut: &str) -> Self {
        match self {
            MenuItem::Action { id, label, icon, enabled, checked, .. } => {
                MenuItem::Action { id, label, icon, shortcut: Some(String::from(shortcut)), enabled, checked }
            }
            other => other,
        }
    }

    /// With icon name.
    pub fn with_icon(self, icon: &str) -> Self {
        match self {
            MenuItem::Action { id, label, shortcut, enabled, checked, .. } => {
                MenuItem::Action { id, label, icon: Some(String::from(icon)), shortcut, enabled, checked }
            }
            other => other,
        }
    }

    /// Set disabled.
    pub fn disabled(self) -> Self {
        match self {
            MenuItem::Action { id, label, icon, shortcut, checked, .. } => {
                MenuItem::Action { id, label, icon, shortcut, enabled: false, checked }
            }
            other => other,
        }
    }

    /// Set checkmark state.
    pub fn checked(self, state: bool) -> Self {
        match self {
            MenuItem::Action { id, label, icon, shortcut, enabled, .. } => {
                MenuItem::Action { id, label, icon, shortcut, enabled, checked: Some(state) }
            }
            other => other,
        }
    }
}

/// A context menu.
#[derive(Debug, Clone)]
pub struct ContextMenu {
    /// Menu items
    pub items: Vec<MenuItem>,
    /// Position (x, y)
    pub x: f32,
    pub y: f32,
    /// Width (auto-calculated or fixed)
    pub width: f32,
    /// Is the menu visible?
    pub visible: bool,
    /// Currently highlighted item index
    pub highlight: Option<usize>,
    /// Active submenu index
    pub active_submenu: Option<usize>,
    /// Owner widget ID
    pub owner: u64,
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, owner: u64) -> Self {
        ContextMenu {
            items,
            x: 0.0, y: 0.0,
            width: 200.0,
            visible: false,
            highlight: None,
            active_submenu: None,
            owner,
        }
    }

    /// Show at position.
    pub fn show(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.visible = true;
        self.highlight = None;
        self.active_submenu = None;
    }

    /// Hide the menu.
    pub fn hide(&mut self) {
        self.visible = false;
        self.highlight = None;
        self.active_submenu = None;
    }

    /// Get the item height (for rendering).
    pub fn item_height(item: &MenuItem) -> f32 {
        match item {
            MenuItem::Separator => 8.0,
            MenuItem::Header(_) => 28.0,
            _ => 32.0,
        }
    }

    /// Total height of the menu.
    pub fn total_height(&self) -> f32 {
        self.items.iter().map(Self::item_height).sum()
    }

    /// Hit-test: which item is at this y position?
    pub fn hit_test(&self, test_y: f32) -> Option<usize> {
        let mut y = self.y;
        for (i, item) in self.items.iter().enumerate() {
            let h = Self::item_height(item);
            if test_y >= y && test_y < y + h {
                match item {
                    MenuItem::Separator | MenuItem::Header(_) => return None,
                    _ => return Some(i),
                }
            }
            y += h;
        }
        None
    }

    /// Navigate up.
    pub fn navigate_up(&mut self) {
        let count = self.items.len();
        if count == 0 { return; }

        let mut idx = self.highlight.unwrap_or(0);
        loop {
            idx = if idx == 0 { count - 1 } else { idx - 1 };
            if matches!(self.items[idx], MenuItem::Action { .. } | MenuItem::Submenu { .. }) {
                break;
            }
            if idx == self.highlight.unwrap_or(0) { break; }
        }
        self.highlight = Some(idx);
    }

    /// Navigate down.
    pub fn navigate_down(&mut self) {
        let count = self.items.len();
        if count == 0 { return; }

        let mut idx = self.highlight.map(|i| i + 1).unwrap_or(0);
        if idx >= count { idx = 0; }
        loop {
            if matches!(self.items[idx], MenuItem::Action { .. } | MenuItem::Submenu { .. }) {
                break;
            }
            idx = (idx + 1) % count;
        }
        self.highlight = Some(idx);
    }

    /// Activate highlighted item. Returns action ID if clicked.
    pub fn activate(&mut self) -> Option<u64> {
        let idx = self.highlight?;
        let (is_action_enabled, action_id, is_submenu) = match &self.items[idx] {
            MenuItem::Action { id, enabled, .. } => (*enabled, Some(*id), false),
            MenuItem::Submenu { .. } => (false, None, true),
            _ => (false, None, false),
        };

        if is_action_enabled {
            self.hide();
            action_id
        } else if is_submenu {
            self.active_submenu = Some(idx);
            None
        } else {
            None
        }
    }
}

/// The Context Menu Manager.
pub struct ContextMenuManager {
    /// Active menus
    pub menus: Vec<ContextMenu>,
    /// Next menu action callback
    pub last_action: Option<u64>,
}

impl ContextMenuManager {
    pub fn new() -> Self {
        ContextMenuManager {
            menus: Vec::new(),
            last_action: None,
        }
    }

    /// Show a context menu.
    pub fn show(&mut self, mut menu: ContextMenu, x: f32, y: f32) {
        // Close all existing menus
        self.dismiss_all();
        menu.show(x, y);
        self.menus.push(menu);
    }

    /// Dismiss all menus.
    pub fn dismiss_all(&mut self) {
        self.menus.clear();
    }

    /// Is any menu visible?
    pub fn is_active(&self) -> bool {
        self.menus.iter().any(|m| m.visible)
    }
}
