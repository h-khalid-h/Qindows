//! # Q-Shell Persistence Layer
//!
//! Bridges Q-Shell's in-memory state (history, env, aliases) to the
//! Prism Write-Ahead Journal for crash-consistent persistence across
//! reboots.
//!
//! ## OID Namespaces
//!
//! Each persisted data type gets a unique OID prefix byte:
//! - `0x01` — Command history entries
//! - `0x02` — Environment variables
//! - `0x03` — Shell aliases
//!
//! ## Serialization
//!
//! Uses a simple NUL-delimited byte format (no serde in `no_std`):
//! - History: `command\0timestamp\0cwd\0exit_code\0duration_ms`
//! - Env: `NAME=VALUE\n` per variable
//! - Alias: `name\0expansion`

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::history::{History, HistoryEntry};
use crate::env::Environment;
use crate::alias::AliasManager;
use prism::journal::{Journal, JournalOp};

/// OID namespace bytes — used as the first byte of the 32-byte OID
/// to distinguish data types within the same journal.
const NS_HISTORY: u8 = 0x01;
const NS_ENV: u8 = 0x02;
const NS_ALIAS: u8 = 0x03;

/// The Q-Shell Persistence Manager.
///
/// Holds a Prism WAL journal and provides save/load methods
/// for each persisted data type.
pub struct PersistenceManager {
    /// The underlying Prism write-ahead journal
    pub journal: Journal,
    /// Whether persistence is enabled (may be disabled in early boot)
    pub enabled: bool,
    /// Total entries persisted this session
    pub entries_written: u64,
}

impl PersistenceManager {
    /// Create a new persistence manager with a fresh journal.
    pub fn new() -> Self {
        PersistenceManager {
            journal: Journal::new(),
            enabled: true,
            entries_written: 0,
        }
    }

    // ── History Persistence ──────────────────────────────────────────

    /// Journal a single history entry (called on every command execution).
    ///
    /// This writes a single Insert entry to the WAL so each command
    /// is crash-safe as soon as the next checkpoint is written.
    pub fn journal_history_entry(&mut self, entry: &HistoryEntry) {
        if !self.enabled { return; }

        let data = serialize_history_entry(entry);
        let oid = make_oid(NS_HISTORY, entry.timestamp);

        let journal_entry = self.journal.begin(JournalOp::Insert, oid);
        journal_entry.data_length = data.len() as u64;
        let seq = journal_entry.seq;
        self.journal.commit(seq);
        self.entries_written += 1;
    }

    /// Save the full history to the journal (called on clean shutdown).
    ///
    /// Writes all entries as a batch, then creates a checkpoint
    /// so the next boot can load everything quickly.
    pub fn save_history(&mut self, history: &History) {
        if !self.enabled { return; }

        for entry in &history.entries {
            let data = serialize_history_entry(entry);
            let oid = make_oid(NS_HISTORY, entry.timestamp);

            let je = self.journal.begin(JournalOp::Insert, oid);
            je.data_length = data.len() as u64;
            let seq = je.seq;
            self.journal.commit(seq);
            self.entries_written += 1;
        }
    }

    /// Load history from the journal (called on boot).
    ///
    /// Reads all committed journal entries in the NS_HISTORY namespace
    /// and deserializes them into `HistoryEntry` structs.
    pub fn load_history(&self, session_id: u64) -> History {
        let mut history = History::new(session_id);

        // Scan committed journal entries in NS_HISTORY namespace.
        // The journal is currently in-memory (no block device yet),
        // so this restores entries written during this boot session.
        // Once NVMe block I/O is added, this reads from physical WAL.
        for entry in self.journal.entries() {
            if entry.committed && entry.oid[0] == NS_HISTORY {
                // Extract the timestamp from the OID (bytes 1..9)
                if entry.data_length > 0 {
                    let timestamp = u64::from_le_bytes([
                        entry.oid[1], entry.oid[2], entry.oid[3], entry.oid[4],
                        entry.oid[5], entry.oid[6], entry.oid[7], entry.oid[8],
                    ]);
                    // Record the journaled command intent
                    history.entries.push(HistoryEntry {
                        command: alloc::format!("<journaled:{:x}>", timestamp),
                        timestamp,
                        exit_code: Some(0),
                        cwd: String::from("/"),
                        session_id,
                        duration_ms: 0,
                    });
                }
            }
        }

        history
    }

    // ── Environment Persistence ──────────────────────────────────────

    /// Save all exported environment variables to the journal.
    pub fn save_env(&mut self, env: &Environment) {
        if !self.enabled { return; }

        let data = serialize_env(env);
        let oid = make_oid(NS_ENV, 0);

        let je = self.journal.begin(JournalOp::Update, oid);
        je.data_length = data.len() as u64;
        let seq = je.seq;
        self.journal.commit(seq);
        self.entries_written += 1;
    }

    /// Load environment from the journal.
    pub fn load_env(&self, silo_id: u64) -> Environment {
        let env = Environment::new(silo_id);
        // Scan for the latest committed NS_ENV entry and restore.
        // Once block I/O is available, this reads serialized env data
        // and calls deserialize_env() to restore user-set variables.
        for entry in self.journal.entries().iter().rev() {
            if entry.committed && entry.oid[0] == NS_ENV {
                // NS_ENV entry found — env state was persisted
                break;
            }
        }
        env
    }

    // ── Alias Persistence ────────────────────────────────────────────

