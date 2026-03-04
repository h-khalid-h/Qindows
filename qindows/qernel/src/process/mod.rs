//! # Qernel Process Manager
//!
//! Manages the lifecycle of all processes (Silos) in the system.
//! Handles creation, scheduling priorities, resource accounting,
//! signal delivery, and process tree relationships.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Process ID.
pub type Pid = u64;

/// Process state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessState {
    /// Being created
    Creating,
    /// Ready to run
    Ready,
    /// Currently executing on a core
    Running,
    /// Waiting for I/O or event
    Blocked,
    /// Suspended by user or Sentinel
    Suspended,
    /// Zombie (exited but not reaped)
    Zombie,
    /// Terminated and cleaned up
    Dead,
}

/// Process priority levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    /// System-critical (Sentinel, Qernel services)
    Realtime = 0,
    /// Foreground app (user is actively using)
    Foreground = 1,
    /// Background app (visible but not focused)
    Background = 2,
    /// Low priority (background tasks, indexing)
    Low = 3,
    /// Idle (only runs when nothing else needs CPU)
    Idle = 4,
}

/// A signal that can be sent to a process.
#[derive(Debug, Clone, Copy)]
pub enum Signal {
    /// Terminate gracefully
    Terminate,
    /// Kill immediately
    Kill,
    /// Suspend (freeze)
    Suspend,
    /// Resume from suspend
    Resume,
    /// User-defined signal
    User(u32),
    /// Sentinel violation detected
    SentinelAlert,
}

/// Exit status of a process.
#[derive(Debug, Clone, Copy)]
pub enum ExitStatus {
    /// Normal exit with code
    Code(i32),
    /// Killed by signal
    Signal(Signal),
    /// Vaporized by Sentinel
    Vaporized,
    /// Out of memory
    Oom,
}

/// A process (Silo wrapper with lifecycle management).
#[derive(Debug, Clone)]
pub struct Process {
    /// Process ID
    pub pid: Pid,
    /// Parent process ID
    pub parent: Option<Pid>,
    /// Process name (app manifest name)
    pub name: String,
    /// Manifest app ID
    pub app_id: String,
    /// Current state
    pub state: ProcessState,
    /// Priority
    pub priority: Priority,
    /// Silo ID (sandbox)
    pub silo_id: u64,
    /// CPU time used (ns)
    pub cpu_time_ns: u64,
    /// Memory used (bytes)
    pub memory_used: u64,
    /// Memory limit (bytes)
    pub memory_limit: u64,
    /// Child process IDs
    pub children: Vec<Pid>,
    /// Exit status (if zombie/dead)
    pub exit_status: Option<ExitStatus>,
    /// Creation timestamp (ns since boot)
    pub created_at: u64,
    /// Core affinity (None = any core)
    pub core_affinity: Option<u8>,
    /// Number of fibers owned by this process
    pub fiber_count: u32,
    /// Open file descriptor count
    pub fd_count: u32,
}

/// The Process Manager.
pub struct ProcessManager {
    /// All processes
    pub processes: BTreeMap<Pid, Process>,
    /// Next PID
    next_pid: Pid,
    /// PID 1 (init process)
    pub init_pid: Pid,
    /// Statistics
    pub stats: ProcessStats,
}

/// Process manager statistics.
#[derive(Debug, Clone, Default)]
pub struct ProcessStats {
    pub total_created: u64,
    pub total_exited: u64,
    pub total_killed: u64,
    pub total_vaporized: u64,
    pub peak_processes: u64,
}

impl ProcessManager {
    pub fn new() -> Self {
        let mut pm = ProcessManager {
            processes: BTreeMap::new(),
            next_pid: 2, // PID 1 is init
            init_pid: 1,
            stats: ProcessStats::default(),
        };

        // Create PID 1 (init)
        pm.processes.insert(1, Process {
            pid: 1,
            parent: None,
            name: String::from("qinit"),
            app_id: String::from("com.qindows.init"),
            state: ProcessState::Running,
            priority: Priority::Realtime,
            silo_id: 0,
            cpu_time_ns: 0,
            memory_used: 0,
            memory_limit: u64::MAX,
            children: Vec::new(),
            exit_status: None,
            created_at: 0,
            core_affinity: Some(0),
            fiber_count: 1,
            fd_count: 3, // stdin, stdout, stderr
        });
        pm.stats.total_created = 1;

        pm
    }

