//! # Aether Intent UI — The Omni-Bar
//!
//! The Omni-Bar replaces the traditional Start Menu, taskbar, and
//! search bar with a single, context-aware command palette.
//! Think Spotlight + Command Palette + AI assistant in one surface.
//!
//! How it works (Section 4.3 of the spec):
//! - Press `Super` or think "open" via BCI → Omni-Bar appears
//! - Type naturally: "send file to Hasan" → routes to Q-Collab
//! - Context-aware: in a code editor, it shows code actions first
//! - Learns from usage: frequently used commands rise to the top
//! - Extensible: any Silo can register commands

extern crate alloc;

use alloc::collections::BTreeMap;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::string::String;
use alloc::vec::Vec;

/// Command category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CommandCategory {
    /// System commands (settings, power, display)
    System,
    /// Application launch
    AppLaunch,
    /// File/object search
    Search,
    /// Navigation (switch window, go to, open)
    Navigation,
    /// Action (send, share, delete, copy)
    Action,
    /// AI-powered (summarize, translate, generate)
    AiAssist,
    /// Context-specific (depends on active window)
    Contextual,
}

/// A registered Omni-Bar command.
#[derive(Debug, Clone)]
pub struct OmniCommand {
    /// Unique command ID
    pub id: u64,
    /// Display label (shown in the bar)
    pub label: String,
    /// Description (subtitle)
    pub description: String,
    /// Keywords (for fuzzy matching)
    pub keywords: Vec<String>,
    /// Category
    pub category: CommandCategory,
    /// Source Silo (who registered this command)
    pub silo_id: u64,
    /// Icon identifier
    pub icon: String,
    /// Usage count (for ranking)
    pub usage_count: u64,
    /// Last used timestamp
    pub last_used: u64,
    /// Is this visible in the current context?
    pub enabled: bool,
}

/// A search result / suggestion.
#[derive(Debug, Clone)]
pub struct Suggestion {
    /// Command ID
    pub command_id: u64,
    /// Relevance score (higher = better match)
    pub score: f32,
    /// Display label
    pub label: String,
    /// Subtitle
    pub subtitle: String,
    /// Category
    pub category: CommandCategory,
}

/// Context hint (what the user is currently doing).
#[derive(Debug, Clone)]
pub struct ContextHint {
    /// Active window/app name
    pub active_app: String,
    /// Active file extension (if editing a file)
    pub file_ext: Option<String>,
    /// Selected text (if any)
    pub selection: Option<String>,
    /// Clipboard content type
    pub clipboard_type: Option<String>,
}

/// Omni-Bar state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OmniState {
    /// Hidden
    Hidden,
    /// Visible, waiting for input
    Active,
    /// Showing suggestions
    Suggesting,
    /// Executing a command
    Executing,
}

/// Omni-Bar statistics.
#[derive(Debug, Clone, Default)]
pub struct OmniStats {
    pub opens: u64,
    pub queries: u64,
    pub commands_executed: u64,
    pub commands_registered: u64,
    pub ai_queries: u64,
}

/// The Omni-Bar (Intent UI Engine).
pub struct OmniBar {
    /// Registered commands
    pub commands: BTreeMap<u64, OmniCommand>,
    /// Current state
    pub state: OmniState,
    /// Current query string
    pub query: String,
    /// Current suggestions
    pub suggestions: Vec<Suggestion>,
    /// Current context
    pub context: Option<ContextHint>,
    /// Next command ID
    next_id: u64,
    /// Maximum suggestions to show
    pub max_suggestions: usize,
    /// Statistics
    pub stats: OmniStats,
}

impl OmniBar {
    pub fn new() -> Self {
        let mut bar = OmniBar {
            commands: BTreeMap::new(),
            state: OmniState::Hidden,
            query: String::new(),
            suggestions: Vec::new(),
            context: None,
            next_id: 1,
            max_suggestions: 8,
            stats: OmniStats::default(),
        };
        bar.register_builtin_commands();
        bar
    }

    /// Register built-in system commands.
    fn register_builtin_commands(&mut self) {
        let builtins = [
            ("Settings", "Open system settings", CommandCategory::System, &["preferences", "config"][..]),
            ("Lock Screen", "Lock the device", CommandCategory::System, &["lock", "away"]),
            ("Sleep", "Put the device to sleep", CommandCategory::System, &["suspend", "hibernate"]),
            ("Shutdown", "Shut down the system", CommandCategory::System, &["power off", "turn off"]),
            ("File Search", "Search for files and objects", CommandCategory::Search, &["find", "locate"]),
            ("Switch Window", "Switch to another window", CommandCategory::Navigation, &["alt tab", "switch"]),
            ("Clipboard History", "View clipboard history", CommandCategory::Action, &["paste", "copy"]),
            ("Screenshot", "Take a screenshot", CommandCategory::Action, &["capture", "snip"]),
        ];

        for (label, desc, cat, kws) in &builtins {
            self.register(label, desc, *cat, kws.iter().map(|s| String::from(*s)).collect(), 0);
        }
    }

