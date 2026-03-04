//! # Q-Shell Pipe Executor
//!
//! Implements Unix-style command pipelines for Q-Shell.
//! Supports chaining commands with `|`, `>`, `>>`, and `<`
//! redirection, plus background execution with `&`.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// A single command in a pipeline.
#[derive(Debug, Clone)]
pub struct PipeCommand {
    /// Command name
    pub name: String,
    /// Arguments
    pub args: Vec<String>,
    /// Environment overrides
    pub env: Vec<(String, String)>,
}

/// Redirection types.
#[derive(Debug, Clone)]
pub enum Redirect {
    /// Redirect stdin from file: `< file`
    StdinFrom(String),
    /// Redirect stdout to file (overwrite): `> file`
    StdoutTo(String),
    /// Redirect stdout to file (append): `>> file`
    StdoutAppend(String),
    /// Redirect stderr to file: `2> file`
    StderrTo(String),
    /// Redirect stderr to stdout: `2>&1`
    StderrToStdout,
}

/// A full pipeline (chain of commands connected by pipes).
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Commands in order
    pub commands: Vec<PipeCommand>,
    /// Redirections
    pub redirects: Vec<Redirect>,
    /// Run in background?
    pub background: bool,
}

/// Pipeline execution result.
#[derive(Debug, Clone)]
pub struct PipelineResult {
    /// Exit code of the last command
    pub exit_code: i32,
    /// Combined stdout
    pub stdout: Vec<u8>,
    /// Combined stderr
    pub stderr: Vec<u8>,
    /// Per-command exit codes
    pub exit_codes: Vec<i32>,
    /// Execution time (ms)
    pub duration_ms: u64,
}

/// Pipeline execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineState {
    /// Not started
    Pending,
    /// Currently executing
    Running,
    /// Completed
    Done,
    /// Failed to start
    Failed,
    /// Killed
    Killed,
}

/// A running pipeline job.
#[derive(Debug, Clone)]
pub struct Job {
    /// Job ID
    pub id: u64,
    /// The pipeline
    pub pipeline: Pipeline,
    /// Current state
    pub state: PipelineState,
    /// Result (when done)
    pub result: Option<PipelineResult>,
    /// Start timestamp
    pub started_at: u64,
}

/// Parse a command line string into a Pipeline.
pub fn parse_pipeline(input: &str) -> Pipeline {
    let input = input.trim();

    // Check for background execution
    let (input, background) = if input.ends_with('&') {
        (&input[..input.len() - 1], true)
    } else {
        (input, false)
    };

    let mut commands = Vec::new();
    let mut redirects = Vec::new();

    // Split by pipe
    let pipe_segments: Vec<&str> = split_respecting_quotes(input, '|');

    for segment in pipe_segments {
        let segment = segment.trim();
        if segment.is_empty() { continue; }

        // Parse redirections from this segment
        let (cmd_part, segment_redirects) = extract_redirects(segment);
        redirects.extend(segment_redirects);

        // Parse command and arguments
        let tokens = tokenize_command(cmd_part.trim());
        if tokens.is_empty() { continue; }

        commands.push(PipeCommand {
            name: tokens[0].clone(),
            args: tokens[1..].to_vec(),
            env: Vec::new(),
        });
    }

    Pipeline { commands, redirects, background }
}

/// Split a string by a delimiter, respecting quoted strings.
fn split_respecting_quotes(input: &str, delim: char) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut start = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;

    for (i, ch) in input.char_indices() {
        match ch {
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            c if c == delim && !in_single_quote && !in_double_quote => {
                segments.push(&input[start..i]);
                start = i + ch.len_utf8();
            }
            _ => {}
        }
    }
    segments.push(&input[start..]);
    segments
}

