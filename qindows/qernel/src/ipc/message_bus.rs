//! # Qernel IPC Message Bus
//!
//! High-level message passing between Silos.
//! Builds on top of Q-Ring for raw byte transport, adding structured
//! message types, request-reply patterns, broadcast, and pub-sub.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// One-way fire-and-forget
    Send,
    /// Request expecting a reply
    Request,
    /// Reply to a request
    Reply,
    /// Broadcast to all subscribers
    Broadcast,
    /// Subscribe to a topic
    Subscribe,
    /// Unsubscribe from a topic
    Unsubscribe,
    /// Error response
    Error,
}

/// A structured IPC message.
#[derive(Debug, Clone)]
pub struct Message {
    /// Unique message ID
    pub id: u64,
    /// Message type
    pub msg_type: MessageType,
    /// Sender Silo ID
    pub from: u64,
    /// Recipient Silo ID (0 = broadcast)
    pub to: u64,
    /// Topic (for pub-sub)
    pub topic: Option<String>,
    /// Reply-to message ID (for Request/Reply)
    pub reply_to: Option<u64>,
    /// Payload
    pub payload: MessagePayload,
    /// Timestamp (ns)
    pub timestamp: u64,
    /// TTL (max hops for mesh messages)
    pub ttl: u8,
}

/// Message payload variants.
#[derive(Debug, Clone)]
pub enum MessagePayload {
    /// Raw bytes
    Bytes(Vec<u8>),
    /// Text string
    Text(String),
    /// Integer
    Int(i64),
    /// Key-value map
    Map(BTreeMap<String, String>),
    /// Empty (for control messages)
    Empty,
}

/// A topic subscription.
#[derive(Debug, Clone)]
pub struct Subscription {
    pub silo_id: u64,
    pub topic: String,
    pub created_at: u64,
}

/// Pending request (waiting for reply).
#[derive(Debug, Clone)]
pub struct PendingRequest {
    pub message_id: u64,
    pub from: u64,
    pub timeout_ns: u64,
    pub sent_at: u64,
}

/// The Message Bus.
pub struct MessageBus {
    /// Mailboxes: silo_id → Vec<Message>
    pub mailboxes: BTreeMap<u64, Vec<Message>>,
    /// Topic subscriptions: topic → Vec<silo_id>
    pub subscriptions: BTreeMap<String, Vec<u64>>,
    /// Pending requests (waiting for reply)
    pub pending: Vec<PendingRequest>,
    /// Next message ID
    next_id: u64,
    /// Stats
    pub stats: BusStats,
}

/// Bus statistics.
#[derive(Debug, Clone, Default)]
pub struct BusStats {
    pub messages_sent: u64,
    pub messages_delivered: u64,
    pub broadcasts: u64,
    pub requests: u64,
    pub replies: u64,
    pub timeouts: u64,
    pub dropped: u64,
}

impl MessageBus {
    pub fn new() -> Self {
        MessageBus {
            mailboxes: BTreeMap::new(),
            subscriptions: BTreeMap::new(),
            pending: Vec::new(),
            next_id: 1,
            stats: BusStats::default(),
        }
    }

    /// Send a one-way message.
    pub fn send(&mut self, from: u64, to: u64, payload: MessagePayload) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.stats.messages_sent += 1;

        let msg = Message {
            id,
            msg_type: MessageType::Send,
            from, to,
            topic: None,
            reply_to: None,
            payload,
            timestamp: 0,
            ttl: 16,
        };

