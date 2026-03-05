//! # Q-Shell Tab Completion Engine
//!
//! Context-aware tab completion for the Q-Shell command line.
//! Supports path completion, command completion, argument/flag
//! completion, environment variable completion, and fuzzy matching
//! with ranked scoring.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Completion source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    /// A file path
    File,
    /// A directory path
    Directory,
    /// A command (builtin or from PATH)
    Command,
    /// An alias
    Alias,
    /// An environment variable
    Variable,
    /// A command flag (--flag)
    Flag,
    /// A subcommand
    Subcommand,
    /// A hostname
    Hostname,
    /// A username
    Username,
    /// A custom argument
    Custom,
}

impl CompletionKind {
    pub fn icon(&self) -> char {
        match self {
            CompletionKind::File      => '📄',
            CompletionKind::Directory => '📁',
            CompletionKind::Command   => '⚡',
            CompletionKind::Alias     => '🔗',
            CompletionKind::Variable  => '$',
            CompletionKind::Flag      => '🏳',
            CompletionKind::Subcommand => '▸',
            CompletionKind::Hostname  => '🌐',
            CompletionKind::Username  => '👤',
            CompletionKind::Custom    => '•',
        }
    }
}

/// A single completion candidate.
#[derive(Debug, Clone)]
pub struct Completion {
    /// The completed text (to be inserted)
    pub text: String,
    /// Display label (may differ from text, e.g., include description)
    pub display: String,
    /// Kind of completion
    pub kind: CompletionKind,
    /// Match score (higher = better match)
    pub score: i32,
    /// Description / annotation (e.g., "builtin", "directory")
    pub description: String,
    /// Add trailing slash (for directories) or space (for commands)?
    pub suffix: CompletionSuffix,
}

/// What to append after inserting the completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSuffix {
    /// Append a trailing space
    Space,
    /// Append a trailing slash (for directories)
    Slash,
    /// Append nothing (for partial completions)
    None,
}

/// Where in the command line the cursor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorContext {
    /// At the very first word (command position)
    CommandPosition,
    /// After a command, at an argument position
    ArgumentPosition,
    /// After a '-' or '--' (flag position)
    FlagPosition,
    /// After a '$' (variable name)
    VariablePosition,
    /// After a '~' (home dir)
    TildePosition,
}

/// Match type for scoring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MatchType {
    /// Exact prefix match (highest score)
    Prefix,
    /// Case-insensitive prefix
    PrefixInsensitive,
    /// Substring match
    Substring,
    /// Fuzzy match (characters in order but not contiguous)
    Fuzzy,
    /// No match
    None,
}

// ─── Fuzzy Matching ─────────────────────────────────────────────────────────

/// Compute match type and score for a query against a candidate.
fn score_match(query: &str, candidate: &str) -> (MatchType, i32) {
    if query.is_empty() {
        return (MatchType::Prefix, 0); // Empty query matches everything
    }

    // Exact prefix match
    if candidate.starts_with(query) {
        let score = 100 + (query.len() as i32 * 10) - (candidate.len() as i32);
        return (MatchType::Prefix, score);
    }

    // Case-insensitive prefix
    let q_lower = query.to_ascii_lowercase();
    let c_lower = candidate.to_ascii_lowercase();
    if c_lower.starts_with(&q_lower) {
        let score = 80 + (query.len() as i32 * 8) - (candidate.len() as i32);
        return (MatchType::PrefixInsensitive, score);
    }

    // Substring match
    if c_lower.contains(&q_lower) {
        let score = 50 + (query.len() as i32 * 5) - (candidate.len() as i32);
        return (MatchType::Substring, score);
    }

    // Fuzzy match: all query chars appear in order in candidate
    let fuzzy_score = fuzzy_score(&q_lower, &c_lower);
    if fuzzy_score > 0 {
        return (MatchType::Fuzzy, fuzzy_score);
    }

    (MatchType::None, 0)
}

/// Compute a fuzzy match score. Returns 0 if no match.
fn fuzzy_score(query: &str, candidate: &str) -> i32 {
    let query_chars: Vec<char> = query.chars().collect();
    let candidate_chars: Vec<char> = candidate.chars().collect();

    let mut qi = 0;
    let mut score: i32 = 0;
    let mut prev_match = false;
    let mut prev_idx: i32 = -1;

    for (ci, &cc) in candidate_chars.iter().enumerate() {
        if qi < query_chars.len() && cc == query_chars[qi] {
            qi += 1;
            score += 10;

            // Bonus for consecutive matches
            if prev_match {
                score += 5;
            }

            // Bonus for matching after separator (/, -, _)
            if ci > 0 {
                let prev_c = candidate_chars[ci - 1];
                if prev_c == '/' || prev_c == '-' || prev_c == '_' || prev_c == '.' {
                    score += 8;
                }
            }

            // Bonus for matching at start
            if ci == 0 { score += 15; }

            // Penalty for distance between matches
            if prev_idx >= 0 {
                let gap = (ci as i32) - prev_idx - 1;
                score -= gap;
            }

            prev_match = true;
            prev_idx = ci as i32;
        } else {
            prev_match = false;
        }
    }

    if qi == query_chars.len() {
        // Penalty for longer candidates
        score -= (candidate_chars.len() as i32) / 4;
        score.max(1) // At least 1 if all chars matched
    } else {
        0 // Not all query chars found
    }
}

