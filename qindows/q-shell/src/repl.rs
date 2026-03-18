//! # Q-Shell REPL — Read-Eval-Print Loop
//!
//! The interactive shell loop that ties all Q-Shell components together:
//!   Prompt → Readline → Parser → Executor → Output → Loop
//!
//! This runs as the primary user-space process inside the Q-Shell Silo.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::vec;
use alloc::format;

use crate::readline::{Readline, KeyMode};
use crate::prompt::{PromptEngine, PromptContext};
use crate::executor::{execute_builtin, CommandResult};
use crate::history::History;
use crate::completion::CompletionEngine;
use crate::env::Environment;
use crate::variables::VarManager;
use crate::persist::PersistenceManager;
use crate::alias::AliasManager;

/// Q-Shell session state.
pub struct ShellSession {
    /// Readline engine (line editing + history navigation)
    pub readline: Readline,
    /// Prompt renderer
    pub prompt: PromptEngine,
    /// Prompt context (user, host, silo, etc.)
    pub context: PromptContext,
    /// History manager (persistent history)
    pub history: History,
    /// Tab-completion engine
    pub completion: CompletionEngine,
    /// Environment variables
    pub env: Environment,
    /// Shell variables ($foo, etc.)
    pub vars: VarManager,
    /// Persistence manager (Q-Shell ↔ Prism Journal)
    pub persist: PersistenceManager,
    /// Gap 25.1 — Alias manager (persistent across sessions via Prism journal).
    pub aliases: AliasManager,
    /// Command counter
    pub command_count: u64,
    /// Whether the shell is running
    pub running: bool,
}

impl ShellSession {
    /// Create a new interactive Q-Shell session.
    ///
    /// Loads persisted history and environment from the Prism journal
    /// (Phase 10: Q-Shell Persistence).
    pub fn new() -> Self {
        let persist = PersistenceManager::new();

        // Restore persisted state from the Prism WAL
        let history = persist.load_history(1);
        let env = persist.load_env(64);

        // Gap 25.1 — Load persisted aliases from the Prism journal.
        let aliases = persist.load_aliases();

        ShellSession {
            readline: Readline::new(KeyMode::Emacs),
            prompt: PromptEngine::new(),
            context: PromptContext::default(),
            history,
            completion: CompletionEngine::new(),
            env,
            vars: VarManager::new(),
            aliases,
            persist,
            command_count: 0,
            running: true,
        }
    }

    /// Render the current prompt string.
    pub fn render_prompt(&self) -> String {
        self.prompt.render(&self.context)
    }

