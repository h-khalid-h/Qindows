//! # Q-Shell Job Control
//!
//! Shell job control for background/foreground process management:
//!   - `&` — run command in background
//!   - `Ctrl+Z` — suspend foreground job
//!   - `fg %N` — resume job N in foreground
//!   - `bg %N` — resume job N in background
//!   - `jobs` — list all jobs
//!   - `kill %N` — terminate job N
//!
//! Each job tracks its pipeline, state, and resource usage.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── Job State ──────────────────────────────────────────────────────────────

/// Job execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    /// Running in foreground
    Foreground,
    /// Running in background
    Background,
    /// Suspended (Ctrl+Z)
    Stopped,
    /// Completed successfully
    Done,
    /// Terminated by signal
    Killed,
    /// Exited with error
    Failed(i32),
}

impl JobState {
    pub fn label(&self) -> &'static str {
        match self {
            JobState::Foreground => "Running (fg)",
            JobState::Background => "Running (bg)",
            JobState::Stopped    => "Stopped",
            JobState::Done       => "Done",
            JobState::Killed     => "Killed",
            JobState::Failed(_)  => "Failed",
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, JobState::Foreground | JobState::Background | JobState::Stopped)
    }
}

/// Signal to send to a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// SIGTERM — request graceful termination
    Term,
    /// SIGKILL — force kill
    Kill,
    /// SIGSTOP — suspend
    Stop,
    /// SIGCONT — resume
    Continue,
    /// SIGINT — interrupt (Ctrl+C)
    Interrupt,
    /// SIGHUP — hangup
    Hangup,
}

// ─── Job ────────────────────────────────────────────────────────────────────

/// A shell job (one or more piped commands).
#[derive(Debug, Clone)]
pub struct Job {
    /// Job number (1-based, as shown to user with %N)
    pub id: u32,
    /// Original command line
    pub command: String,
    /// Current state
    pub state: JobState,
    /// Process/fiber IDs for each command in the pipeline
    pub pids: Vec<u64>,
    /// Exit codes (populated when processes finish)
    pub exit_codes: Vec<Option<i32>>,
    /// When the job was started (ns since boot)
    pub started_at: u64,
    /// When the job finished (None if still running)
    pub finished_at: Option<u64>,
    /// CPU time consumed (ns)
    pub cpu_time_ns: u64,
    /// Is this the "current" job (shown with `+`)?
    pub is_current: bool,
    /// Is this the "previous" job (shown with `-`)?
    pub is_previous: bool,
    /// Notification shown to user?
    pub notified: bool,
}

impl Job {
    /// Check if the entire pipeline has exited.
    pub fn all_exited(&self) -> bool {
        self.exit_codes.iter().all(|c| c.is_some())
    }

    /// Get the final exit code (exit code of last process).
    pub fn final_exit_code(&self) -> Option<i32> {
        self.exit_codes.last().copied().flatten()
    }

    /// Elapsed time in milliseconds.
    pub fn elapsed_ms(&self, now: u64) -> u64 {
        let end = self.finished_at.unwrap_or(now);
        end.saturating_sub(self.started_at) / 1_000_000
    }

    /// Format for `jobs` command display.
    pub fn display(&self, now: u64) -> String {
        let marker = if self.is_current {
            "+"
        } else if self.is_previous {
            "-"
        } else {
            " "
        };

        let elapsed = self.elapsed_ms(now);
        let exit_info = match self.state {
            JobState::Failed(code) => alloc::format!(" (exit {})", code),
            JobState::Done => String::from(" (exit 0)"),
            _ => String::new(),
        };

        alloc::format!(
            "[{}]{} {} {}ms {}{}",
            self.id, marker, self.state.label(), elapsed, self.command, exit_info
        )
    }
}

// ─── Job Table ──────────────────────────────────────────────────────────────

/// Job table statistics.
#[derive(Debug, Clone, Default)]
pub struct JobStats {
    pub jobs_created: u64,
    pub jobs_completed: u64,
    pub jobs_killed: u64,
    pub jobs_suspended: u64,
    pub jobs_resumed: u64,
    pub fg_switches: u64,
    pub bg_switches: u64,
}

/// The Job Table.
pub struct JobTable {
    /// All jobs (active + recent completed)
    pub jobs: Vec<Job>,
    /// Next job ID
    next_id: u32,
    /// Maximum completed jobs to retain
    pub max_retained: usize,
    /// Statistics
    pub stats: JobStats,
}

impl JobTable {
    pub fn new() -> Self {
        JobTable {
            jobs: Vec::new(),
            next_id: 1,
            max_retained: 32,
            stats: JobStats::default(),
        }
    }

    /// Create a new job.
    pub fn create_job(
        &mut self,
        command: &str,
        pids: Vec<u64>,
        background: bool,
        now: u64,
    ) -> u32 {
        let id = self.next_id;
        self.next_id += 1;

        let state = if background {
            JobState::Background
        } else {
            JobState::Foreground
        };

        let exit_codes = pids.iter().map(|_| None).collect();

        // Update current/previous markers
        self.update_markers(id);

        let job = Job {
            id,
            command: String::from(command),
            state,
            pids,
            exit_codes,
            started_at: now,
            finished_at: None,
            cpu_time_ns: 0,
            is_current: true,
            is_previous: false,
            notified: false,
        };

        self.jobs.push(job);
        self.stats.jobs_created += 1;

        id
    }

