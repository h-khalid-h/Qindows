//! # Aether Focus Manager
//!
//! Manages keyboard/input focus across the Qindows window
//! hierarchy with tab-order, focus rings, and z-order
//! integration (Section 4.9).
//!
//! Features:
//! - Window-level and widget-level focus tracking
//! - Tab order navigation (forward/backward)
//! - Focus ring visual indicators
//! - Focus steal prevention (per-Silo policy)
//! - Focus change event notifications
//! - Z-order aware focus cycling

extern crate alloc;

use alloc::vec::Vec;

/// Focus target type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusTarget {
    Window(u64),
    Widget { window_id: u64, widget_id: u64 },
}

/// Focus change reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusReason {
    Click,
    Tab,
    ShiftTab,
    Programmatic,
    WindowActivation,
}

/// A focusable element.
#[derive(Debug, Clone)]
pub struct FocusEntry {
    pub target: FocusTarget,
    pub silo_id: u64,
    pub tab_index: i32,
    pub focusable: bool,
    pub can_steal: bool,
}

/// Focus manager statistics.
#[derive(Debug, Clone, Default)]
pub struct FocusStats {
    pub focus_changes: u64,
    pub tab_navigations: u64,
    pub steal_attempts: u64,
    pub steal_blocked: u64,
}

/// The Focus Manager.
pub struct FocusManager {
    /// All focusable elements, keyed by window then widget
    pub entries: Vec<FocusEntry>,
    /// Currently focused target
    pub current: Option<FocusTarget>,
    /// Previous focus (for restore)
    pub previous: Option<FocusTarget>,
    /// Current focus index in entries
    pub focus_index: Option<usize>,
    pub stats: FocusStats,
}

impl FocusManager {
    pub fn new() -> Self {
        FocusManager {
            entries: Vec::new(),
            current: None,
            previous: None,
            focus_index: None,
            stats: FocusStats::default(),
        }
    }

    /// Register a focusable element.
    pub fn register(&mut self, entry: FocusEntry) {
        self.entries.push(entry);
        // Sort by tab_index (negative = natural order)
        self.entries.sort_by_key(|e| e.tab_index);
    }

    /// Unregister all elements for a window.
    pub fn unregister_window(&mut self, window_id: u64) {
        self.entries.retain(|e| match e.target {
            FocusTarget::Window(id) => id != window_id,
            FocusTarget::Widget { window_id: wid, .. } => wid != window_id,
        });
        // Reset focus if it was in the removed window
        if let Some(FocusTarget::Window(id)) = self.current {
            if id == window_id { self.current = None; self.focus_index = None; }
        }
        if let Some(FocusTarget::Widget { window_id: wid, .. }) = self.current {
            if wid == window_id { self.current = None; self.focus_index = None; }
        }
    }

    /// Set focus to a specific target.
    pub fn set_focus(&mut self, target: FocusTarget, reason: FocusReason, requester_silo: u64) -> bool {
        // Find the target entry
        let idx = self.entries.iter().position(|e| e.target == target);
        let entry = match idx {
            Some(i) => &self.entries[i],
            None => return false,
        };

        if !entry.focusable { return false; }

        // Check steal prevention
        if let Some(current) = self.current {
            if current != target {
                let current_silo = self.entries.iter()
                    .find(|e| e.target == current)
                    .map(|e| e.silo_id);

                if let Some(csid) = current_silo {
                    if csid != requester_silo && !entry.can_steal {
                        self.stats.steal_attempts += 1;
                        self.stats.steal_blocked += 1;
                        return false;
                    }
                }
                self.stats.steal_attempts += 1;
            }
        }

        self.previous = self.current;
        self.current = Some(target);
        self.focus_index = idx;
        self.stats.focus_changes += 1;

        if matches!(reason, FocusReason::Tab | FocusReason::ShiftTab) {
            self.stats.tab_navigations += 1;
        }
        true
    }

    /// Tab to next focusable element.
    pub fn tab_next(&mut self, requester_silo: u64) -> Option<FocusTarget> {
        if self.entries.is_empty() { return None; }

        let focusable: Vec<usize> = self.entries.iter()
            .enumerate()
            .filter(|(_, e)| e.focusable)
            .map(|(i, _)| i)
            .collect();

        if focusable.is_empty() { return None; }

        let current_pos = self.focus_index
            .and_then(|fi| focusable.iter().position(|&i| i == fi))
            .unwrap_or(focusable.len() - 1);

        let next_pos = (current_pos + 1) % focusable.len();
        let next_target = self.entries[focusable[next_pos]].target;

        if self.set_focus(next_target, FocusReason::Tab, requester_silo) {
            Some(next_target)
        } else { None }
    }

    /// Tab to previous focusable element.
    pub fn tab_prev(&mut self, requester_silo: u64) -> Option<FocusTarget> {
        if self.entries.is_empty() { return None; }

        let focusable: Vec<usize> = self.entries.iter()
            .enumerate()
            .filter(|(_, e)| e.focusable)
            .map(|(i, _)| i)
            .collect();

        if focusable.is_empty() { return None; }

        let current_pos = self.focus_index
            .and_then(|fi| focusable.iter().position(|&i| i == fi))
            .unwrap_or(0);

        let prev_pos = if current_pos == 0 { focusable.len() - 1 } else { current_pos - 1 };
        let prev_target = self.entries[focusable[prev_pos]].target;

        if self.set_focus(prev_target, FocusReason::ShiftTab, requester_silo) {
            Some(prev_target)
        } else { None }
    }

    /// Get the currently focused target.
    pub fn focused(&self) -> Option<FocusTarget> {
        self.current
    }
}
