//! # Aether Notification Center
//!
//! System-wide notification management with priority levels,
//! grouping, actions, and persistence.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Notification urgency level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    /// Low — silent, badge only
    Low,
    /// Normal — visible toast
    Normal,
    /// High — persistent banner
    High,
    /// Critical — must acknowledge (alarms, security alerts)
    Critical,
}

/// Notification category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotifCategory {
    System,
    App,
    Message,
    Email,
    Calendar,
    Download,
    Update,
    Security,
    Social,
}

/// A notification action button.
#[derive(Debug, Clone)]
pub struct NotifAction {
    /// Action label shown on button
    pub label: String,
    /// Action identifier (dispatched to app)
    pub action_id: String,
    /// Is this the default action?
    pub is_default: bool,
}

/// A single notification.
#[derive(Debug, Clone)]
pub struct Notification {
    /// Unique notification ID
    pub id: u64,
    /// Source app Silo ID
    pub silo_id: u64,
    /// App name
    pub app_name: String,
    /// Title
    pub title: String,
    /// Body text
    pub body: String,
    /// Urgency
    pub urgency: Urgency,
    /// Category
    pub category: NotifCategory,
    /// Group key (for stacking related notifications)
    pub group: Option<String>,
    /// Actions (buttons)
    pub actions: Vec<NotifAction>,
    /// Timestamp (ns)
    pub timestamp: u64,
    /// Has been read?
    pub read: bool,
    /// Has been dismissed?
    pub dismissed: bool,
    /// Auto-dismiss after (ms), 0 = persistent
    pub timeout_ms: u32,
    /// Badge count (for app icon)
    pub badge: u32,
    /// Icon identifier
    pub icon: Option<String>,
    /// Progress (0-100, for download/update notifications)
    pub progress: Option<u8>,
}

/// Notification filter (Do Not Disturb rules).
#[derive(Debug, Clone)]
pub struct NotifFilter {
    /// Block all notifications?
    pub dnd_enabled: bool,
    /// Allow critical notifications through DND?
    pub allow_critical: bool,
    /// Blocked app Silo IDs
    pub blocked_silos: Vec<u64>,
    /// Blocked categories
    pub blocked_categories: Vec<NotifCategory>,
    /// Muted group keys
    pub muted_groups: Vec<String>,
}

impl Default for NotifFilter {
    fn default() -> Self {
        NotifFilter {
            dnd_enabled: false,
            allow_critical: true,
            blocked_silos: Vec::new(),
            blocked_categories: Vec::new(),
            muted_groups: Vec::new(),
        }
    }
}

/// The Notification Center.
pub struct NotificationCenter {
    /// All notifications (newest first)
    pub notifications: Vec<Notification>,
    /// Filter/DND rules
    pub filter: NotifFilter,
    /// Maximum stored notifications
    pub max_stored: usize,
    /// Next notification ID
    next_id: u64,
    /// Pending toasts (to be displayed)
    pub pending_toasts: Vec<u64>,
    /// Stats
    pub stats: NotifStats,
}

/// Notification statistics.
#[derive(Debug, Clone, Default)]
pub struct NotifStats {
    pub total_received: u64,
    pub total_displayed: u64,
    pub total_dismissed: u64,
    pub total_actioned: u64,
    pub total_filtered: u64,
}

impl NotificationCenter {
    pub fn new() -> Self {
        NotificationCenter {
            notifications: Vec::new(),
            filter: NotifFilter::default(),
            max_stored: 200,
            next_id: 1,
            pending_toasts: Vec::new(),
            stats: NotifStats::default(),
        }
    }

    /// Post a notification.
    pub fn post(&mut self, mut notif: Notification) -> Option<u64> {
        notif.id = self.next_id;
        self.next_id += 1;
        self.stats.total_received += 1;

        // Apply filters
        if self.should_filter(&notif) {
            self.stats.total_filtered += 1;
            return None;
        }

        let id = notif.id;

        // Add to pending toasts if visible urgency
        if notif.urgency >= Urgency::Normal {
            self.pending_toasts.push(id);
            self.stats.total_displayed += 1;
        }

        // Insert at front (newest first)
        self.notifications.insert(0, notif);

        // Trim old notifications
        while self.notifications.len() > self.max_stored {
            self.notifications.pop();
        }

        Some(id)
    }

    /// Dismiss a notification.
    pub fn dismiss(&mut self, notif_id: u64) {
        if let Some(notif) = self.notifications.iter_mut().find(|n| n.id == notif_id) {
            notif.dismissed = true;
            self.stats.total_dismissed += 1;
        }
        self.pending_toasts.retain(|&id| id != notif_id);
    }

    /// Mark as read.
    pub fn mark_read(&mut self, notif_id: u64) {
        if let Some(notif) = self.notifications.iter_mut().find(|n| n.id == notif_id) {
            notif.read = true;
        }
    }

    /// Dismiss all from a Silo.
    pub fn dismiss_all_from(&mut self, silo_id: u64) {
        for notif in &mut self.notifications {
            if notif.silo_id == silo_id {
                notif.dismissed = true;
                self.stats.total_dismissed += 1;
            }
        }
    }

    /// Trigger an action on a notification.
    pub fn trigger_action(&mut self, notif_id: u64, action_id: &str) -> Option<(u64, String)> {
        if let Some(notif) = self.notifications.iter().find(|n| n.id == notif_id) {
            if notif.actions.iter().any(|a| a.action_id == action_id) {
                self.stats.total_actioned += 1;
                return Some((notif.silo_id, String::from(action_id)));
            }
        }
        None
    }

    /// Get unread count.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read && !n.dismissed).count()
    }

    /// Get unread count for a specific app.
    pub fn unread_for_silo(&self, silo_id: u64) -> usize {
        self.notifications.iter()
            .filter(|n| n.silo_id == silo_id && !n.read && !n.dismissed)
            .count()
    }

    /// Get grouped notifications.
    pub fn grouped(&self) -> Vec<(String, Vec<&Notification>)> {
        let mut groups: alloc::collections::BTreeMap<String, Vec<&Notification>> = alloc::collections::BTreeMap::new();
        for notif in &self.notifications {
            if notif.dismissed { continue; }
            let key = notif.group.clone().unwrap_or_else(|| notif.app_name.clone());
            groups.entry(key).or_insert_with(Vec::new).push(notif);
        }
        groups.into_iter().collect()
    }

    /// Update progress on a notification.
    pub fn update_progress(&mut self, notif_id: u64, progress: u8) {
        if let Some(notif) = self.notifications.iter_mut().find(|n| n.id == notif_id) {
            notif.progress = Some(progress.min(100));
        }
    }

    /// Pop pending toasts for display.
    pub fn pop_toasts(&mut self) -> Vec<u64> {
        let toasts = self.pending_toasts.clone();
        self.pending_toasts.clear();
        toasts
    }

    /// Should this notification be filtered?
    fn should_filter(&self, notif: &Notification) -> bool {
        if self.filter.dnd_enabled {
            if notif.urgency == Urgency::Critical && self.filter.allow_critical {
                return false; // Critical bypasses DND
            }
            return true;
        }

        if self.filter.blocked_silos.contains(&notif.silo_id) { return true; }
        if self.filter.blocked_categories.contains(&notif.category) { return true; }
        if let Some(ref group) = notif.group {
            if self.filter.muted_groups.contains(group) { return true; }
        }

        false
    }

    /// Clear all dismissed notifications.
    pub fn clear_dismissed(&mut self) {
        self.notifications.retain(|n| !n.dismissed);
    }
}
