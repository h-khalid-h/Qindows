//! # Q-Shell Readline — Line Editing & History Navigation
//!
//! Provides terminal-style line editing, history navigation,
//! and keybinding support for the Q-Shell command palette
//! (Section 6.1).
//!
//! Features:
//! - Emacs/Vi keybinding modes
//! - History search (Ctrl+R reverse incremental)
//! - Multi-line editing
//! - Cursor word-navigation (Ctrl+Left/Right)
//! - Kill ring (Ctrl+K/Y)
//! - Undo buffer

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Keybinding mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    Emacs,
    Vi,
}

/// Vi sub-mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViMode {
    Insert,
    Normal,
    Visual,
}

/// A line buffer with cursor.
#[derive(Debug, Clone)]
pub struct LineBuffer {
    pub text: String,
    pub cursor: usize,   // Byte position
    pub mark: Option<usize>, // Selection start
}

impl LineBuffer {
    pub fn new() -> Self {
        LineBuffer { text: String::new(), cursor: 0, mark: None }
    }

    /// Insert a character at cursor.
    pub fn insert(&mut self, ch: char) {
        if self.cursor <= self.text.len() && self.text.is_char_boundary(self.cursor) {
            self.text.insert(self.cursor, ch);
            self.cursor += ch.len_utf8();
        }
    }

    /// Delete character before cursor (backspace).
    pub fn backspace(&mut self) -> bool {
        if self.cursor == 0 { return false; }
        // Find previous char boundary
        let prev = self.text[..self.cursor].char_indices()
            .last().map(|(i, _)| i).unwrap_or(0);
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
        true
    }

    /// Delete character at cursor.
    pub fn delete(&mut self) -> bool {
        if self.cursor >= self.text.len() { return false; }
        let next = self.text[self.cursor..].char_indices()
            .nth(1).map(|(i, _)| self.cursor + i)
            .unwrap_or(self.text.len());
        self.text.drain(self.cursor..next);
        true
    }

    /// Move cursor left.
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor].char_indices()
                .last().map(|(i, _)| i).unwrap_or(0);
        }
    }

    /// Move cursor right.
    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..].char_indices()
                .nth(1).map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
        }
    }

    /// Move to start of line.
    pub fn home(&mut self) { self.cursor = 0; }

    /// Move to end of line.
    pub fn end(&mut self) { self.cursor = self.text.len(); }

    /// Kill from cursor to end (Ctrl+K).
    pub fn kill_to_end(&mut self) -> String {
        let killed = String::from(&self.text[self.cursor..]);
        self.text.truncate(self.cursor);
        killed
    }

    /// Move cursor to next word boundary.
    pub fn word_forward(&mut self) {
        let remaining = &self.text[self.cursor..];
        // Skip non-whitespace then whitespace
        let skip_word = remaining.find(char::is_whitespace).unwrap_or(remaining.len());
        let after_ws = remaining[skip_word..].find(|c: char| !c.is_whitespace())
            .map(|i| skip_word + i).unwrap_or(remaining.len());
        self.cursor += after_ws;
    }

    /// Move cursor to previous word boundary.
    pub fn word_backward(&mut self) {
        let before = &self.text[..self.cursor];
        let trimmed = before.trim_end();
        self.cursor = trimmed.rfind(char::is_whitespace)
            .map(|i| i + 1).unwrap_or(0);
    }

    /// Clear the buffer.
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.mark = None;
    }
}

/// Readline statistics.
#[derive(Debug, Clone, Default)]
pub struct ReadlineStats {
    pub lines_entered: u64,
    pub history_searches: u64,
    pub completions_used: u64,
    pub undo_count: u64,
}

/// The Q-Shell Readline.
pub struct Readline {
    pub buffer: LineBuffer,
    pub history: Vec<String>,
    pub history_pos: Option<usize>,
    pub max_history: usize,
    pub kill_ring: Vec<String>,
    pub undo_stack: Vec<String>,
    pub mode: KeyMode,
    pub vi_mode: ViMode,
    pub stats: ReadlineStats,
}

impl Readline {
    pub fn new(mode: KeyMode) -> Self {
        Readline {
            buffer: LineBuffer::new(),
            history: Vec::new(), history_pos: None,
            max_history: 1000,
            kill_ring: Vec::new(), undo_stack: Vec::new(),
            mode, vi_mode: ViMode::Insert,
            stats: ReadlineStats::default(),
        }
    }

    /// Submit current line.
    pub fn submit(&mut self) -> String {
        let line = self.buffer.text.clone();
        if !line.is_empty() {
            self.history.push(line.clone());
            if self.history.len() > self.max_history {
                self.history.remove(0);
            }
        }
        self.buffer.clear();
        self.history_pos = None;
        self.undo_stack.clear();
        self.stats.lines_entered += 1;
        line
    }

    /// Navigate history up.
    pub fn history_prev(&mut self) {
        if self.history.is_empty() { return; }
        let pos = match self.history_pos {
            Some(p) if p > 0 => p - 1,
            None => self.history.len() - 1,
            _ => return,
        };
        self.history_pos = Some(pos);
        self.buffer.text = self.history[pos].clone();
        self.buffer.end();
    }

    /// Navigate history down.
    pub fn history_next(&mut self) {
        match self.history_pos {
            Some(p) if p + 1 < self.history.len() => {
                self.history_pos = Some(p + 1);
                self.buffer.text = self.history[p + 1].clone();
                self.buffer.end();
            }
            Some(_) => {
                self.history_pos = None;
                self.buffer.clear();
            }
            None => {}
        }
    }

    /// Reverse history search.
    pub fn search_history(&self, query: &str) -> Option<&str> {
        self.history.iter().rev()
            .find(|h| h.contains(query))
            .map(|s| s.as_str())
    }

    /// Save undo snapshot.
    pub fn save_undo(&mut self) {
        self.undo_stack.push(self.buffer.text.clone());
    }

    /// Undo last change.
    pub fn undo(&mut self) {
        if let Some(prev) = self.undo_stack.pop() {
            self.buffer.text = prev;
            self.buffer.end();
            self.stats.undo_count += 1;
        }
    }
}