        self.deliver(to, msg);
        id
    }

    /// Send a request and expect a reply.
    pub fn request(&mut self, from: u64, to: u64, payload: MessagePayload, timeout_ns: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.stats.requests += 1;

        let msg = Message {
            id,
            msg_type: MessageType::Request,
            from, to,
            topic: None,
            reply_to: None,
            payload,
            timestamp: 0,
            ttl: 16,
        };

        self.pending.push(PendingRequest {
            message_id: id, from, timeout_ns, sent_at: 0,
        });

        self.deliver(to, msg);
        id
    }

    /// Send a reply to a request.
    pub fn reply(&mut self, from: u64, to: u64, reply_to: u64, payload: MessagePayload) {
        self.stats.replies += 1;

        let msg = Message {
            id: self.next_id,
            msg_type: MessageType::Reply,
            from, to,
            topic: None,
            reply_to: Some(reply_to),
            payload,
            timestamp: 0,
            ttl: 16,
        };
        self.next_id += 1;

        // Remove from pending
        self.pending.retain(|p| p.message_id != reply_to);

        self.deliver(to, msg);
    }

    /// Subscribe to a topic.
    pub fn subscribe(&mut self, silo_id: u64, topic: &str) {
        let subs = self.subscriptions.entry(String::from(topic)).or_insert_with(Vec::new);
        if !subs.contains(&silo_id) {
            subs.push(silo_id);
        }
    }

    /// Unsubscribe from a topic.
    pub fn unsubscribe(&mut self, silo_id: u64, topic: &str) {
        if let Some(subs) = self.subscriptions.get_mut(topic) {
            subs.retain(|&s| s != silo_id);
        }
    }

    /// Broadcast a message to all subscribers of a topic.
    pub fn broadcast(&mut self, from: u64, topic: &str, payload: MessagePayload) {
        self.stats.broadcasts += 1;

        let subscribers = match self.subscriptions.get(topic) {
            Some(subs) => subs.clone(),
            None => return,
        };

        for &sub_id in &subscribers {
            if sub_id == from { continue; } // Don't send to self

            let msg = Message {
                id: self.next_id,
                msg_type: MessageType::Broadcast,
                from,
                to: sub_id,
                topic: Some(String::from(topic)),
                reply_to: None,
                payload: payload.clone(),
                timestamp: 0,
                ttl: 16,
            };
            self.next_id += 1;
            self.deliver(sub_id, msg);
        }
    }

    /// Receive messages for a silo (drain mailbox).
    pub fn receive(&mut self, silo_id: u64) -> Vec<Message> {
        self.mailboxes.remove(&silo_id).unwrap_or_default()
    }

    /// Peek at the mailbox without draining.
    pub fn peek(&self, silo_id: u64) -> usize {
        self.mailboxes.get(&silo_id).map(|m| m.len()).unwrap_or(0)
    }

    /// Deliver a message to a mailbox.
    fn deliver(&mut self, to: u64, msg: Message) {
        let mailbox = self.mailboxes.entry(to).or_insert_with(Vec::new);

        // Cap mailbox size
        if mailbox.len() >= 1024 {
            self.stats.dropped += 1;
            return;
        }

        mailbox.push(msg);
        self.stats.messages_delivered += 1;
    }

    /// Expire timed-out requests.
    pub fn expire_requests(&mut self, now_ns: u64) {
        let timed_out: Vec<PendingRequest> = self.pending.iter()
            .filter(|p| now_ns - p.sent_at > p.timeout_ns)
            .cloned()
            .collect();

        for req in &timed_out {
            self.stats.timeouts += 1;
            // Deliver timeout error to requester
            let error = Message {
                id: self.next_id,
                msg_type: MessageType::Error,
                from: 0,
                to: req.from,
                topic: None,
                reply_to: Some(req.message_id),
                payload: MessagePayload::Text(String::from("Request timed out")),
                timestamp: now_ns,
                ttl: 0,
            };
            self.next_id += 1;
            self.deliver(req.from, error);
        }

        self.pending.retain(|p| now_ns - p.sent_at <= p.timeout_ns);
    }

    /// Remove all subscriptions and messages for a Silo (on exit).
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        self.mailboxes.remove(&silo_id);
        for subs in self.subscriptions.values_mut() {
            subs.retain(|&s| s != silo_id);
        }
        self.pending.retain(|p| p.from != silo_id);
    }
}
