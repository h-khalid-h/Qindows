//! # Q-Shell Pipeline System
//!
//! Implements Unix-style pipelines, I/O redirects, and subshells
//! for the Q-Shell command interpreter.
//!
//! Supported operators:
//! - `|`  — Pipe stdout of left to stdin of right
//! - `>`  — Redirect stdout to file (overwrite)
//! - `>>` — Redirect stdout to file (append)
//! - `<`  — Redirect file to stdin
//! - `2>` — Redirect stderr to file
//! - `&`  — Run in background
//! - `;`  — Sequential execution
//! - `&&` — Execute right only if left succeeds
//! - `||` — Execute right only if left fails

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Pipeline operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeOp {
    /// `|` — standard pipe (legacy byte stream)
    Pipe,
    /// `|&` — pipe both stdout and stderr
    PipeBoth,
    /// `~>` — Semantic Flow (passing QNode Object-IDs)
    Flow,
    /// `~>>` — Remote Mesh Flow (passing Object-IDs across Q-Fabric)
    RemoteFlow,
}

/// Redirect type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Redirect {
    /// `> file` — stdout to file (overwrite)
    StdoutOverwrite(String),
    /// `>> file` — stdout to file (append)
    StdoutAppend(String),
    /// `< file` — file to stdin
    StdinFrom(String),
    /// `2> file` — stderr to file
    StderrOverwrite(String),
    /// `2>> file` — stderr to file (append)
    StderrAppend(String),
    /// `&> file` — both stdout+stderr to file
    BothOverwrite(String),
}

/// Execution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecMode {
    /// Run and wait for completion
    Foreground,
    /// Run in background (`&`)
    Background,
}

/// Chain operator (between commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainOp {
    /// `;` — always run next
    Sequential,
    /// `&&` — run next only if previous succeeded (exit code 0)
    And,
    /// `||` — run next only if previous failed (exit code != 0)
    Or,
}

/// A single command in a pipeline.
#[derive(Debug, Clone)]
pub struct PipeCommand {
    /// Program name
    pub program: String,
    /// Arguments
    pub args: Vec<String>,
    /// I/O redirects
    pub redirects: Vec<Redirect>,
    /// Execution mode
    pub exec_mode: ExecMode,
}

impl PipeCommand {
    pub fn new(program: &str) -> Self {
        PipeCommand {
            program: String::from(program),
            args: Vec::new(),
            redirects: Vec::new(),
            exec_mode: ExecMode::Foreground,
        }
    }

    pub fn arg(mut self, arg: &str) -> Self {
        self.args.push(String::from(arg));
        self
    }

    pub fn redirect(mut self, r: Redirect) -> Self {
        self.redirects.push(r);
        self
    }
}

/// A pipeline — a chain of commands connected by pipes.
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Commands in the pipeline
    pub commands: Vec<PipeCommand>,
    /// Pipe operator between each pair
    pub pipe_ops: Vec<PipeOp>,
}

impl Pipeline {
    pub fn single(cmd: PipeCommand) -> Self {
        Pipeline { commands: alloc::vec![cmd], pipe_ops: Vec::new() }
    }

    pub fn pipe(mut self, op: PipeOp, cmd: PipeCommand) -> Self {
        self.pipe_ops.push(op);
        self.commands.push(cmd);
        self
    }
}

/// A command chain — pipelines joined by chain operators.
#[derive(Debug, Clone)]
pub struct CommandChain {
    pub pipelines: Vec<Pipeline>,
    pub chain_ops: Vec<ChainOp>,
}

impl CommandChain {
    pub fn single(pipeline: Pipeline) -> Self {
        CommandChain { pipelines: alloc::vec![pipeline], chain_ops: Vec::new() }
    }

    pub fn chain(mut self, op: ChainOp, pipeline: Pipeline) -> Self {
        self.chain_ops.push(op);
        self.pipelines.push(pipeline);
        self
    }
}