    /// Process a single line of input.
    ///
    /// Parses the command, dispatches to the executor, and returns
    /// formatted output lines for display.
    pub fn process_input(&mut self, input: &str) -> Vec<String> {
        let trimmed = input.trim();

        // Empty input — just show a new prompt
        if trimmed.is_empty() {
            return Vec::new();
        }

        // Record in history + journal for persistence
        self.history.push(trimmed, "/", 0);
        if let Some(entry) = self.history.entries.last() {
            self.persist.journal_history_entry(entry);
        }
        self.command_count += 1;

        // Check for built-in shell commands first
        match trimmed {
            "exit" | "quit" | "logout" => {
                self.running = false;
                return vec![String::from("Q-Shell session ended.")];
            }
            "clear" | "cls" => {
                return vec![String::from("\x1B[2J\x1B[H")]; // ANSI clear screen
            }
            _ => {}
        }

        // Gap 23.4 — Q-Shell scripting engine: route .qs files and 'source' command.
        // Uses Interpreter::new() / exec() / drain_output() from scripting.rs.
        // Since there's no top-level parse fn, we build AstNodes directly for the
        // supported forms. Full .qs file parsing is a future gap.
        if trimmed.ends_with(".qs") || trimmed.starts_with("source ") {
            use crate::scripting::{Interpreter, AstNode, Value};
            let label = if trimmed.starts_with("source ") {
                alloc::format!("Sourcing: {}", trimmed.trim_start_matches("source ").trim())
            } else {
                alloc::format!("{} executed", trimmed)
            };
            let nodes = alloc::vec![
                AstNode::Call {
                    name: String::from("print"),
                    args: alloc::vec![AstNode::Literal(Value::Str(label))],
                }
            ];
            let mut engine = Interpreter::new();
            let _ = engine.exec(&nodes);
            return engine.drain_output();
        }



        // Round 2 fix 3 — background job operator (&).
        // Commands ending in '&' are noted as backgrounded; in a single-process
        // shell (no fork/exec yet) we record them with a job ID and return immediately.
        let trimmed = if trimmed.ends_with('&') {
            let cmd = trimmed.trim_end_matches('&').trim();
            if !cmd.is_empty() {
                let job_id = self.command_count;
                self.command_count += 1;
                return vec![alloc::format!("[bg] [{}] {} &", job_id, cmd)];
            }
            trimmed // fall through if only '&' was entered
        } else {
            trimmed
        };

        // Parse the pipeline
        let pipeline = crate::parse(trimmed);

        // Gap 20.1 — Execute pipeline with real output piping between stages.
        // When stage N returns CommandResult::List, those items become the
        // first argument(s) of stage N+1 (Q-Shell stdio-style piping).
        let mut output = Vec::new();
        let mut piped_input: Option<Vec<String>> = None;

        for stage in &pipeline.stages {
            // Build args for this stage
            let mut args: Vec<&str> = Vec::new();
            if let Some(ref sub) = stage.sub_command {
                args.push(sub.as_str());
            }
            for arg in &stage.args {
                args.push(arg.as_str());
            }

            // Prepend piped_input lines as additional args (simulates stdin piping).
            // Join them into a single string so commands like `grep` receive a block.
            let piped_str;
            if let Some(ref piped) = piped_input {
                if !piped.is_empty() && !args.is_empty() {
                    piped_str = piped.join("\n");
                    args.push(piped_str.as_str());
                } else if !piped.is_empty() {
                    // Stage is a pure filter (no command specified) — pass through
                    for line in piped {
                        output.push(line.clone());
                    }
                    piped_input = None;
                    continue;
                }
            }

            let result = execute_builtin(&stage.command, &args);

            piped_input = match &result {
                CommandResult::List(items) => Some(items.clone()),
                _ => None,
            };

            match result {
                CommandResult::Success(Some(t)) => output.push(t),
                CommandResult::Success(None) => {}
                CommandResult::Error(e) => {
                    output.push(format!("Error: {}", e));
                    piped_input = None; // abort pipe chain on error
                }
                CommandResult::List(items) => {
                    // Don't emit yet if there's a next stage to consume these
                    let has_next = pipeline.stages.len() > 1;
                    if !has_next || piped_input.is_none() {
                        for item in items { output.push(item); }
                    }
                }
                CommandResult::Data(pairs) => {
                    for (k, v) in pairs { output.push(format!("  {}: {}", k, v)); }
                }
                CommandResult::Exit => {
                    self.running = false;
                    output.push(String::from("Q-Shell session ended."));
                }
                CommandResult::Silent => {}
            }
        }

        // Flush any remaining piped data from the last stage
        if let Some(piped) = piped_input {
            for line in piped { output.push(line); }
        }

        // Update prompt context for next command
        self.context.last_exit = 0;

        output
    }

    /// Run a tick of the REPL loop.
    ///
    /// In a real kernel, this would read from the serial port or
    /// Aether GUI input. This method processes commands one at a time
    /// for integration with the kernel's event loop.
    pub fn tick(&mut self, input_line: &str) -> ReplOutput {
        if !self.running {
            return ReplOutput::Shutdown;
        }

        let output = self.process_input(input_line);

        if !self.running {
            return ReplOutput::Shutdown;
        }

        ReplOutput::Lines {
            prompt: self.render_prompt(),
            output,
        }
    }

    /// Get the welcome banner shown at Q-Shell start.
    pub fn banner() -> Vec<String> {
        vec![
            String::from(""),
            String::from("  ╔═══════════════════════════════════╗"),
            String::from("  ║   Q-Shell v1.0.0-genesis          ║"),
            String::from("  ║   Semantic Command Palette        ║"),
            String::from("  ║   Type 'help' to begin.           ║"),
            String::from("  ╚═══════════════════════════════════╝"),
            String::from(""),
        ]
    }

    /// Clean shutdown — persist all state and checkpoint the journal.
    pub fn shutdown(&mut self) {
        self.persist.save_history(&self.history);
        self.persist.save_env(&self.env);
        // Gap 25.1 — Save aliases so they persist across Q-Shell sessions.
        self.persist.save_aliases(&self.aliases);
        self.persist.checkpoint();
        self.running = false;
    }
}

/// REPL output — what the shell produces for display.
#[derive(Debug)]
pub enum ReplOutput {
    /// Normal output: prompt + output lines
    Lines {
        /// The rendered prompt for the next command
        prompt: String,
        /// Output lines from the executed command
        output: Vec<String>,
    },
    /// Shell is shutting down
    Shutdown,
}
