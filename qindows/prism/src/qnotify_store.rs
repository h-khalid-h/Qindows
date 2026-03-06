//! # Q-Notify Store — Notification Persistence + Search
//!
//! Stores notification history for per-Silo retrieval,
//! search, and replay (Section 4.13).
//!
//! Features:
//! - Persistent notification log
//! - Per-Silo isolation
//! - Read/unread tracking
//! - Priority filtering
//! - Search by title/body text
//! - Retention policy (max age, max count)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Notification priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotifyPriority {
    Low,
    Normal,
    High,
    Urgent,
}

/// A stored notification.
#[derive(Debug, Clone)]
pub struct StoredNotification {
    pub id: u64,
    pub silo_id: u64,
    pub title: String,
    pub body: String,
    pub priority: NotifyPriority,
    pub timestamp: u64,
    pub read: bool,
    pub source_app: String,
}

/// Notification store statistics.
#[derive(Debug, Clone, Default)]
pub struct NotifyStoreStats {
    pub total_stored: u64,
    pub total_read: u64,
    pub total_expired: u64,
    pub searches: u64,
}

/// The Notification Store.
pub struct QNotifyStore {
    pub notifications: BTreeMap<u64, StoredNotification>,
    next_id: u64,
    pub max_per_silo: usize,
    pub max_age_ms: u64,
    pub stats: NotifyStoreStats,
}

impl QNotifyStore {
    pub fn new() -> Self {
        QNotifyStore {
            notifications: BTreeMap::new(),
            next_id: 1,
            max_per_silo: 1000,
            max_age_ms: 7 * 24 * 3600 * 1000, // 7 days
            stats: NotifyStoreStats::default(),
        }
    }

    /// Store a notification.
    pub fn store(&mut self, silo_id: u64, title: &str, body: &str, priority: NotifyPriority, source: &str, now: u64) -> u64 {
        // Enforce per-Silo limit
        let silo_count = self.notifications.values()
            .filter(|n| n.silo_id == silo_id)
            .count();
        if silo_count >= self.max_per_silo {
            // Remove oldest for this Silo
            let oldest_id = self.notifications.values()
                .filter(|n| n.silo_id == silo_id)
                .min_by_key(|n| n.timestamp)
                .map(|n| n.id);
            if let Some(id) = oldest_id {
                self.notifications.remove(&id);
                self.stats.total_expired += 1;
            }
        }

        let id = self.next_id;
        self.next_id += 1;

        self.notifications.insert(id, StoredNotification {
            id, silo_id, title: String::from(title), body: String::from(body),
            priority, timestamp: now, read: false, source_app: String::from(source),
        });

        self.stats.total_stored += 1;
        id
    }

    /// Mark a notification as read.
    pub fn mark_read(&mut self, id: u64) {
        if let Some(n) = self.notifications.get_mut(&id) {
            if !n.read {
                n.read = true;
                self.stats.total_read += 1;
            }
        }
    }

    /// Get unread notifications for a Silo.
    pub fn unread(&self, silo_id: u64) -> Vec<&StoredNotification> {
        self.notifications.values()
            .filter(|n| n.silo_id == silo_id && !n.read)
            .collect()
    }

    /// Search notifications by text.
    pub fn search(&mut self, silo_id: u64, query: &str) -> Vec<&StoredNotification> {
        self.stats.searches += 1;
        let q = query.to_lowercase();
        self.notifications.values()
            .filter(|n| n.silo_id == silo_id)
            .filter(|n| {
                n.title.to_lowercase().contains(&q) ||
                n.body.to_lowercase().contains(&q)
            })
            .collect()
    }

    /// Expire old notifications.
    pub fn expire(&mut self, now: u64) {
        let expired: Vec<u64> = self.notifications.values()
            .filter(|n| now.saturating_sub(n.timestamp) > self.max_age_ms)
            .map(|n| n.id)
            .collect();
        for id in expired {
            self.notifications.remove(&id);
            self.stats.total_expired += 1;
        }
    }
}
