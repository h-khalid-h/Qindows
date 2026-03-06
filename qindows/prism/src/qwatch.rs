//! # Q-Watch — Filesystem Change Notifications
//!
//! Monitors Q-Object tree changes and delivers notifications
//! to registered watchers (Section 3.23).
//!
//! Features:
//! - Per-directory recursive watch
//! - Event types: create, modify, delete, rename, attr_change
//! - Per-Silo watch isolation
//! - Debouncing (coalesce rapid changes)
//! - Watch handle management

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// File change event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchEvent {
    Created,
    Modified,
    Deleted,
    Renamed,
    AttrChanged,
}

/// A filesystem change notification.
#[derive(Debug, Clone)]
pub struct ChangeNotification {
    pub watch_id: u64,
    pub event: WatchEvent,
    pub path: String,
    pub old_path: Option<String>,
    pub timestamp: u64,
    pub silo_id: u64,
}

/// A watch registration.
#[derive(Debug, Clone)]
pub struct Watch {
    pub id: u64,
    pub silo_id: u64,
    pub path: String,
    pub recursive: bool,
    pub events: Vec<WatchEvent>,
    pub debounce_ms: u64,
    pub last_event_time: u64,
    pub notifications_sent: u64,
}

/// Watch statistics.
#[derive(Debug, Clone, Default)]
pub struct WatchStats {
    pub watches_created: u64,
    pub watches_removed: u64,
    pub notifications_sent: u64,
    pub debounced: u64,
}

/// The Q-Watch Manager.
pub struct QWatch {
    pub watches: BTreeMap<u64, Watch>,
    pub pending: Vec<ChangeNotification>,
    next_id: u64,
    pub stats: WatchStats,
}

impl QWatch {
    pub fn new() -> Self {
        QWatch {
            watches: BTreeMap::new(),
            pending: Vec::new(),
            next_id: 1,
            stats: WatchStats::default(),
        }
    }

    /// Register a watch on a path.
    pub fn watch(&mut self, silo_id: u64, path: &str, recursive: bool, events: Vec<WatchEvent>, debounce_ms: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.watches.insert(id, Watch {
            id, silo_id, path: String::from(path), recursive,
            events, debounce_ms, last_event_time: 0, notifications_sent: 0,
        });

        self.stats.watches_created += 1;
        id
    }

    /// Report a filesystem change. Generates notifications for matching watches.
    pub fn report_change(&mut self, path: &str, event: WatchEvent, silo_id: u64, now: u64) {
        let matching_ids: Vec<u64> = self.watches.values()
            .filter(|w| w.silo_id == silo_id && w.events.contains(&event))
            .filter(|w| {
                if w.recursive {
                    path.starts_with(&w.path)
                } else {
                    // Non-recursive: path must be directly in watched directory
                    if let Some(parent) = path.rsplit_once('/').map(|(p, _)| p) {
                        parent == w.path || (parent.is_empty() && w.path == "/")
                    } else {
                        false
                    }
                }
            })
            .map(|w| w.id)
            .collect();

        for watch_id in matching_ids {
            if let Some(watch) = self.watches.get_mut(&watch_id) {
                // Debounce check
                if now.saturating_sub(watch.last_event_time) < watch.debounce_ms {
                    self.stats.debounced += 1;
                    continue;
                }

                watch.last_event_time = now;
                watch.notifications_sent += 1;
                self.stats.notifications_sent += 1;

                self.pending.push(ChangeNotification {
                    watch_id, event, path: String::from(path),
                    old_path: None, timestamp: now, silo_id,
                });
            }
        }
    }

    /// Drain pending notifications.
    pub fn drain(&mut self) -> Vec<ChangeNotification> {
        core::mem::take(&mut self.pending)
    }

    /// Remove a watch.
    pub fn unwatch(&mut self, watch_id: u64) {
        if self.watches.remove(&watch_id).is_some() {
            self.stats.watches_removed += 1;
        }
    }
}