    /// Update current/previous markers when a new job becomes current.
    fn update_markers(&mut self, _new_current: u32) {
        // Two-pass to avoid nested mutable borrow:
        // Pass 1: find the old current job ID (if active)
        let old_current_id = self.jobs.iter()
            .find(|j| j.is_current)
            .filter(|j| j.state.is_active())
            .map(|j| j.id);

        // Pass 2: clear all markers, then set the old current as previous
        for job in &mut self.jobs {
            job.is_current = false;
            job.is_previous = false;
        }
        if let Some(prev_id) = old_current_id {
            if let Some(job) = self.jobs.iter_mut().find(|j| j.id == prev_id) {
                job.is_previous = true;
            }
        }
    }

    /// Send a signal to a job.
    pub fn signal(&mut self, job_id: u32, signal: Signal, now: u64) -> Result<(), JobError> {
        let job = self.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(JobError::NotFound(job_id))?;

        if !job.state.is_active() {
            return Err(JobError::NotActive(job_id));
        }

        match signal {
            Signal::Stop => {
                job.state = JobState::Stopped;
                self.stats.jobs_suspended += 1;
            }
            Signal::Continue => {
                if job.state == JobState::Stopped {
                    job.state = JobState::Background;
                    self.stats.jobs_resumed += 1;
                }
            }
            Signal::Kill | Signal::Term | Signal::Interrupt => {
                job.state = JobState::Killed;
                job.finished_at = Some(now);
                self.stats.jobs_killed += 1;
            }
            Signal::Hangup => {
                job.state = JobState::Killed;
                job.finished_at = Some(now);
            }
        }

        Ok(())
    }

    /// Move a job to the foreground.
    pub fn foreground(&mut self, job_id: u32) -> Result<(), JobError> {
        let job = self.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(JobError::NotFound(job_id))?;

        if !job.state.is_active() {
            return Err(JobError::NotActive(job_id));
        }

        job.state = JobState::Foreground;
        job.is_current = true;
        self.stats.fg_switches += 1;

        Ok(())
    }

    /// Move a job to the background.
    pub fn background(&mut self, job_id: u32) -> Result<(), JobError> {
        let job = self.jobs.iter_mut()
            .find(|j| j.id == job_id)
            .ok_or(JobError::NotFound(job_id))?;

        if !job.state.is_active() {
            return Err(JobError::NotActive(job_id));
        }

        job.state = JobState::Background;
        self.stats.bg_switches += 1;

        Ok(())
    }

    /// Mark a job process as exited.
    pub fn process_exited(
        &mut self,
        pid: u64,
        exit_code: i32,
        now: u64,
    ) {
        for job in &mut self.jobs {
            if let Some(idx) = job.pids.iter().position(|&p| p == pid) {
                job.exit_codes[idx] = Some(exit_code);

                if job.all_exited() {
                    let code = job.final_exit_code().unwrap_or(0);
                    job.state = if code == 0 {
                        JobState::Done
                    } else {
                        JobState::Failed(code)
                    };
                    job.finished_at = Some(now);
                    self.stats.jobs_completed += 1;
                }
                return;
            }
        }
    }

    /// Get active (non-done) jobs.
    pub fn active_jobs(&self) -> Vec<&Job> {
        self.jobs.iter().filter(|j| j.state.is_active()).collect()
    }

    /// Get the current foreground job.
    pub fn foreground_job(&self) -> Option<&Job> {
        self.jobs.iter().find(|j| j.state == JobState::Foreground)
    }

    /// Get a job by ID.
    pub fn get(&self, job_id: u32) -> Option<&Job> {
        self.jobs.iter().find(|j| j.id == job_id)
    }

    /// Get recently completed jobs that haven't been notified yet.
    pub fn pending_notifications(&mut self) -> Vec<String> {
        let mut notifications = Vec::new();
        for job in &mut self.jobs {
            if !job.notified && !job.state.is_active() {
                notifications.push(alloc::format!(
                    "[{}] {} {}",
                    job.id, job.state.label(), job.command
                ));
                job.notified = true;
            }
        }
        notifications
    }

    /// List all jobs for the `jobs` command.
    pub fn list(&self, now: u64) -> Vec<String> {
        self.jobs.iter()
            .filter(|j| j.state.is_active() || !j.notified)
            .map(|j| j.display(now))
            .collect()
    }

    /// Clean up old completed jobs.
    pub fn cleanup(&mut self) {
        let active_count = self.jobs.iter().filter(|j| j.state.is_active()).count();
        let completed: Vec<usize> = self.jobs.iter().enumerate()
            .filter(|(_, j)| !j.state.is_active() && j.notified)
            .map(|(i, _)| i)
            .collect();

        // Keep only max_retained completed jobs
        if completed.len() > self.max_retained {
            let to_remove = completed.len() - self.max_retained;
            let remove_indices: Vec<usize> = completed[..to_remove].to_vec();
            // Remove in reverse order to preserve indices
            for &idx in remove_indices.iter().rev() {
                self.jobs.remove(idx);
            }
        }
    }
}

/// Job control errors.
#[derive(Debug, Clone)]
pub enum JobError {
    /// Job not found
    NotFound(u32),
    /// Job not active (already finished)
    NotActive(u32),
    /// No foreground job
    NoForegroundJob,
    /// Cannot suspend (not in foreground)
    NotForeground(u32),
}
