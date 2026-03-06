//! # Task Switcher — Alt-Tab with Spatial Preview
//!
//! Rich task switching experience with live window
//! thumbnails and spatial arrangement (Section 5.3).
//!
//! Features:
//! - MRU (most-recently-used) ordering
//! - Live thumbnail previews
//! - Per-Silo task lists (isolated view)
//! - Quick-switch (last two tasks)
//! - Spatial layout (arrange by virtual desktop)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Task entry in the switcher.
#[derive(Debug, Clone)]
pub struct SwitcherEntry {
    pub window_id: u64,
    pub silo_id: u64,
    pub title: String,
    pub app_name: String,
    pub desktop_id: u32,
    pub last_focused: u64,
    pub thumbnail_hash: [u8; 32],
    pub pinned: bool,
}

/// Switcher state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwitcherState {
    Hidden,
    Showing,
    Selecting,
}

/// Switcher statistics.
#[derive(Debug, Clone, Default)]
pub struct SwitcherStats {
    pub activations: u64,
    pub switches: u64,
    pub quick_switches: u64,
}

/// The Task Switcher.
pub struct TaskSwitcher {
    /// MRU-ordered task list per Silo
    pub entries: BTreeMap<u64, Vec<SwitcherEntry>>,
    pub state: SwitcherState,
    pub selected_index: usize,
    pub active_silo: u64,
    pub stats: SwitcherStats,
}

impl TaskSwitcher {
    pub fn new() -> Self {
        TaskSwitcher {
            entries: BTreeMap::new(),
            state: SwitcherState::Hidden,
            selected_index: 0,
            active_silo: 0,
            stats: SwitcherStats::default(),
        }
    }

    /// Register a window.
    pub fn register(&mut self, window_id: u64, silo_id: u64, title: &str, app: &str, desktop: u32, now: u64) {
        let list = self.entries.entry(silo_id).or_insert_with(Vec::new);
        list.push(SwitcherEntry {
            window_id, silo_id, title: String::from(title),
            app_name: String::from(app), desktop_id: desktop,
            last_focused: now, thumbnail_hash: [0u8; 32], pinned: false,
        });
    }

    /// Unregister a window.
    pub fn unregister(&mut self, silo_id: u64, window_id: u64) {
        if let Some(list) = self.entries.get_mut(&silo_id) {
            list.retain(|e| e.window_id != window_id);
        }
    }

    /// Focus a window (moves it to MRU front).
    pub fn focus(&mut self, silo_id: u64, window_id: u64, now: u64) {
        if let Some(list) = self.entries.get_mut(&silo_id) {
            if let Some(pos) = list.iter().position(|e| e.window_id == window_id) {
                list[pos].last_focused = now;
                let entry = list.remove(pos);
                list.insert(0, entry);
            }
        }
    }

    /// Activate the switcher (Alt-Tab pressed).
    pub fn activate(&mut self, silo_id: u64) {
        self.active_silo = silo_id;
        self.state = SwitcherState::Showing;
        self.selected_index = 1; // Start at second item (first is current)
        self.stats.activations += 1;
    }

    /// Move selection forward.
    pub fn next(&mut self) {
        if let Some(list) = self.entries.get(&self.active_silo) {
            if !list.is_empty() {
                self.selected_index = (self.selected_index + 1) % list.len();
                self.state = SwitcherState::Selecting;
            }
        }
    }

    /// Move selection backward.
    pub fn prev(&mut self) {
        if let Some(list) = self.entries.get(&self.active_silo) {
            if !list.is_empty() {
                self.selected_index = if self.selected_index == 0 {
                    list.len() - 1
                } else {
                    self.selected_index - 1
                };
                self.state = SwitcherState::Selecting;
            }
        }
    }

    /// Confirm selection (Alt released).
    pub fn confirm(&mut self) -> Option<u64> {
        let result = self.entries.get(&self.active_silo)
            .and_then(|list| list.get(self.selected_index))
            .map(|e| e.window_id);

        self.state = SwitcherState::Hidden;
        self.selected_index = 0;

        if result.is_some() {
            self.stats.switches += 1;
        }
        result
    }

    /// Quick switch to last-used window.
    pub fn quick_switch(&mut self, silo_id: u64) -> Option<u64> {
        let list = self.entries.get(&silo_id)?;
        if list.len() < 2 {
            return None;
        }
        let window_id = list[1].window_id;
        self.stats.quick_switches += 1;
        Some(window_id)
    }
}