// ─── Completion Engine ──────────────────────────────────────────────────────

/// Built-in shell commands.
const BUILTIN_COMMANDS: &[&str] = &[
    "cd", "ls", "pwd", "echo", "cat", "cp", "mv", "rm", "mkdir", "rmdir",
    "touch", "head", "tail", "grep", "find", "sort", "wc", "clear",
    "exit", "history", "alias", "unalias", "export", "env", "set", "unset",
    "source", "exec", "help", "man", "which", "type", "jobs", "fg", "bg",
    "kill", "ps", "top", "df", "du", "chmod", "chown", "ln", "tar",
    "curl", "ping", "ssh", "scp", "git",
];

/// The Tab Completion Engine.
pub struct CompletionEngine {
    /// Current completions
    pub candidates: Vec<Completion>,
    /// Currently selected candidate index
    pub selected: Option<usize>,
    /// Is the completion dropdown visible?
    pub visible: bool,
    /// Known aliases (name, expansion)
    pub aliases: Vec<(String, String)>,
    /// Known environment variable names
    pub env_vars: Vec<String>,
    /// Custom command completors (command → subcommands/flags)
    pub custom_completors: Vec<CustomCompletor>,
    /// PATH directories for command lookup
    pub path_dirs: Vec<String>,
    /// Stats
    pub stats: CompletionStats,
}

/// Custom completor for specific commands (e.g., git, cargo).
#[derive(Debug, Clone)]
pub struct CustomCompletor {
    pub command: String,
    pub subcommands: Vec<String>,
    pub flags: Vec<String>,
}

/// Completion statistics.
#[derive(Debug, Clone, Default)]
pub struct CompletionStats {
    pub completions_offered: u64,
    pub completions_accepted: u64,
    pub fuzzy_matches: u64,
    pub prefix_matches: u64,
}

impl CompletionEngine {
    pub fn new() -> Self {
        let mut engine = CompletionEngine {
            candidates: Vec::new(),
            selected: None,
            visible: false,
            aliases: Vec::new(),
            env_vars: alloc::vec![
                String::from("HOME"), String::from("PATH"), String::from("USER"),
                String::from("SHELL"), String::from("PWD"), String::from("TERM"),
                String::from("LANG"), String::from("EDITOR"),
            ],
            custom_completors: Vec::new(),
            path_dirs: alloc::vec![
                String::from("/system/bin"),
                String::from("/apps/bin"),
            ],
            stats: CompletionStats::default(),
        };

        // Register common command completors
        engine.register_git_completor();
        engine.register_cargo_completor();

        engine
    }

    /// Trigger completion for the given input line and cursor position.
    pub fn complete(&mut self, line: &str, cursor: usize) -> &[Completion] {
        self.candidates.clear();
        self.selected = None;

        let line = &line[..cursor.min(line.len())];
        let (word, context) = self.analyze_context(line);

        match context {
            CursorContext::CommandPosition => {
                self.complete_command(&word);
            }
            CursorContext::ArgumentPosition => {
                let cmd = self.extract_command(line);
                self.complete_argument(&word, &cmd);
            }
            CursorContext::FlagPosition => {
                let cmd = self.extract_command(line);
                self.complete_flags(&word, &cmd);
            }
            CursorContext::VariablePosition => {
                self.complete_variable(&word);
            }
            CursorContext::TildePosition => {
                self.complete_tilde(&word);
            }
        }

        // Sort by score (highest first)
        self.candidates.sort_by(|a, b| b.score.cmp(&a.score));

        // Limit to top 20
        self.candidates.truncate(20);

        if !self.candidates.is_empty() {
            self.visible = true;
            self.selected = Some(0);
            self.stats.completions_offered += 1;
        }

        &self.candidates
    }