/// Pipeline execution result.
#[derive(Debug, Clone)]
pub struct PipeResult {
    /// Exit code of the last command
    pub exit_code: i32,
    /// Stdout capture (if piped to collector)
    pub stdout: Vec<u8>,
    /// Stderr capture
    pub stderr: Vec<u8>,
    /// Was this run in background?
    pub background: bool,
    /// Job ID (if background)
    pub job_id: Option<u64>,
}

/// Pipeline executor statistics.
#[derive(Debug, Clone, Default)]
pub struct PipelineStats {
    pub commands_executed: u64,
    pub pipes_created: u64,
    pub redirects_applied: u64,
    pub background_jobs: u64,
    pub chains_executed: u64,
}

/// The Pipeline Executor.
pub struct PipelineExecutor {
    /// Next job ID for background processes
    next_job_id: u64,
    /// Background jobs (job_id → command string)
    pub background_jobs: Vec<(u64, String)>,
    /// Statistics
    pub stats: PipelineStats,
}

impl PipelineExecutor {
    pub fn new() -> Self {
        PipelineExecutor {
            next_job_id: 1,
            background_jobs: Vec::new(),
            stats: PipelineStats::default(),
        }
    }

    /// Execute a pipeline.
    pub fn execute_pipeline(&mut self, pipeline: &Pipeline) -> PipeResult {
        let mut last_stdout: Vec<u8> = Vec::new();
        let mut last_exit = 0i32;

        for (i, cmd) in pipeline.commands.iter().enumerate() {
            // Apply input from previous command's stdout
            let stdin_data = if i > 0 { last_stdout.clone() } else { Vec::new() };

            let result = self.execute_command(cmd, &stdin_data);
            last_stdout = result.stdout;
            last_exit = result.exit_code;

            self.stats.commands_executed += 1;
            if i < pipeline.pipe_ops.len() {
                self.stats.pipes_created += 1;
            }
        }

        let bg = pipeline.commands.last()
            .map(|c| c.exec_mode == ExecMode::Background)
            .unwrap_or(false);

        let job_id = if bg {
            let jid = self.next_job_id;
            self.next_job_id += 1;
            let cmd_str = pipeline.commands.iter()
                .map(|c| c.program.as_str())
                .collect::<Vec<_>>()
                .join(" | ");
            self.background_jobs.push((jid, cmd_str));
            self.stats.background_jobs += 1;
            Some(jid)
        } else { None };

        PipeResult {
            exit_code: last_exit,
            stdout: last_stdout,
            stderr: Vec::new(),
            background: bg,
            job_id,
        }
    }

    /// Execute a command chain.
    pub fn execute_chain(&mut self, chain: &CommandChain) -> PipeResult {
        self.stats.chains_executed += 1;
        let mut last_result = PipeResult {
            exit_code: 0, stdout: Vec::new(), stderr: Vec::new(),
            background: false, job_id: None,
        };

        for (i, pipeline) in chain.pipelines.iter().enumerate() {
            if i > 0 {
                let op = chain.chain_ops[i - 1];
                match op {
                    ChainOp::Sequential => {} // Always run
                    ChainOp::And => {
                        if last_result.exit_code != 0 { continue; }
                    }
                    ChainOp::Or => {
                        if last_result.exit_code == 0 { continue; }
                    }
                }
            }
            last_result = self.execute_pipeline(pipeline);
        }

        last_result
    }

    /// Execute a single command (simplified — production dispatches to Q-Shell executor).
    fn execute_command(&mut self, cmd: &PipeCommand, _stdin: &[u8]) -> PipeResult {
        // Count redirects
        self.stats.redirects_applied += cmd.redirects.len() as u64;

        // In production: fork a fiber, set up redirects, execute via executor
        // Simplified: echo the command back as output
        let output = alloc::format!("[exec] {} {}\n",
            cmd.program,
            cmd.args.join(" ")
        );

        PipeResult {
            exit_code: 0,
            stdout: output.into_bytes(),
            stderr: Vec::new(),
            background: cmd.exec_mode == ExecMode::Background,
            job_id: None,
        }
    }
}