    /// Create a new process.
    pub fn spawn(
        &mut self,
        parent: Pid,
        name: &str,
        app_id: &str,
        silo_id: u64,
        memory_limit: u64,
    ) -> Option<Pid> {
        // Verify parent exists
        if !self.processes.contains_key(&parent) {
            return None;
        }

        let pid = self.next_pid;
        self.next_pid += 1;

        let process = Process {
            pid,
            parent: Some(parent),
            name: String::from(name),
            app_id: String::from(app_id),
            state: ProcessState::Creating,
            priority: Priority::Background,
            silo_id,
            cpu_time_ns: 0,
            memory_used: 0,
            memory_limit,
            children: Vec::new(),
            exit_status: None,
            created_at: 0,
            core_affinity: None,
            fiber_count: 0,
            fd_count: 3,
        };

        self.processes.insert(pid, process);
        self.stats.total_created += 1;

        if self.processes.len() as u64 > self.stats.peak_processes {
            self.stats.peak_processes = self.processes.len() as u64;
        }

        // Add to parent's children list
        if let Some(parent_proc) = self.processes.get_mut(&parent) {
            parent_proc.children.push(pid);
        }

        Some(pid)
    }

    /// Transition a process to Ready state.
    pub fn make_ready(&mut self, pid: Pid) {
        if let Some(proc) = self.processes.get_mut(&pid) {
            if proc.state == ProcessState::Creating {
                proc.state = ProcessState::Ready;
            }
        }
    }

    /// Set process priority.
    pub fn set_priority(&mut self, pid: Pid, priority: Priority) {
        if let Some(proc) = self.processes.get_mut(&pid) {
            proc.priority = priority;
        }
    }

    /// Send a signal to a process.
    pub fn send_signal(&mut self, pid: Pid, signal: Signal) {
        if let Some(proc) = self.processes.get_mut(&pid) {
            match signal {
                Signal::Kill | Signal::Terminate => {
                    proc.state = ProcessState::Zombie;
                    proc.exit_status = Some(ExitStatus::Signal(signal));
                    self.stats.total_killed += 1;
                }
                Signal::Suspend => {
                    proc.state = ProcessState::Suspended;
                }
                Signal::Resume => {
                    if proc.state == ProcessState::Suspended {
                        proc.state = ProcessState::Ready;
                    }
                }
                Signal::SentinelAlert => {
                    proc.state = ProcessState::Zombie;
                    proc.exit_status = Some(ExitStatus::Vaporized);
                    self.stats.total_vaporized += 1;
                }
                Signal::User(_) => {
                    // Deliver to the process's signal handler
                }
            }
        }
    }

    /// Reap a zombie process (clean up resources).
    pub fn reap(&mut self, pid: Pid) -> Option<ExitStatus> {
        let exit = self.processes.get(&pid)?.exit_status;

        if let Some(proc) = self.processes.get(&pid) {
            if proc.state != ProcessState::Zombie { return None; }

            // Remove from parent's children
            if let Some(parent_pid) = proc.parent {
                if let Some(parent) = self.processes.get_mut(&parent_pid) {
                    parent.children.retain(|&c| c != pid);
                }
            }
        }

        self.processes.remove(&pid);
        self.stats.total_exited += 1;

        exit
    }

    /// Get all running processes.
    pub fn running(&self) -> Vec<&Process> {
        self.processes.values()
            .filter(|p| p.state == ProcessState::Running || p.state == ProcessState::Ready)
            .collect()
    }

    /// Get process count by state.
    pub fn count_by_state(&self, state: ProcessState) -> usize {
        self.processes.values().filter(|p| p.state == state).count()
    }

    /// Get total memory usage across all processes.
    pub fn total_memory(&self) -> u64 {
        self.processes.values().map(|p| p.memory_used).sum()
    }
}