    /// Analyze the cursor context and extract the current word.
    fn analyze_context(&self, line: &str) -> (String, CursorContext) {
        let trimmed = line.trim_start();

        // Find the last word being typed
        let last_word = trimmed.rsplit(|c: char| c.is_whitespace())
            .next()
            .unwrap_or("");

        // Determine context
        if last_word.starts_with('$') {
            return (last_word[1..].to_string(), CursorContext::VariablePosition);
        }
        if last_word.starts_with("--") || last_word.starts_with('-') {
            return (last_word.to_string(), CursorContext::FlagPosition);
        }
        if last_word.starts_with('~') {
            return (last_word.to_string(), CursorContext::TildePosition);
        }

        // Check if we're at command position (first word)
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.is_empty() || (words.len() == 1 && !line.ends_with(' ')) {
            (last_word.to_string(), CursorContext::CommandPosition)
        } else {
            (last_word.to_string(), CursorContext::ArgumentPosition)
        }
    }

    /// Extract the command name from the line.
    fn extract_command(&self, line: &str) -> String {
        line.trim_start()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string()
    }

    /// Complete command names (builtins + aliases + PATH).
    fn complete_command(&mut self, query: &str) {
        // Builtins
        for &cmd in BUILTIN_COMMANDS {
            let (mt, score) = score_match(query, cmd);
            if mt != MatchType::None {
                if mt == MatchType::Fuzzy { self.stats.fuzzy_matches += 1; }
                else { self.stats.prefix_matches += 1; }
                self.candidates.push(Completion {
                    text: String::from(cmd),
                    display: String::from(cmd),
                    kind: CompletionKind::Command,
                    score,
                    description: String::from("builtin"),
                    suffix: CompletionSuffix::Space,
                });
            }
        }

        // Aliases
        for (name, expansion) in &self.aliases {
            let (mt, score) = score_match(query, name);
            if mt != MatchType::None {
                self.candidates.push(Completion {
                    text: name.clone(),
                    display: name.clone(),
                    kind: CompletionKind::Alias,
                    score: score + 5, // Slight boost for aliases
                    description: alloc::format!("→ {}", expansion),
                    suffix: CompletionSuffix::Space,
                });
            }
        }

        // Also try path completion if query contains '/'
        if query.contains('/') || query.starts_with('.') {
            self.complete_path(query);
        }
    }

    /// Complete file/directory paths.
    fn complete_path(&mut self, query: &str) {
        // Split into directory and partial filename
        let (dir, partial) = if let Some(sep) = query.rfind('/') {
            (&query[..=sep], &query[sep + 1..])
        } else {
            ("./", query)
        };

        // In production: read directory entries from Prism VFS
        // For now: generate plausible completions from VFS standard dirs
        let standard_entries = [
            ("system", true), ("users", true), ("apps", true),
            ("drivers", true), ("temp", true), ("chimera", true),
        ];

        if dir == "/" || dir == "./" {
            for (name, is_dir) in &standard_entries {
                let (mt, score) = score_match(partial, name);
                if mt != MatchType::None {
                    self.candidates.push(Completion {
                        text: alloc::format!("{}{}", dir, name),
                        display: String::from(*name),
                        kind: if *is_dir { CompletionKind::Directory } else { CompletionKind::File },
                        score,
                        description: if *is_dir { String::from("dir") } else { String::from("file") },
                        suffix: if *is_dir { CompletionSuffix::Slash } else { CompletionSuffix::Space },
                    });
                }
            }
        }
    }

    /// Complete command arguments (context-aware).
    fn complete_argument(&mut self, query: &str, command: &str) {
        // Check for custom completor
        if let Some(cc) = self.custom_completors.iter().find(|c| c.command == command) {
            for sub in &cc.subcommands {
                let (mt, score) = score_match(query, sub);
                if mt != MatchType::None {
                    self.candidates.push(Completion {
                        text: sub.clone(),
                        display: sub.clone(),
                        kind: CompletionKind::Subcommand,
                        score,
                        description: alloc::format!("{} subcommand", command),
                        suffix: CompletionSuffix::Space,
                    });
                }
            }
        }

        // Path completions for file-oriented commands
        let file_commands = ["cat", "less", "head", "tail", "cp", "mv", "rm",
                             "chmod", "chown", "source", "ls", "cd", "mkdir"];
        if file_commands.contains(&command) {
            self.complete_path(query);
        }
    }

    /// Complete flags (--flag or -f).
    fn complete_flags(&mut self, query: &str, command: &str) {
        if let Some(cc) = self.custom_completors.iter().find(|c| c.command == command) {
            for flag in &cc.flags {
                let (mt, score) = score_match(query, flag);
                if mt != MatchType::None {
                    self.candidates.push(Completion {
                        text: flag.clone(),
                        display: flag.clone(),
                        kind: CompletionKind::Flag,
                        score,
                        description: String::new(),
                        suffix: CompletionSuffix::Space,
                    });
                }
            }
        }

        // Common flags for all commands
        let common = ["--help", "--version", "--verbose", "-h", "-v"];
        for flag in &common {
            let (mt, score) = score_match(query, flag);
            if mt != MatchType::None {
                self.candidates.push(Completion {
                    text: String::from(*flag),
                    display: String::from(*flag),
                    kind: CompletionKind::Flag,
                    score: score - 10, // Lower priority than command-specific flags
                    description: String::new(),
                    suffix: CompletionSuffix::Space,
                });
            }
        }
    }

