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
    /// Command counter
    pub command_count: u64,
    /// Whether the shell is running
    pub running: bool,
}

impl ShellSession {
    /// Create a new interactive Q-Shell session.
    pub fn new() -> Self {
        ShellSession {
            readline: Readline::new(KeyMode::Emacs),
            prompt: PromptEngine::new(),
            context: PromptContext::default(),
            history: History::new(1),
            completion: CompletionEngine::new(),
            env: Environment::new(64),
            vars: VarManager::new(),
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

        // Record in history
        self.history.push(trimmed, "/", 0);
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

        // Parse the pipeline
        let pipeline = crate::parse(trimmed);

        // Execute each stage
        let mut output = Vec::new();
        for stage in &pipeline.stages {
            // Collect args as &str references
            let args: Vec<&str> = {
                let mut a = Vec::new();
                if let Some(ref sub) = stage.sub_command {
                    a.push(sub.as_str());
                }
                for arg in &stage.args {
                    a.push(arg.as_str());
                }
                a
            };

            let result = execute_builtin(&stage.command, &args);

            match result {
                CommandResult::Success(Some(t)) => output.push(t),
                CommandResult::Success(None) => {}
                CommandResult::Error(e) => output.push(format!("Error: {}", e)),
                CommandResult::List(items) => {
                    for item in items {
                        output.push(item);
                    }
                }
                CommandResult::Data(pairs) => {
                    for (k, v) in pairs {
                        output.push(format!("  {}: {}", k, v));
                    }
                }
                CommandResult::Exit => {
                    self.running = false;
                    output.push(String::from("Q-Shell session ended."));
                }
                CommandResult::Silent => {}
            }
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
