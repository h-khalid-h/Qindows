//! # Q-Shell Command History
//!
//! Persistent command history with search, deduplication,
//! and per-session tracking. Supports reverse-i-search (Ctrl+R).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;

/// A single history entry.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    /// The command string
    pub command: String,
    /// Execution timestamp
    pub timestamp: u64,
    /// Exit code (None if still running)
    pub exit_code: Option<i32>,
    /// Working directory when executed
    pub cwd: String,
    /// Session ID
    pub session_id: u64,
    /// Execution duration (ms)
    pub duration_ms: u64,
}

/// History search result.
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub entry: HistoryEntry,
    pub index: usize,
    pub match_pos: usize,
}

/// The Shell History Manager.
pub struct History {
    /// All entries (ordered by time)
    pub entries: Vec<HistoryEntry>,
    /// Cursor position for up/down navigation
    pub cursor: Option<usize>,
    /// Max entries
    pub max_entries: usize,
    /// Current session ID
    pub session_id: u64,
    /// Frequency map for ranking
    pub frequency: BTreeMap<String, u32>,
    /// Current search query (for Ctrl+R)
    pub search_query: Option<String>,
    /// Current search match index
    pub search_index: Option<usize>,
}

impl History {
    pub fn new(session_id: u64) -> Self {
        History {
            entries: Vec::new(),
            cursor: None,
            max_entries: 10_000,
            session_id,
            frequency: BTreeMap::new(),
            search_query: None,
            search_index: None,
        }
    }

    /// Add a command to history.
    pub fn push(&mut self, command: &str, cwd: &str, timestamp: u64) {
        // Skip empty commands and duplicates of the last command
        if command.is_empty() { return; }
        if let Some(last) = self.entries.last() {
            if last.command == command { return; }
        }

        // Track frequency
        *self.frequency.entry(String::from(command)).or_insert(0) += 1;

        self.entries.push(HistoryEntry {
            command: String::from(command),
            timestamp,
            exit_code: None,
            cwd: String::from(cwd),
            session_id: self.session_id,
            duration_ms: 0,
        });

        // Trim old entries
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }

        // Reset cursor
        self.cursor = None;
    }

    /// Record the result of the last command.
    pub fn record_result(&mut self, exit_code: i32, duration_ms: u64) {
        if let Some(last) = self.entries.last_mut() {
            last.exit_code = Some(exit_code);
            last.duration_ms = duration_ms;
        }
    }

    /// Navigate up (previous command).
    pub fn up(&mut self) -> Option<&str> {
        if self.entries.is_empty() { return None; }

        let idx = match self.cursor {
            Some(0) => return Some(&self.entries[0].command),
            Some(c) => c - 1,
            None => self.entries.len() - 1,
        };

        self.cursor = Some(idx);
        Some(&self.entries[idx].command)
    }

    /// Navigate down (next command).
    pub fn down(&mut self) -> Option<&str> {
        let cursor = self.cursor?;

        if cursor + 1 >= self.entries.len() {
            self.cursor = None;
            return None; // Back to empty prompt
        }

        self.cursor = Some(cursor + 1);
        Some(&self.entries[cursor + 1].command)
    }

    /// Search history for a substring (reverse-i-search).
    pub fn search(&self, query: &str) -> Vec<SearchResult> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        for (i, entry) in self.entries.iter().enumerate().rev() {
            let cmd_lower = entry.command.to_lowercase();
            if let Some(pos) = cmd_lower.find(&query_lower) {
                results.push(SearchResult {
                    entry: entry.clone(),
                    index: i,
                    match_pos: pos,
                });
                if results.len() >= 20 { break; }
            }
        }

        results
    }

    /// Start interactive search (Ctrl+R).
    pub fn begin_search(&mut self) {
        self.search_query = Some(String::new());
        self.search_index = None;
    }

    /// Add a character to the search query.
    pub fn search_char(&mut self, ch: char) -> Option<&str> {
        if let Some(ref mut query) = self.search_query {
            query.push(ch);
        }
        let query = match self.search_query {
            Some(ref q) => q.clone(),
            None => return None,
        };
        let results = self.search(&query);
        if let Some(first) = results.first() {
            self.search_index = Some(first.index);
            return Some(&self.entries[first.index].command);
        }
        None
    }

    /// End interactive search.
    pub fn end_search(&mut self) -> Option<String> {
        self.search_query = None;
        let idx = self.search_index.take()?;
        self.entries.get(idx).map(|e| e.command.clone())
    }

    /// Get the N most frequently used commands.
    pub fn most_frequent(&self, n: usize) -> Vec<(&str, u32)> {
        let mut freq: Vec<(&str, u32)> = self.frequency.iter()
            .map(|(cmd, count)| (cmd.as_str(), *count))
            .collect();
        freq.sort_by(|a, b| b.1.cmp(&a.1));
        freq.truncate(n);
        freq
    }

    /// Get commands from the current session only.
    pub fn current_session(&self) -> Vec<&HistoryEntry> {
        self.entries.iter().filter(|e| e.session_id == self.session_id).collect()
    }

    /// Total commands executed.
    pub fn total_commands(&self) -> usize {
        self.entries.len()
    }
}