    /// Complete environment variable names.
    fn complete_variable(&mut self, query: &str) {
        for var in &self.env_vars {
            let (mt, score) = score_match(query, var);
            if mt != MatchType::None {
                self.candidates.push(Completion {
                    text: alloc::format!("${}", var),
                    display: alloc::format!("${}", var),
                    kind: CompletionKind::Variable,
                    score,
                    description: String::from("env"),
                    suffix: CompletionSuffix::None,
                });
            }
        }
    }

    /// Complete tilde expansion.
    fn complete_tilde(&mut self, _query: &str) {
        self.candidates.push(Completion {
            text: String::from("~/"),
            display: String::from("~ (home)"),
            kind: CompletionKind::Directory,
            score: 100,
            description: String::from("/users/home"),
            suffix: CompletionSuffix::None,
        });
    }

    /// Get the longest common prefix among all candidates.
    pub fn common_prefix(&self) -> String {
        if self.candidates.is_empty() { return String::new(); }
        if self.candidates.len() == 1 { return self.candidates[0].text.clone(); }

        let first = &self.candidates[0].text;
        let mut prefix_len = first.len();

        for candidate in &self.candidates[1..] {
            let common = first.chars().zip(candidate.text.chars())
                .take_while(|(a, b)| a == b)
                .count();
            prefix_len = prefix_len.min(common);
        }

        first[..prefix_len].to_string()
    }

    /// Select next candidate (Tab).
    pub fn select_next(&mut self) {
        if self.candidates.is_empty() { return; }
        self.selected = Some(match self.selected {
            Some(i) => (i + 1) % self.candidates.len(),
            None => 0,
        });
    }

    /// Select previous candidate (Shift-Tab).
    pub fn select_prev(&mut self) {
        if self.candidates.is_empty() { return; }
        self.selected = Some(match self.selected {
            Some(0) => self.candidates.len() - 1,
            Some(i) => i - 1,
            None => self.candidates.len() - 1,
        });
    }

    /// Accept the currently selected completion.
    pub fn accept(&mut self) -> Option<Completion> {
        let idx = self.selected?;
        let completion = self.candidates.get(idx)?.clone();
        self.stats.completions_accepted += 1;
        self.dismiss();
        Some(completion)
    }

    /// Dismiss the completion dropdown.
    pub fn dismiss(&mut self) {
        self.visible = false;
        self.candidates.clear();
        self.selected = None;
    }

    /// Register an alias for completion.
    pub fn register_alias(&mut self, name: &str, expansion: &str) {
        self.aliases.push((String::from(name), String::from(expansion)));
    }

    /// Register an environment variable.
    pub fn register_var(&mut self, name: &str) {
        if !self.env_vars.iter().any(|v| v == name) {
            self.env_vars.push(String::from(name));
        }
    }

    // ─── Built-in Custom Completors ─────────────────────────────────

    fn register_git_completor(&mut self) {
        self.custom_completors.push(CustomCompletor {
            command: String::from("git"),
            subcommands: ["add", "commit", "push", "pull", "fetch", "clone",
                          "checkout", "branch", "merge", "rebase", "status",
                          "log", "diff", "stash", "reset", "tag", "remote",
                          "init", "show", "bisect", "cherry-pick"]
                .iter().map(|s| String::from(*s)).collect(),
            flags: ["--all", "--force", "--verbose", "--quiet", "--dry-run",
                    "--no-edit", "--amend", "--interactive", "--cached",
                    "--staged", "--oneline", "--graph", "--stat"]
                .iter().map(|s| String::from(*s)).collect(),
        });
    }

    fn register_cargo_completor(&mut self) {
        self.custom_completors.push(CustomCompletor {
            command: String::from("cargo"),
            subcommands: ["build", "run", "test", "check", "bench", "doc",
                          "clean", "update", "publish", "install", "new",
                          "init", "add", "remove", "fmt", "clippy", "fix"]
                .iter().map(|s| String::from(*s)).collect(),
            flags: ["--release", "--verbose", "--quiet", "--target", "--jobs",
                    "--features", "--all-features", "--no-default-features",
                    "--workspace", "--manifest-path", "--locked"]
                .iter().map(|s| String::from(*s)).collect(),
        });
    }
}
