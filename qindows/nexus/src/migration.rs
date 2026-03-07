//! # Nexus Fiber Migration
//!
//! Transparent fiber migration across the Global Mesh.
//! A running computation can be paused on one machine,
//! serialized, transmitted via QUIC, and resumed on another —
//! seamlessly. "Your app follows you."

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

/// Fiber migration state — the serializable snapshot of a running fiber.
#[derive(Debug, Clone)]
pub struct FiberSnapshot {
    /// Source node ID
    pub source_node: [u8; 32],
    /// Destination node ID
    pub dest_node: [u8; 32],
    /// Fiber ID
    pub fiber_id: u64,
    /// Silo ID (sandbox context)
    pub silo_id: u64,
    /// Saved CPU registers (all 16 general-purpose)
    pub registers: RegisterState,
    /// Stack data (serialized)
    pub stack_data: Vec<u8>,
    /// Heap pages (serialized, delta-compressed)
    pub heap_pages: Vec<PageSnapshot>,
    /// Open Q-Ring channels (will be re-established at destination)
    pub channels: Vec<ChannelRef>,
    /// Capability tokens (must be validated at destination)
    pub capabilities: Vec<u64>,
    /// Migration timestamp
    pub timestamp: u64,
    /// Priority (higher = migrate first)
    pub priority: u8,
    /// HMAC signature (for Sentinel validation)
    pub signature: [u8; 32],
}

/// Saved register state.
#[derive(Debug, Clone, Default)]
pub struct RegisterState {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8: u64, pub r9: u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, pub rflags: u64,
    pub cr3: u64,
    // SSE/AVX state would be here too in production
}

/// A page of memory to be migrated.
#[derive(Debug, Clone)]
pub struct PageSnapshot {
    /// Virtual address
    pub vaddr: u64,
    /// Page data (4096 bytes)
    pub data: Vec<u8>,
    /// Page flags (read/write/execute/user)
    pub flags: u32,
    /// Is this a dirty page (modified since last migration)?
    pub dirty: bool,
}

/// Reference to a Q-Ring channel that needs re-establishment.
#[derive(Debug, Clone)]
pub struct ChannelRef {
    /// Channel ID
    pub channel_id: u64,
    /// Peer Silo ID
    pub peer_silo: u64,
    /// Direction
    pub direction: ChannelDirection,
}

#[derive(Debug, Clone, Copy)]
pub enum ChannelDirection {
    Send,
    Receive,
    Bidirectional,
}

/// Migration reasons
#[derive(Debug, Clone, Copy)]
pub enum MigrationReason {
    /// User moved to a different device
    UserFollow,
    /// Load balancing across mesh nodes
    LoadBalance,
    /// Current node is shutting down
    NodeDrain,
    /// Better hardware available (e.g., GPU node)
    ResourceAffinity,
    /// Thermal/power limit on current node
    ThermalThrottle,
}

/// Migration status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationStatus {
    /// Preparing snapshot
    Capturing,
    /// Compressing and serializing
    Serializing,
    /// Transmitting via QUIC
    Transmitting,
    /// Destination is restoring
    Restoring,
    /// Migration complete — fiber running on new node
    Complete,
    /// Migration failed — fiber still on source
    Failed,
    /// Migration cancelled
    Cancelled,
}

/// A migration job.
#[derive(Debug, Clone)]
pub struct MigrationJob {
    /// Job ID
    pub id: u64,
    /// Fiber snapshot
    pub snapshot: FiberSnapshot,
    /// Reason for migration
    pub reason: MigrationReason,
    /// Current status
    pub status: MigrationStatus,
    /// Progress (0-100)
    pub progress: u8,
    /// Elapsed time (ms)
    pub elapsed_ms: u64,
    /// Total data size (bytes)
    pub total_bytes: u64,
    /// Bytes transferred so far
    pub transferred_bytes: u64,
}

/// The Migration Manager.
pub struct MigrationManager {
    /// Active migration jobs
    pub jobs: Vec<MigrationJob>,
    /// Next job ID
    next_id: u64,
    /// Total successful migrations
    pub total_completed: u64,
    /// Total failed migrations
    pub total_failed: u64,
    /// Average migration time (ms)
    pub avg_time_ms: u64,
}

impl MigrationManager {
    pub fn new() -> Self {
        MigrationManager {
            jobs: Vec::new(),
            next_id: 1,
            total_completed: 0,
            total_failed: 0,
            avg_time_ms: 0,
        }
    }

    /// Initiate a fiber migration.
    pub fn start_migration(
        &mut self,
        snapshot: FiberSnapshot,
        reason: MigrationReason,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let total_bytes = snapshot.stack_data.len() as u64
            + snapshot.heap_pages.iter().map(|p| p.data.len() as u64).sum::<u64>();

        self.jobs.push(MigrationJob {
            id,
            snapshot,
            reason,
            status: MigrationStatus::Capturing,
            progress: 0,
            elapsed_ms: 0,
            total_bytes,
            transferred_bytes: 0,
        });

        id
    }

    /// Advance a migration job's status.
    pub fn advance(&mut self, job_id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            match job.status {
                MigrationStatus::Capturing => {
                    job.status = MigrationStatus::Serializing;
                    job.progress = 25;
                }
                MigrationStatus::Serializing => {
                    job.status = MigrationStatus::Transmitting;
                    job.progress = 50;
                }
                MigrationStatus::Transmitting => {
                    job.status = MigrationStatus::Restoring;
                    job.progress = 75;
                    job.transferred_bytes = job.total_bytes;
                }
                MigrationStatus::Restoring => {
                    job.status = MigrationStatus::Complete;
                    job.progress = 100;
                    self.total_completed += 1;
                }
                _ => {}
            }
        }
    }

    /// Cancel a migration.
    pub fn cancel(&mut self, job_id: u64) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == job_id) {
            if job.status != MigrationStatus::Complete {
                job.status = MigrationStatus::Cancelled;
            }
        }
    }

    /// Clean up completed/cancelled jobs.
    pub fn gc(&mut self) {
        self.jobs.retain(|j| {
            j.status != MigrationStatus::Complete
            && j.status != MigrationStatus::Failed
            && j.status != MigrationStatus::Cancelled
        });
    }

    /// Get active migration count.
    pub fn active_count(&self) -> usize {
        self.jobs.iter().filter(|j| {
            j.status != MigrationStatus::Complete
            && j.status != MigrationStatus::Failed
            && j.status != MigrationStatus::Cancelled
        }).count()
    }
}