    /// Register a command.
    pub fn register(
        &mut self,
        label: &str,
        description: &str,
        category: CommandCategory,
        keywords: Vec<String>,
        silo_id: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.commands.insert(id, OmniCommand {
            id,
            label: String::from(label),
            description: String::from(description),
            keywords,
            category,
            silo_id,
            icon: String::new(),
            usage_count: 0,
            last_used: 0,
            enabled: true,
        });

        self.stats.commands_registered += 1;
        id
    }

    /// Open the Omni-Bar.
    pub fn open(&mut self, context: Option<ContextHint>) {
        self.state = OmniState::Active;
        self.query.clear();
        self.suggestions.clear();
        self.context = context;
        self.stats.opens += 1;
    }

    /// Close the Omni-Bar.
    pub fn close(&mut self) {
        self.state = OmniState::Hidden;
        self.query.clear();
        self.suggestions.clear();
    }

    /// Update query and recalculate suggestions.
    pub fn set_query(&mut self, query: &str) {
        self.query = String::from(query);
        self.stats.queries += 1;
        self.update_suggestions();
        self.state = if self.suggestions.is_empty() {
            OmniState::Active
        } else {
            OmniState::Suggesting
        };
    }

    /// Calculate suggestions based on current query and context.
    fn update_suggestions(&mut self) {
        self.suggestions.clear();
        let query_lower = self.query.to_lowercase();

        if query_lower.is_empty() {
            // Show most-used commands
            let mut top: Vec<&OmniCommand> = self.commands.values()
                .filter(|c| c.enabled)
                .collect();
            top.sort_by(|a, b| b.usage_count.cmp(&a.usage_count));

            for cmd in top.iter().take(self.max_suggestions) {
                self.suggestions.push(Suggestion {
                    command_id: cmd.id,
                    score: cmd.usage_count as f32,
                    label: cmd.label.clone(),
                    subtitle: cmd.description.clone(),
                    category: cmd.category,
                });
            }
            return;
        }

        // Fuzzy match against labels and keywords
        for cmd in self.commands.values().filter(|c| c.enabled) {
            let mut score = 0.0f32;

            // Label match
            let label_lower = cmd.label.to_lowercase();
            if label_lower == query_lower {
                score += 100.0;
            } else if label_lower.starts_with(&query_lower) {
                score += 50.0;
            } else if label_lower.contains(&query_lower) {
                score += 25.0;
            }

            // Keyword match
            for kw in &cmd.keywords {
                let kw_lower = kw.to_lowercase();
                if kw_lower.starts_with(&query_lower) {
                    score += 30.0;
                } else if kw_lower.contains(&query_lower) {
                    score += 15.0;
                }
            }

            // Context boost
            if let Some(ctx) = &self.context {
                if cmd.category == CommandCategory::Contextual {
                    score += 20.0;
                }
                if cmd.label.to_lowercase().contains(&ctx.active_app.to_lowercase()) {
                    score += 10.0;
                }
            }

            // Frequency boost (log scale)
            if cmd.usage_count > 0 {
                score += (cmd.usage_count as f32).ln() * 5.0;
            }

            if score > 0.0 {
                self.suggestions.push(Suggestion {
                    command_id: cmd.id,
                    score,
                    label: cmd.label.clone(),
                    subtitle: cmd.description.clone(),
                    category: cmd.category,
                });
            }
        }

        // Sort by score descending
        self.suggestions.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(core::cmp::Ordering::Equal));
        self.suggestions.truncate(self.max_suggestions);
    }

    /// Execute a command by ID.
    pub fn execute(&mut self, command_id: u64, now: u64) -> Option<&OmniCommand> {
        if let Some(cmd) = self.commands.get_mut(&command_id) {
            cmd.usage_count += 1;
            cmd.last_used = now;
            self.stats.commands_executed += 1;
            self.state = OmniState::Executing;
        }
        self.commands.get(&command_id)
    }

    /// Execute the top suggestion.
    pub fn execute_top(&mut self, now: u64) -> Option<u64> {
        let top_id = self.suggestions.first()?.command_id;
        self.execute(top_id, now);
        Some(top_id)
    }
}
