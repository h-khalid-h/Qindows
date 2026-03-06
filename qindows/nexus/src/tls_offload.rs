//! # TLS Offload — Hardware-Accelerated TLS
//!
//! Offloads TLS encryption/decryption to hardware crypto
//! engines or batches operations for throughput (Section 11.12).
//!
//! Features:
//! - TLS 1.3 session management
//! - Cipher suite negotiation
//! - Session ticket caching
//! - Per-Silo session isolation
//! - Handshake and record statistics

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// TLS protocol version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    Tls12,
    Tls13,
}

/// Cipher suite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CipherSuite {
    Aes128GcmSha256,
    Aes256GcmSha384,
    Chacha20Poly1305Sha256,
}

/// TLS session state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Handshaking,
    Established,
    Resuming,
    Closed,
}

/// A TLS session.
#[derive(Debug, Clone)]
pub struct TlsSession {
    pub id: u64,
    pub silo_id: u64,
    pub version: TlsVersion,
    pub cipher: CipherSuite,
    pub state: SessionState,
    pub peer_name: String,
    pub session_ticket: Vec<u8>,
    pub bytes_encrypted: u64,
    pub bytes_decrypted: u64,
    pub handshake_ms: u32,
}

/// TLS offload statistics.
#[derive(Debug, Clone, Default)]
pub struct TlsStats {
    pub sessions_created: u64,
    pub handshakes_completed: u64,
    pub session_resumptions: u64,
    pub bytes_encrypted: u64,
    pub bytes_decrypted: u64,
    pub handshake_failures: u64,
}

/// The TLS Offload Engine.
pub struct TlsOffload {
    pub sessions: BTreeMap<u64, TlsSession>,
    /// Session ticket cache: ticket_hash → session_id
    pub ticket_cache: BTreeMap<u64, u64>,
    next_id: u64,
    pub preferred_cipher: CipherSuite,
    pub stats: TlsStats,
}

impl TlsOffload {
    pub fn new() -> Self {
        TlsOffload {
            sessions: BTreeMap::new(),
            ticket_cache: BTreeMap::new(),
            next_id: 1,
            preferred_cipher: CipherSuite::Aes256GcmSha384,
            stats: TlsStats::default(),
        }
    }

    /// Begin a TLS handshake.
    pub fn begin_handshake(&mut self, silo_id: u64, peer: &str, version: TlsVersion) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.sessions.insert(id, TlsSession {
            id, silo_id, version, cipher: self.preferred_cipher,
            state: SessionState::Handshaking, peer_name: String::from(peer),
            session_ticket: Vec::new(), bytes_encrypted: 0,
            bytes_decrypted: 0, handshake_ms: 0,
        });

        self.stats.sessions_created += 1;
        id
    }

    /// Complete a handshake.
    pub fn complete_handshake(&mut self, session_id: u64, cipher: CipherSuite, ticket: Vec<u8>, latency_ms: u32) -> bool {
        if let Some(sess) = self.sessions.get_mut(&session_id) {
            if sess.state != SessionState::Handshaking { return false; }
            sess.state = SessionState::Established;
            sess.cipher = cipher;
            sess.handshake_ms = latency_ms;

            // Cache session ticket
            if !ticket.is_empty() {
                let hash = ticket.iter().fold(0u64, |h, &b| h.wrapping_mul(31).wrapping_add(b as u64));
                self.ticket_cache.insert(hash, session_id);
                sess.session_ticket = ticket;
            }

            self.stats.handshakes_completed += 1;
            true
        } else { false }
    }

    /// Try to resume a session via ticket.
    pub fn try_resume(&mut self, ticket_hash: u64, silo_id: u64) -> Option<u64> {
        let orig_id = self.ticket_cache.get(&ticket_hash).copied()?;
        let orig = self.sessions.get(&orig_id)?;
        if orig.silo_id != silo_id { return None; }

        let id = self.next_id;
        self.next_id += 1;

        self.sessions.insert(id, TlsSession {
            id, silo_id, version: orig.version, cipher: orig.cipher,
            state: SessionState::Established, peer_name: orig.peer_name.clone(),
            session_ticket: orig.session_ticket.clone(),
            bytes_encrypted: 0, bytes_decrypted: 0, handshake_ms: 0,
        });

        self.stats.session_resumptions += 1;
        self.stats.sessions_created += 1;
        Some(id)
    }

    /// Record encryption of data.
    pub fn encrypt(&mut self, session_id: u64, len: u64) {
        if let Some(s) = self.sessions.get_mut(&session_id) {
            s.bytes_encrypted += len;
            self.stats.bytes_encrypted += len;
        }
    }

    /// Record decryption of data.
    pub fn decrypt(&mut self, session_id: u64, len: u64) {
        if let Some(s) = self.sessions.get_mut(&session_id) {
            s.bytes_decrypted += len;
            self.stats.bytes_decrypted += len;
        }
    }

    /// Close a TLS session.
    pub fn close(&mut self, session_id: u64) {
        if let Some(s) = self.sessions.get_mut(&session_id) {
            s.state = SessionState::Closed;
        }
    }

    /// Clean up all sessions for a Silo.
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        let to_close: Vec<u64> = self.sessions.values()
            .filter(|s| s.silo_id == silo_id)
            .map(|s| s.id)
            .collect();
        for id in to_close {
            self.close(id);
        }
    }
}
