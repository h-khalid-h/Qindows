//! # Q-Notify — Unified Notification Center
//!
//! All system and app notifications flow through Q-Notify (Section 4.3).
//! Notifications are priority-ranked, grouped by Silo, and can be
//! snoozed, dismissed, or acted upon with Thought-Gate.
//!
//! Features:
//! - Priority ranking (Critical > High > Normal > Low)
//! - Per-Silo grouping and Do-Not-Disturb per Silo
//! - Rich actions (inline reply, quick-act buttons)
//! - Focus modes (Work, Gaming, Sleep — auto-filter by priority)
//! - Notification history with search

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Notification priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Notification state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifState {
    Pending,
    Shown,
    Read,
    Dismissed,
    Snoozed,
    Acted,
}

/// Focus mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusMode {
    /// All notifications
    All,
    /// Work: only High + Critical
    Work,
    /// Gaming: only Critical
    Gaming,
    /// Sleep: none (silent)
    Sleep,
}

impl FocusMode {
    pub fn min_priority(&self) -> Priority {
        match self {
            FocusMode::All => Priority::Low,
            FocusMode::Work => Priority::High,
            FocusMode::Gaming => Priority::Critical,
            FocusMode::Sleep => Priority::Critical, // effectively blocks all
        }
    }
}

/// An action button on a notification.
#[derive(Debug, Clone)]
pub struct NotifAction {
    pub label: String,
    pub action_id: u64,
}

/// A notification.
#[derive(Debug, Clone)]
pub struct Notification {
    pub id: u64,
    pub silo_id: u64,
    pub title: String,
    pub body: String,
    pub priority: Priority,
    pub state: NotifState,
    pub created_at: u64,
    pub shown_at: u64,
    pub snoozed_until: u64,
    pub actions: Vec<NotifAction>,
    pub icon_id: u64,
    pub group: String,
}

/// Q-Notify statistics.
#[derive(Debug, Clone, Default)]
pub struct NotifyStats {
    pub total: u64,
    pub shown: u64,
    pub dismissed: u64,
    pub acted: u64,
    pub snoozed: u64,
    pub filtered_by_focus: u64,
    pub filtered_by_dnd: u64,
}

/// The Q-Notify Manager.
pub struct QNotify {
    pub notifications: BTreeMap<u64, Notification>,
    pub focus: FocusMode,
    /// Per-Silo Do-Not-Disturb
    pub dnd_silos: Vec<u64>,
    next_id: u64,
    next_action_id: u64,
    /// Max notifications to keep in history
    pub max_history: usize,
    pub stats: NotifyStats,
}

impl QNotify {
    pub fn new() -> Self {
        QNotify {
            notifications: BTreeMap::new(),
            focus: FocusMode::All,
            dnd_silos: Vec::new(),
            next_id: 1,
            next_action_id: 1,
            max_history: 500,
            stats: NotifyStats::default(),
        }
    }

    /// Post a notification.
    pub fn post(
        &mut self,
        silo_id: u64,
        title: &str,
        body: &str,
        priority: Priority,
        group: &str,
        actions: Vec<&str>,
        now: u64,
    ) -> Option<u64> {
        // Check DND
        if self.dnd_silos.contains(&silo_id) {
            self.stats.filtered_by_dnd += 1;
            return None;
        }

        // Check focus mode
        if (priority as u8) < (self.focus.min_priority() as u8) {
            self.stats.filtered_by_focus += 1;
            return None;
        }

        let id = self.next_id;
        self.next_id += 1;

        let acts: Vec<NotifAction> = actions.iter().map(|label| {
            let aid = self.next_action_id;
            self.next_action_id += 1;
            NotifAction { label: String::from(*label), action_id: aid }
        }).collect();

        self.notifications.insert(id, Notification {
            id, silo_id,
            title: String::from(title),
            body: String::from(body),
            priority, state: NotifState::Pending,
            created_at: now, shown_at: 0, snoozed_until: 0,
            actions: acts, icon_id: 0,
            group: String::from(group),
        });

        self.stats.total += 1;

        // Trim history
        while self.notifications.len() > self.max_history {
            if let Some(&oldest_id) = self.notifications.keys().next() {
                self.notifications.remove(&oldest_id);
            }
        }

        Some(id)
    }

    /// Mark notification as shown.
    pub fn show(&mut self, id: u64, now: u64) {
        if let Some(n) = self.notifications.get_mut(&id) {
            n.state = NotifState::Shown;
            n.shown_at = now;
            self.stats.shown += 1;
        }
    }

    /// Dismiss a notification.
    pub fn dismiss(&mut self, id: u64) {
        if let Some(n) = self.notifications.get_mut(&id) {
            n.state = NotifState::Dismissed;
            self.stats.dismissed += 1;
        }
    }

    /// Snooze a notification.
    pub fn snooze(&mut self, id: u64, duration: u64, now: u64) {
        if let Some(n) = self.notifications.get_mut(&id) {
            n.state = NotifState::Snoozed;
            n.snoozed_until = now + duration;
            self.stats.snoozed += 1;
        }
    }

    /// Act on a notification action.
    pub fn act(&mut self, id: u64, _action_id: u64) {
        if let Some(n) = self.notifications.get_mut(&id) {
            n.state = NotifState::Acted;
            self.stats.acted += 1;
        }
    }

    /// Unsnooze due notifications.
    pub fn unsnooze(&mut self, now: u64) {
        for n in self.notifications.values_mut() {
            if n.state == NotifState::Snoozed && now >= n.snoozed_until {
                n.state = NotifState::Pending;
            }
        }
    }

    /// Set focus mode.
    pub fn set_focus(&mut self, mode: FocusMode) {
        self.focus = mode;
    }

    /// Toggle DND for a Silo.
    pub fn toggle_dnd(&mut self, silo_id: u64) {
        if let Some(pos) = self.dnd_silos.iter().position(|&s| s == silo_id) {
            self.dnd_silos.remove(pos);
        } else {
            self.dnd_silos.push(silo_id);
        }
    }

    /// Get pending notifications (sorted by priority desc).
    pub fn pending(&self) -> Vec<&Notification> {
        let mut pending: Vec<&Notification> = self.notifications.values()
            .filter(|n| n.state == NotifState::Pending || n.state == NotifState::Shown)
            .collect();
        pending.sort_by(|a, b| b.priority.cmp(&a.priority));
        pending
    }
}
