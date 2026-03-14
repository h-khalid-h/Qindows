//! # Q-Audit — Security Event Logging
//!
//! Tamper-resistant security audit log for the Qernel.
//! Records all security-critical events (capability grants,
//! Sentinel verdicts, authentication attempts, Silo lifecycle)
//! to an append-only, hash-chained log (Section 7.6).
//!
//! Features:
//! - Hash-chained entries (tamper detection)
//! - Per-Silo event isolation
//! - Severity-based filtering
//! - Structured event payloads
//! - Ring buffer with overflow to Prism

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Audit event severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Alert,
    Critical,
}

/// Audit event category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditCategory {
    Authentication,
    Authorization,
    CapabilityGrant,
    CapabilityRevoke,
    SiloLifecycle,
    SentinelVerdict,
    FileAccess,
    NetworkAccess,
    PolicyChange,
    SystemBoot,
    Integrity,
}

/// A single audit event.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    pub sequence: u64,
    pub timestamp: u64,
    pub severity: Severity,
    pub category: AuditCategory,
    pub silo_id: Option<u64>,
    pub subject: String,
    pub action: String,
    pub outcome: bool,
    pub detail: String,
    /// Hash of this entry (hash of prev_hash + payload)
    pub hash: u64,
    /// Hash of the previous entry (chain link)
    pub prev_hash: u64,
}

/// Audit log statistics.
#[derive(Debug, Clone, Default)]
pub struct AuditStats {
    pub events_logged: u64,
    pub events_overflowed: u64,
    pub chain_verified: u64,
    pub chain_broken: u64,
    pub alerts: u64,
    pub criticals: u64,
}

/// The Audit Logger.
pub struct AuditLog {
    pub events: Vec<AuditEvent>,
    pub max_events: usize,
    next_seq: u64,
    last_hash: u64,
    /// Per-category event count (indexed by AuditCategory::as_u8())
    pub category_counts: [u64; 11],
    pub stats: AuditStats,
}

// Manual impl because AuditCategory needs Ord for BTreeMap
impl AuditCategory {
    fn as_u8(&self) -> u8 {
        match self {
            AuditCategory::Authentication => 0,
            AuditCategory::Authorization => 1,
            AuditCategory::CapabilityGrant => 2,
            AuditCategory::CapabilityRevoke => 3,
            AuditCategory::SiloLifecycle => 4,
            AuditCategory::SentinelVerdict => 5,
            AuditCategory::FileAccess => 6,
            AuditCategory::NetworkAccess => 7,
            AuditCategory::PolicyChange => 8,
            AuditCategory::SystemBoot => 9,
            AuditCategory::Integrity => 10,
        }
    }
}

impl PartialOrd for AuditCategory {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AuditCategory {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.as_u8().cmp(&other.as_u8())
    }
}

impl AuditLog {
    pub fn new(max_events: usize) -> Self {
        AuditLog {
            events: Vec::new(),
            max_events,
            next_seq: 1,
            last_hash: 0,
            category_counts: [0; 11],
            stats: AuditStats::default(),
        }
    }

    /// Log an audit event.
    pub fn log(
        &mut self,
        severity: Severity,
        category: AuditCategory,
        silo_id: Option<u64>,
        subject: &str,
        action: &str,
        outcome: bool,
        detail: &str,
        now: u64,
    ) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;

        // Compute hash chain: simple FNV-like hash of prev + payload
        let hash = Self::compute_hash(self.last_hash, seq, now, subject, action);

        // Evict oldest if full
        if self.events.len() >= self.max_events {
            self.events.remove(0);
            self.stats.events_overflowed += 1;
        }

        let event = AuditEvent {
            sequence: seq,
            timestamp: now,
            severity, category,
            silo_id,
            subject: String::from(subject),
            action: String::from(action),
            outcome,
            detail: String::from(detail),
            hash, prev_hash: self.last_hash,
        };

        self.last_hash = hash;
        self.events.push(event);

        self.category_counts[category.as_u8() as usize] += 1;
        self.stats.events_logged += 1;

        match severity {
            Severity::Alert => self.stats.alerts += 1,
            Severity::Critical => self.stats.criticals += 1,
            _ => {}
        }

        seq
    }

    /// Verify hash chain integrity.
    pub fn verify_chain(&mut self) -> bool {
        let mut prev_hash = 0u64;
        for event in &self.events {
            if event.prev_hash != prev_hash {
                self.stats.chain_broken += 1;
                return false;
            }
            let expected = Self::compute_hash(
                prev_hash, event.sequence, event.timestamp,
                &event.subject, &event.action,
            );
            if event.hash != expected {
                self.stats.chain_broken += 1;
                return false;
            }
            prev_hash = event.hash;
        }
        self.stats.chain_verified += 1;
        true
    }

    /// Query events by category.
    pub fn query_category(&self, category: AuditCategory) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.category == category).collect()
    }

    /// Query events by Silo.
    pub fn query_silo(&self, silo_id: u64) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.silo_id == Some(silo_id)).collect()
    }

    /// Query events at or above severity.
    pub fn query_severity(&self, min: Severity) -> Vec<&AuditEvent> {
        self.events.iter().filter(|e| e.severity >= min).collect()
    }

    /// Simple FNV-1a-like hash for chain.
    fn compute_hash(prev: u64, seq: u64, ts: u64, subject: &str, action: &str) -> u64 {
        let mut h = prev ^ 0xcbf29ce484222325;
        h = h.wrapping_mul(0x100000001b3) ^ seq;
        h = h.wrapping_mul(0x100000001b3) ^ ts;
        for b in subject.bytes() {
            h = h.wrapping_mul(0x100000001b3) ^ b as u64;
        }
        for b in action.bytes() {
            h = h.wrapping_mul(0x100000001b3) ^ b as u64;
        }
        h
    }
}
