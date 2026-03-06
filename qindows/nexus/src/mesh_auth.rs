//! # Mesh Auth — mTLS Peer Authentication
//!
//! Authenticates mesh peers using mutual TLS with
//! certificate validation and per-node identity (Section 11.17).
//!
//! Features:
//! - X.509 certificate management
//! - Certificate pinning
//! - Peer identity verification
//! - Certificate revocation
//! - Auth event logging

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Peer auth state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    Unknown,
    Authenticating,
    Authenticated,
    Rejected,
    Revoked,
}

/// A peer certificate.
#[derive(Debug, Clone)]
pub struct PeerCert {
    pub node_id: [u8; 32],
    pub fingerprint: u64,
    pub common_name: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub state: AuthState,
    pub auth_count: u64,
}

/// Auth event.
#[derive(Debug, Clone)]
pub struct AuthEvent {
    pub node_id: [u8; 32],
    pub timestamp: u64,
    pub success: bool,
    pub reason: String,
}

/// Auth statistics.
#[derive(Debug, Clone, Default)]
pub struct AuthStats {
    pub peers_authenticated: u64,
    pub peers_rejected: u64,
    pub certs_revoked: u64,
    pub auth_attempts: u64,
}

/// The Mesh Auth Manager.
pub struct MeshAuth {
    pub peers: BTreeMap<[u8; 32], PeerCert>,
    pub revoked: Vec<u64>,
    pub events: Vec<AuthEvent>,
    pub max_events: usize,
    pub stats: AuthStats,
}

impl MeshAuth {
    pub fn new() -> Self {
        MeshAuth {
            peers: BTreeMap::new(),
            revoked: Vec::new(),
            events: Vec::new(),
            max_events: 1000,
            stats: AuthStats::default(),
        }
    }

    /// Register a peer certificate.
    pub fn register_peer(&mut self, node_id: [u8; 32], cn: &str, fingerprint: u64, issued: u64, expires: u64) {
        self.peers.insert(node_id, PeerCert {
            node_id, fingerprint, common_name: String::from(cn),
            issued_at: issued, expires_at: expires,
            state: AuthState::Unknown, auth_count: 0,
        });
    }

    /// Authenticate a peer.
    pub fn authenticate(&mut self, node_id: &[u8; 32], fingerprint: u64, now: u64) -> bool {
        self.stats.auth_attempts += 1;

        let peer = match self.peers.get_mut(node_id) {
            Some(p) => p,
            None => {
                self.log_event(*node_id, now, false, "Unknown peer");
                self.stats.peers_rejected += 1;
                return false;
            }
        };

        // Check revocation
        if self.revoked.contains(&peer.fingerprint) {
            peer.state = AuthState::Revoked;
            self.log_event(*node_id, now, false, "Certificate revoked");
            self.stats.peers_rejected += 1;
            return false;
        }

        // Check expiry
        if now > peer.expires_at {
            peer.state = AuthState::Rejected;
            self.log_event(*node_id, now, false, "Certificate expired");
            self.stats.peers_rejected += 1;
            return false;
        }

        // Check fingerprint match
        if peer.fingerprint != fingerprint {
            peer.state = AuthState::Rejected;
            self.log_event(*node_id, now, false, "Fingerprint mismatch");
            self.stats.peers_rejected += 1;
            return false;
        }

        peer.state = AuthState::Authenticated;
        peer.auth_count += 1;
        self.stats.peers_authenticated += 1;
        self.log_event(*node_id, now, true, "Authenticated");
        true
    }

    /// Revoke a certificate by fingerprint.
    pub fn revoke(&mut self, fingerprint: u64) {
        if !self.revoked.contains(&fingerprint) {
            self.revoked.push(fingerprint);
            self.stats.certs_revoked += 1;
        }
    }

    fn log_event(&mut self, node_id: [u8; 32], timestamp: u64, success: bool, reason: &str) {
        self.events.push(AuthEvent {
            node_id, timestamp, success, reason: String::from(reason),
        });
        if self.events.len() > self.max_events {
            self.events.remove(0);
        }
    }
}
