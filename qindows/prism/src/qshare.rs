//! # Q-Share — P2P File Sharing with Capability Gates
//!
//! Share Q-Objects between Silos or across the mesh with
//! fine-grained access control (Section 3.8).
//!
//! Features:
//! - Share links with capability tokens (read-only, read-write, time-limited)
//! - Per-share bandwidth limits
//! - Access logging for compliance
//! - Revocable shares (Sentinel can revoke at any time)
//! - Encrypted transfer via Q-Fabric

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Share permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharePerm {
    ReadOnly,
    ReadWrite,
    Execute,
}

/// Share state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShareState {
    Active,
    Expired,
    Revoked,
    Exhausted,
}

/// A share link.
#[derive(Debug, Clone)]
pub struct ShareLink {
    pub id: u64,
    pub oid: u64,
    pub owner_silo: u64,
    pub permission: SharePerm,
    pub state: ShareState,
    pub created_at: u64,
    pub expires_at: u64,
    pub max_accesses: u64,
    pub access_count: u64,
    pub recipients: Vec<u64>,
    pub bandwidth_limit: u64,
    pub bytes_transferred: u64,
}

/// Share statistics.
#[derive(Debug, Clone, Default)]
pub struct ShareStats {
    pub shares_created: u64,
    pub shares_revoked: u64,
    pub shares_expired: u64,
    pub accesses: u64,
    pub bytes_shared: u64,
    pub access_denied: u64,
}

/// The Q-Share Manager.
pub struct QShare {
    pub shares: BTreeMap<u64, ShareLink>,
    next_id: u64,
    pub stats: ShareStats,
}

impl QShare {
    pub fn new() -> Self {
        QShare {
            shares: BTreeMap::new(),
            next_id: 1,
            stats: ShareStats::default(),
        }
    }

    /// Create a share link.
    pub fn create(&mut self, oid: u64, owner_silo: u64, perm: SharePerm, expires_at: u64, max_accesses: u64, bw_limit: u64, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.shares.insert(id, ShareLink {
            id, oid, owner_silo, permission: perm,
            state: ShareState::Active,
            created_at: now, expires_at, max_accesses,
            access_count: 0, recipients: Vec::new(),
            bandwidth_limit: bw_limit, bytes_transferred: 0,
        });

        self.stats.shares_created += 1;
        id
    }

    /// Access a shared object.
    pub fn access(&mut self, share_id: u64, accessor_silo: u64, bytes: u64, now: u64) -> Result<SharePerm, &'static str> {
        let share = self.shares.get_mut(&share_id).ok_or("Share not found")?;

        if share.state != ShareState::Active {
            self.stats.access_denied += 1;
            return Err("Share not active");
        }
        if now >= share.expires_at && share.expires_at > 0 {
            share.state = ShareState::Expired;
            self.stats.shares_expired += 1;
            self.stats.access_denied += 1;
            return Err("Share expired");
        }
        if share.access_count >= share.max_accesses && share.max_accesses > 0 {
            share.state = ShareState::Exhausted;
            self.stats.access_denied += 1;
            return Err("Access limit reached");
        }

        share.access_count += 1;
        share.bytes_transferred += bytes;
        if !share.recipients.contains(&accessor_silo) {
            share.recipients.push(accessor_silo);
        }

        self.stats.accesses += 1;
        self.stats.bytes_shared += bytes;
        Ok(share.permission)
    }

    /// Revoke a share.
    pub fn revoke(&mut self, share_id: u64) {
        if let Some(share) = self.shares.get_mut(&share_id) {
            share.state = ShareState::Revoked;
            self.stats.shares_revoked += 1;
        }
    }

    /// Expire stale shares.
    pub fn expire(&mut self, now: u64) {
        for share in self.shares.values_mut() {
            if share.state == ShareState::Active && share.expires_at > 0 && now >= share.expires_at {
                share.state = ShareState::Expired;
                self.stats.shares_expired += 1;
            }
        }
    }
}