/// Extract redirections from a command segment.
fn extract_redirects(segment: &str) -> (String, Vec<Redirect>) {
    let mut cmd = String::new();
    let mut redirects = Vec::new();
    let chars: Vec<char> = segment.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            '<' => {
                i += 1;
                // Skip whitespace
                while i < chars.len() && chars[i] == ' ' { i += 1; }
                let file = collect_word(&chars, &mut i);
                redirects.push(Redirect::StdinFrom(file));
            }
            '>' => {
                i += 1;
                if i < chars.len() && chars[i] == '>' {
                    i += 1;
                    while i < chars.len() && chars[i] == ' ' { i += 1; }
                    let file = collect_word(&chars, &mut i);
                    redirects.push(Redirect::StdoutAppend(file));
                } else {
                    while i < chars.len() && chars[i] == ' ' { i += 1; }
                    let file = collect_word(&chars, &mut i);
                    redirects.push(Redirect::StdoutTo(file));
                }
            }
            '2' if i + 1 < chars.len() && chars[i + 1] == '>' => {
                i += 2;
                if i + 1 < chars.len() && chars[i] == '&' && chars[i + 1] == '1' {
                    redirects.push(Redirect::StderrToStdout);
                    i += 2;
                } else {
                    while i < chars.len() && chars[i] == ' ' { i += 1; }
                    let file = collect_word(&chars, &mut i);
                    redirects.push(Redirect::StderrTo(file));
                }
            }
            _ => {
                cmd.push(chars[i]);
                i += 1;
            }
        }
    }

    (cmd, redirects)
}

/// Collect a word (non-whitespace sequence) from character array.
fn collect_word(chars: &[char], pos: &mut usize) -> String {
    let mut word = String::new();
    let mut in_quote = false;

    while *pos < chars.len() {
        let ch = chars[*pos];
        match ch {
            '"' | '\'' => {
                in_quote = !in_quote;
                *pos += 1;
            }
            ' ' | '\t' if !in_quote => break,
            _ => {
                word.push(ch);
                *pos += 1;
            }
        }
    }
    word
}

/// Tokenize a command string into individual tokens.
fn tokenize_command(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escape_next = false;

    for ch in input.chars() {
        if escape_next {
            current.push(ch);
            escape_next = false;
            continue;
        }

        match ch {
            '\\' if !in_single => escape_next = true,
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            ' ' | '\t' if !in_single && !in_double => {
                if !current.is_empty() {
                    tokens.push(current.clone());
                    current.clear();
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

/// The Pipe Executor.
pub struct PipeExecutor {
    /// Active jobs
    pub jobs: Vec<Job>,
    /// Next job ID
    next_id: u64,
    /// Stats
    pub pipelines_executed: u64,
    pub pipelines_failed: u64,
}

impl PipeExecutor {
    pub fn new() -> Self {
        PipeExecutor {
            jobs: Vec::new(),
            next_id: 1,
            pipelines_executed: 0,
            pipelines_failed: 0,
        }
    }

    /// Execute a pipeline string.
    pub fn execute(&mut self, input: &str, now: u64) -> u64 {
        let pipeline = parse_pipeline(input);
        let id = self.next_id;
        self.next_id += 1;

        let job = Job {
            id,
            pipeline,
            state: PipelineState::Running,
            result: None,
            started_at: now,
        };

        self.jobs.push(job);
        self.pipelines_executed += 1;
        id
    }

    /// Simulate completing a job with output.
    pub fn complete_job(&mut self, job_id: u64, stdout: Vec<u8>, exit_code: i32, now: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            let duration = now.saturating_sub(job.started_at);
            let cmd_count = job.pipeline.commands.len();

            job.result = Some(PipelineResult {
                exit_code,
                stdout,
                stderr: Vec::new(),
                exit_codes: alloc::vec![exit_code; cmd_count],
                duration_ms: duration / 1_000_000,
            });
            job.state = if exit_code == 0 { PipelineState::Done } else { PipelineState::Failed };

            if exit_code != 0 {
                self.pipelines_failed += 1;
            }
        }
    }

    /// Kill a background job.
    pub fn kill_job(&mut self, job_id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            job.state = PipelineState::Killed;
        }
    }

    /// Get all background jobs.
    pub fn background_jobs(&self) -> Vec<&Job> {
        self.jobs.iter()
            .filter(|j| j.pipeline.background && j.state == PipelineState::Running)
            .collect()
    }

    /// Clean up completed/killed jobs.
    pub fn cleanup(&mut self) {
        self.jobs.retain(|j| j.state == PipelineState::Running || j.state == PipelineState::Pending);
    }
}
