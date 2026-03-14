//! # Q-Shell Persistence Layer (IPC Stubbed)
//!
//! Because Q-Shell is a pure Ring 3 terminal application, it does NOT embed
//! the Prism Journal engine directly anymore.
//!
//! Persistence state changes are serialized and transmitted via Syscall 163 (IpcBatch)
//! to the Kernel Prism-Daemon Silo.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use crate::history::{History, HistoryEntry};
use crate::env::Environment;
use crate::alias::AliasManager;
use crate::executor::SyscallBroker;

/// The Q-Shell Persistence Manager (IPC Client).
pub struct PersistenceManager {
    pub enabled: bool,
    pub entries_written: u64,
}

impl PersistenceManager {
    /// Create a new Persistence IPC broker.
    pub fn new() -> Self {
        PersistenceManager {
            enabled: true,
            entries_written: 0,
        }
    }

    /// Journal a single history entry.
    pub fn journal_history_entry(&mut self, entry: &HistoryEntry) {
        if !self.enabled { return; }
        // Emulate sending a JournalInsert packet over IPC
        SyscallBroker::dispatch_command("journal", &["insert", "history", &entry.command]);
        self.entries_written += 1;
    }

    /// Save the full history.
    pub fn save_history(&mut self, _history: &History) {
        if !self.enabled { return; }
        SyscallBroker::dispatch_command("journal", &["batch-sync", "history"]);
    }

    /// Load history from the Journal Daemon.
    pub fn load_history(&self, session_id: u64) -> History {
        // Send a fetch request over IPC.
        SyscallBroker::dispatch_command("journal", &["fetch", "history"]);
        // Return blank history for now until asynchronous IPC shared buffers exist.
        History::new(session_id)
    }

    /// Save environment variables.
    pub fn save_env(&mut self, _env: &Environment) {
        if !self.enabled { return; }
        SyscallBroker::dispatch_command("journal", &["batch-sync", "env"]);
    }

    /// Load environment variables.
    pub fn load_env(&self, silo_id: u64) -> Environment {
        SyscallBroker::dispatch_command("journal", &["fetch", "env"]);
        Environment::new(silo_id)
    }

    /// Save all aliases.
    pub fn save_aliases(&mut self, _aliases: &AliasManager) {
        if !self.enabled { return; }
        SyscallBroker::dispatch_command("journal", &["batch-sync", "aliases"]);
    }

    /// Load aliases.
    pub fn load_aliases(&self) -> AliasManager {
        SyscallBroker::dispatch_command("journal", &["fetch", "aliases"]);
        AliasManager::new()
    }

    /// Request a Journal Checkpoint via IPC.
    pub fn checkpoint(&mut self) {
        SyscallBroker::dispatch_command("journal", &["checkpoint"]);
    }
}
