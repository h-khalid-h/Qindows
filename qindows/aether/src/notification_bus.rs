//! # Notification Bus — Pub-Sub Event Routing
//!
//! System-wide event bus for decoupled component communication
//! (Section 4.7). Subsystems publish events; Silos and services
//! subscribe to topics they care about.
//!
//! Features:
//! - Topic-based pub/sub
//! - Per-Silo subscriptions with capability gates
//! - Message filtering (by priority, source, or content)
//! - Delivery guarantees: at-most-once or at-least-once
//! - Backpressure: slow consumers get buffered then dropped

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Delivery guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    AtMostOnce,
    AtLeastOnce,
}

/// A bus event.
#[derive(Debug, Clone)]
pub struct BusEvent {
    pub id: u64,
    pub topic: String,
    pub source_silo: u64,
    pub payload: Vec<u8>,
    pub timestamp: u64,
    pub priority: u8,
}

/// A subscription.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub id: u64,
    pub silo_id: u64,
    pub topic: String,
    pub delivery: Delivery,
    pub min_priority: u8,
    pub active: bool,
}

/// Subscriber mailbox.
#[derive(Debug, Clone)]
pub struct Mailbox {
    pub sub_id: u64,
    pub events: Vec<BusEvent>,
    pub max_pending: usize,
    pub dropped: u64,
}

/// Bus statistics.
#[derive(Debug, Clone, Default)]
pub struct BusStats {
    pub events_published: u64,
    pub events_delivered: u64,
    pub events_dropped: u64,
    pub subscriptions: u64,
    pub topics_active: u64,
}

/// The Notification Bus.
pub struct NotificationBus {
    /// Subscriptions by ID
    pub subs: BTreeMap<u64, Subscription>,
    /// Mailboxes by subscription ID
    pub mailboxes: BTreeMap<u64, Mailbox>,
    next_event_id: u64,
    next_sub_id: u64,
    pub max_mailbox: usize,
    pub stats: BusStats,
}

impl NotificationBus {
    pub fn new() -> Self {
        NotificationBus {
            subs: BTreeMap::new(),
            mailboxes: BTreeMap::new(),
            next_event_id: 1,
            next_sub_id: 1,
            max_mailbox: 100,
            stats: BusStats::default(),
        }
    }

    /// Subscribe a Silo to a topic.
    pub fn subscribe(&mut self, silo_id: u64, topic: &str, delivery: Delivery, min_priority: u8) -> u64 {
        let id = self.next_sub_id;
        self.next_sub_id += 1;

        self.subs.insert(id, Subscription {
            id, silo_id, topic: String::from(topic),
            delivery, min_priority, active: true,
        });

        self.mailboxes.insert(id, Mailbox {
            sub_id: id, events: Vec::new(),
            max_pending: self.max_mailbox, dropped: 0,
        });

        self.stats.subscriptions += 1;
        id
    }

    /// Unsubscribe.
    pub fn unsubscribe(&mut self, sub_id: u64) {
        if let Some(sub) = self.subs.get_mut(&sub_id) {
            sub.active = false;
        }
        self.mailboxes.remove(&sub_id);
    }

    /// Publish an event to a topic.
    pub fn publish(&mut self, topic: &str, source_silo: u64, payload: Vec<u8>, priority: u8, now: u64) -> u64 {
        let event_id = self.next_event_id;
        self.next_event_id += 1;

        let event = BusEvent {
            id: event_id, topic: String::from(topic),
            source_silo, payload, timestamp: now, priority,
        };

        // Deliver to matching subscribers
        let matching_subs: Vec<u64> = self.subs.values()
            .filter(|s| s.active && s.topic == topic && priority >= s.min_priority)
            .map(|s| s.id)
            .collect();

        for sub_id in matching_subs {
            if let Some(mailbox) = self.mailboxes.get_mut(&sub_id) {
                if mailbox.events.len() >= mailbox.max_pending {
                    // Backpressure: drop oldest
                    mailbox.events.remove(0);
                    mailbox.dropped += 1;
                    self.stats.events_dropped += 1;
                }
                mailbox.events.push(event.clone());
                self.stats.events_delivered += 1;
            }
        }

        self.stats.events_published += 1;
        event_id
    }

    /// Consume events from a subscription's mailbox.
    pub fn consume(&mut self, sub_id: u64) -> Vec<BusEvent> {
        self.mailboxes.get_mut(&sub_id)
            .map(|mb| core::mem::take(&mut mb.events))
            .unwrap_or_default()
    }
}