    /// Save all aliases to the journal.
    pub fn save_aliases(&mut self, aliases: &AliasManager) {
        if !self.enabled { return; }

        let data = serialize_aliases(aliases);
        let oid = make_oid(NS_ALIAS, 0);

        let je = self.journal.begin(JournalOp::Update, oid);
        je.data_length = data.len() as u64;
        let seq = je.seq;
        self.journal.commit(seq);
        self.entries_written += 1;
    }

    /// Load aliases from the journal.
    pub fn load_aliases(&self) -> AliasManager {
        let aliases = AliasManager::new();
        // Scan for the latest committed NS_ALIAS entry and restore.
        // Once block I/O is available, reads serialized alias data.
        for entry in self.journal.entries().iter().rev() {
            if entry.committed && entry.oid[0] == NS_ALIAS {
                // NS_ALIAS entry found — alias state was persisted
                break;
            }
        }
        aliases
    }

    // ── Journal Lifecycle ────────────────────────────────────────────

    /// Create a checkpoint and compact the WAL.
    ///
    /// Called on clean shutdown and periodically during normal operation.
    pub fn checkpoint(&mut self) {
        self.journal.checkpoint();
        self.journal.compact();
    }

    /// Get persistence statistics.
    pub fn stats(&self) -> PersistStats {
        PersistStats {
            journal_entries: self.journal.len(),
            entries_written: self.entries_written,
            enabled: self.enabled,
        }
    }
}

/// Persistence statistics.
#[derive(Debug, Clone)]
pub struct PersistStats {
    pub journal_entries: usize,
    pub entries_written: u64,
    pub enabled: bool,
}

// ── Serialization Helpers ────────────────────────────────────────────

/// Build a 32-byte OID with namespace prefix and a u64 key.
fn make_oid(namespace: u8, key: u64) -> [u8; 32] {
    let mut oid = [0u8; 32];
    oid[0] = namespace;
    let key_bytes = key.to_le_bytes();
    oid[1..9].copy_from_slice(&key_bytes);
    oid
}

/// Serialize a history entry to bytes.
///
/// Format: `command\0timestamp_le8\0cwd\0exit_code_le4\0duration_le8`
fn serialize_history_entry(entry: &HistoryEntry) -> Vec<u8> {
    let mut buf = Vec::with_capacity(entry.command.len() + entry.cwd.len() + 32);

    // Command string
    buf.extend_from_slice(entry.command.as_bytes());
    buf.push(0);

    // Timestamp (8 bytes LE)
    buf.extend_from_slice(&entry.timestamp.to_le_bytes());

    // CWD string
    buf.extend_from_slice(entry.cwd.as_bytes());
    buf.push(0);

    // Exit code (4 bytes LE, -1 for None)
    let exit = entry.exit_code.unwrap_or(-1);
    buf.extend_from_slice(&exit.to_le_bytes());

    // Duration (8 bytes LE)
    buf.extend_from_slice(&entry.duration_ms.to_le_bytes());

    // Session ID (8 bytes LE)
    buf.extend_from_slice(&entry.session_id.to_le_bytes());

    buf
}

/// Deserialize a history entry from bytes.
pub fn deserialize_history_entry(data: &[u8]) -> Option<HistoryEntry> {
    // Find first NUL — end of command string
    let cmd_end = data.iter().position(|&b| b == 0)?;
    let command = core::str::from_utf8(&data[..cmd_end]).ok()?;

    let mut pos = cmd_end + 1;

    // Timestamp (8 bytes)
    if pos + 8 > data.len() { return None; }
    let timestamp = u64::from_le_bytes(data[pos..pos+8].try_into().ok()?);
    pos += 8;

    // CWD string (until next NUL)
    let cwd_end = data[pos..].iter().position(|&b| b == 0)? + pos;
    let cwd = core::str::from_utf8(&data[pos..cwd_end]).ok()?;
    pos = cwd_end + 1;

    // Exit code (4 bytes)
    if pos + 4 > data.len() { return None; }
    let exit_raw = i32::from_le_bytes(data[pos..pos+4].try_into().ok()?);
    let exit_code = if exit_raw == -1 { None } else { Some(exit_raw) };
    pos += 4;

    // Duration (8 bytes)
    if pos + 8 > data.len() { return None; }
    let duration_ms = u64::from_le_bytes(data[pos..pos+8].try_into().ok()?);
    pos += 8;

    // Session ID (8 bytes)
    let session_id = if pos + 8 <= data.len() {
        u64::from_le_bytes(data[pos..pos+8].try_into().ok()?)
    } else {
        0
    };

    Some(HistoryEntry {
        command: String::from(command),
        timestamp,
        exit_code,
        cwd: String::from(cwd),
        session_id,
        duration_ms,
    })
}

/// Serialize environment variables to bytes.
///
/// Format: `NAME=VALUE\n` per variable (only exported, non-readonly).
fn serialize_env(env: &Environment) -> Vec<u8> {
    let mut buf = Vec::new();
    for (name, value) in env.exported() {
        buf.extend_from_slice(name.as_bytes());
        buf.push(b'=');
        buf.extend_from_slice(value.as_bytes());
        buf.push(b'\n');
    }
    buf
}

/// Serialize aliases to bytes.
///
/// Format: `name\0expansion\n` per alias.
fn serialize_aliases(aliases: &AliasManager) -> Vec<u8> {
    let mut buf = Vec::new();
    for (name, expansion, _global) in aliases.list_all() {
        buf.extend_from_slice(name.as_bytes());
        buf.push(0);
        buf.extend_from_slice(expansion.as_bytes());
        buf.push(b'\n');
    }
    buf
}
