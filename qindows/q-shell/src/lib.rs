//! # Q-Shell — The Semantic Command Palette
//!
//! The first user-space application on Qindows.
//! Q-Shell doesn't pipe text — it pipes **Live Objects**.
//! Semantic flows replace regex. The `~>` operator is the reactive pipeline.
//!
//! This is "God Mode" for the OS.

#![no_std]

extern crate alloc;

pub mod alias;
pub mod executor;
pub mod env;
pub mod glob;
pub mod history;
pub mod pipe;
pub mod job_control;
pub mod scripting;
pub mod completion;
pub mod prompt;
pub mod pipeline;
pub mod qclip;

use alloc::string::String;
use alloc::vec::Vec;

/// A command token parsed from Q-Shell input.
#[derive(Debug, Clone)]
pub enum ShellToken {
    /// A command name (e.g., "prism", "mesh", "vault")
    Command(String),
    /// A subcommand (e.g., "find", "status", "export")
    SubCommand(String),
    /// A string argument
    StringArg(String),
    /// The flow operator `~>` — pipes objects between commands
    FlowOperator,
    /// A key-value flag (e.g., "--format:csv")
    Flag { key: String, value: String },
}

/// Parsed Q-Shell pipeline — a chain of commands connected by `~>`.
#[derive(Debug)]
pub struct Pipeline {
    pub stages: Vec<PipelineStage>,
}

/// A single stage in a Q-Shell pipeline.
#[derive(Debug)]
pub struct PipelineStage {
    pub command: String,
    pub sub_command: Option<String>,
    pub args: Vec<String>,
    pub flags: Vec<(String, String)>,
}

/// Parse a Q-Shell command string into a Pipeline.
///
/// Example input:
/// ```text
/// prism find "Invoices 2025" ~> q_analyze summarize --format:csv ~> vault export:desktop
/// ```
pub fn parse(input: &str) -> Pipeline {
    let stages_raw: Vec<&str> = input.split("~>").collect();
    let mut stages = Vec::new();

    for stage_str in stages_raw {
        let tokens: Vec<&str> = stage_str.split_whitespace().collect();
        if tokens.is_empty() {
            continue;
        }

        let command = String::from(tokens[0]);
        let mut sub_command = None;
        let mut args = Vec::new();
        let mut flags = Vec::new();

        for &token in &tokens[1..] {
            if token.starts_with("--") {
                // Flag: --key:value
                let parts: Vec<&str> = token[2..].splitn(2, ':').collect();
                let key = String::from(parts[0]);
                let value = if parts.len() > 1 {
                    String::from(parts[1])
                } else {
                    String::from("true")
                };
                flags.push((key, value));
            } else if token.starts_with('"') || token.starts_with('\'') {
                // Quoted argument
                args.push(String::from(token.trim_matches(|c| c == '"' || c == '\'')));
            } else if sub_command.is_none() && !token.contains(':') {
                sub_command = Some(String::from(token));
            } else {
                args.push(String::from(token));
            }
        }

        stages.push(PipelineStage {
            command,
            sub_command,
            args,
            flags,
        });
    }

    Pipeline { stages }
}

/// Built-in Q-Shell commands.
pub enum BuiltinCommand {
    /// `prism find <query>` — search the Prism Object Graph
    PrismFind,
    /// `mesh status` — show Global Mesh contribution stats
    MeshStatus,
    /// `silo list` — show all active hardware-isolated bubbles
    SiloList,
    /// `sentinel report` — display health scores for all Silos
    SentinelReport,
    /// `flow <obj> ~> <action>` — pipe an object into a capability
    Flow,
}
