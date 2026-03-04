//! # Notification Center
//!
//! Manages system-wide notifications from all Silos.
//! Displays toast notifications, maintains a history log,
//! and supports Do Not Disturb mode.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Notification urgency levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    /// Silent — no popup, just logged
    Silent = 0,
    /// Low — small toast, auto-dismiss in 3s
    Low = 1,
    /// Normal — standard toast, auto-dismiss in 5s
    Normal = 2,
    /// High — persistent toast until dismissed
    High = 3,
    /// Critical — full-width banner, sound, cannot be silenced by DND
    Critical = 4,
}

/// A notification.
#[derive(Debug, Clone)]
pub struct Notification {
    /// Unique notification ID
    pub id: u64,
    /// Source Silo ID
    pub silo_id: u64,
    /// Source app name
    pub app_name: String,
    /// Notification title
    pub title: String,
    /// Body text
    pub body: String,
    /// Icon OID (Prism object)
    pub icon_oid: Option<u64>,
    /// Urgency level
    pub urgency: Urgency,
    /// Creation timestamp (ticks)
    pub timestamp: u64,
    /// Has the user seen this?
    pub read: bool,
    /// Action buttons
    pub actions: Vec<NotificationAction>,
    /// Should this replace a previous notification with same app+tag?
    pub tag: Option<String>,
    /// Group key (for stacking multiple notifications)
    pub group: Option<String>,
}

/// A notification action button.
#[derive(Debug, Clone)]
pub struct NotificationAction {
    pub label: String,
    pub action_id: String,
}

/// The Notification Center.
pub struct NotificationCenter {
    /// All notifications (most recent first)
    pub notifications: Vec<Notification>,
    /// Next notification ID
    next_id: u64,
    /// Maximum stored notifications
    pub max_history: usize,
    /// Do Not Disturb mode
    pub dnd_enabled: bool,
    /// DND exceptions (these Silos can still notify even in DND)
    pub dnd_exceptions: Vec<u64>,
    /// Currently showing toast notification
    pub active_toast: Option<u64>,
    /// Toast display duration (ticks)
    pub toast_duration: u64,
    /// Toast was shown at this tick
    pub toast_shown_at: u64,
}

impl NotificationCenter {
    pub fn new() -> Self {
        NotificationCenter {
            notifications: Vec::new(),
            next_id: 1,
            max_history: 200,
            dnd_enabled: false,
            dnd_exceptions: Vec::new(),
            active_toast: None,
            toast_duration: 500, // ~5 seconds at 100 tps
            toast_shown_at: 0,
        }
    }

    /// Post a notification.
    pub fn post(&mut self, mut notif: Notification) -> u64 {
        notif.id = self.next_id;
        self.next_id += 1;

        // Replace existing notification with same tag
        if let Some(ref tag) = notif.tag {
            self.notifications.retain(|n| {
                !(n.silo_id == notif.silo_id && n.tag.as_ref() == Some(tag))
            });
        }

        let id = notif.id;
        let should_toast = self.should_show_toast(&notif);

        // Insert at front (most recent first)
        self.notifications.insert(0, notif);

        // Trim history
        if self.notifications.len() > self.max_history {
            self.notifications.truncate(self.max_history);
        }

        // Show toast if appropriate
        if should_toast {
            self.active_toast = Some(id);
            self.toast_shown_at = 0; // Would be set to now_ticks()
        }

        id
    }

    /// Check if a notification should show a toast popup.
    fn should_show_toast(&self, notif: &Notification) -> bool {
        // Critical notifications always show
        if notif.urgency == Urgency::Critical {
            return true;
        }

        // DND blocks non-critical
        if self.dnd_enabled {
            return self.dnd_exceptions.contains(&notif.silo_id);
        }

        // Silent notifications never toast
        notif.urgency > Urgency::Silent
    }

    /// Dismiss the active toast.
    pub fn dismiss_toast(&mut self) {
        self.active_toast = None;
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, id: u64) {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.read = true;
        }
    }

    /// Mark all as read.
    pub fn mark_all_read(&mut self) {
        for n in &mut self.notifications {
            n.read = true;
        }
    }

    /// Clear all notifications.
    pub fn clear_all(&mut self) {
        self.notifications.clear();
        self.active_toast = None;
    }

    /// Clear notifications from a specific Silo.
    pub fn clear_silo(&mut self, silo_id: u64) {
        self.notifications.retain(|n| n.silo_id != silo_id);
    }

    /// Get unread count.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    /// Get grouped notifications (for display).
    pub fn grouped(&self) -> Vec<(&str, Vec<&Notification>)> {
        let mut groups: Vec<(&str, Vec<&Notification>)> = Vec::new();

        for notif in &self.notifications {
            let key = notif.group.as_deref().unwrap_or(&notif.app_name);
            if let Some(group) = groups.iter_mut().find(|(k, _)| *k == key) {
                group.1.push(notif);
            } else {
                groups.push((key, alloc::vec![notif]));
            }
        }

        groups
    }

    /// Toggle DND mode.
    pub fn toggle_dnd(&mut self) {
        self.dnd_enabled = !self.dnd_enabled;
    }
}
