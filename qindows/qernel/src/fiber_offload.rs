//! # Fiber Offload — Edge-Kernel "Scale to Cloud" (Phase 75)
//!
//! ARCHITECTURE.md §5.3 — Edge-Kernel: Process Offloading:
//! > "Right-click a process → 'Scale to Cloud'"
//! > "Qernel serializes the Fiber's state (registers + stack + memory objects)"
//! > "State transmitted to Q-Server via Q-Fabric"
//! > "UI stays local — only computation moves; user feels zero latency change"
//!
//! ## Architecture Guardian: Separation of Concerns
//! ```text
//! FiberOffloadEngine (this module)
//!   │  Concern: WHEN and HOW to serialize/restore Fiber state
//!   │  Does NOT: open sockets, move bytes (delegates to Q-Fabric)
//!   │  Does NOT: manage UI (delegates to Aether)
//!   │
//!   ├── FiberSnapshot: pure data — CPU registers + stack + memory map
//!   ├── OffloadRecord: lifecycle tracking (serializing/transmitting/running/recalling)
//!   └── FiberOffloadEngine: orchestration (decision + serialization coordination)
//!
//! Q-Fabric (qfabric.rs): actual QUIC transmission of snapshot bytes → Q-Server
//! Nexus (nexus.rs):      Q-Server discovery and compute auction bidding
//! Aether (aether.rs):    UI proxy continues rendering (Silo frozen locally)
//! ```
//!
//! ## How zero perceived latency is achieved
//! 1. Aether takes ownership of the Silo's **scene graph proxy** before serialization
//! 2. Windows still move, blur, resize during offload — Aether has the visuals
//! 3. User input is buffered locally and forwarded to Q-Server
//! 4. Q-Server returns only RESULTS (not screen updates) over Q-Fabric
//! 5. Results update the Prism object store → Aether re-renders from new state
//!
//! ## Q-Manifest Law Compliance
//! - **Law 6**: Serialized Fiber snapshot is encrypted with a session key
//!   Only the Q-Server's VERIFIED enclave (via TPM attestation) can decrypt it
//! - **Law 7**: Offload traffic goes through Q-Fabric, billed against NET_SEND token
//! - **Law 9**: Fiber addresses objects by OID — location doesn't change post-offload
//! - **Law 10**: If Q-Server disconnects mid-computation, Fiber is recalled and
//!   resumes locally from the last checkpoint — zero data loss

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Fiber CPU State ───────────────────────────────────────────────────────────

/// A snapshot of a Fiber's x86-64 CPU register state.
/// This is the minimum state needed to reconstitute execution on a remote CPU.
#[derive(Debug, Clone, Copy, Default)]
pub struct FiberCpuState {
    pub rax: u64, pub rbx: u64, pub rcx: u64, pub rdx: u64,
    pub rsi: u64, pub rdi: u64, pub rbp: u64, pub rsp: u64,
    pub r8:  u64, pub r9:  u64, pub r10: u64, pub r11: u64,
    pub r12: u64, pub r13: u64, pub r14: u64, pub r15: u64,
    pub rip: u64, // instruction pointer (where to resume)
    pub rflags: u64,
    pub cs: u16, pub ss: u16, // segment selectors
    pub fs_base: u64, // TLS base (for thread-local storage)
    pub gs_base: u64,
    /// x87/SSE/AVX state (simplified: just the MXCSR for now)
    pub mxcsr: u32,
    /// PKRU (memory protection key) state
    pub pkru: u32,
}

// ── Memory Region ─────────────────────────────────────────────────────────────

/// A serialized memory region from the Fiber's address space.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Virtual base address in the original Silo
    pub virt_base: u64,
    /// Size in bytes
    pub size_bytes: u64,
    /// Is this region read-only? (Law 2: Prism objects are RO)
    pub read_only: bool,
    /// Is this region replicated from Prism? (OID → skip transmission; Q-Server fetches directly)
    pub prism_oid: Option<[u8; 32]>,
    /// Raw bytes (None if prism_oid is set — fetched by Q-Server via Prism)
    pub bytes: Option<Vec<u8>>,
}

// ── Fiber Snapshot ────────────────────────────────────────────────────────────

/// A complete, serializable snapshot of a Fiber's execution state.
#[derive(Debug, Clone)]
pub struct FiberSnapshot {
    /// Silo ID of origin
    pub silo_id: u64,
    /// Fiber ID within the Silo
    pub fiber_id: u64,
    /// Snapshot version (increments on checkpoint)
    pub version: u32,
    /// Kernel tick when snapshot was taken
    pub taken_at: u64,
    /// CPU register state
    pub cpu: FiberCpuState,
    /// Memory regions (Prism-backed regions replaced by OID references)
    pub memory_regions: Vec<MemoryRegion>,
    /// Capability tokens that this Fiber holds (Laws 1 + 6)
    pub cap_tokens: Vec<u64>, // Simplified: CapToken IDs
    /// Encrypted session key for this offload (XOR placeholder; real = AES-256-GCM)
    pub session_key: [u8; 32],
    /// Total serialized bytes (excluding Prism-backed regions)
    pub serialized_bytes: u64,
}

impl FiberSnapshot {
    /// Compute the total transmission size accounting for Prism deduplication.
    pub fn transmission_bytes(&self) -> u64 {
        // Prism-backed regions are referenced by OID — Q-Server fetches them directly
        // Only non-Prism regions need transmission
        self.memory_regions.iter()
            .filter(|r| r.prism_oid.is_none())
            .map(|r| r.size_bytes)
            .sum::<u64>()
            + core::mem::size_of::<FiberCpuState>() as u64
    }

    /// How much was saved by Prism deduplication?
    pub fn prism_savings_bytes(&self) -> u64 {
        self.memory_regions.iter()
            .filter(|r| r.prism_oid.is_some())
            .map(|r| r.size_bytes)
            .sum()
    }
}

// ── Offload Lifecycle ─────────────────────────────────────────────────────────

/// The current phase of a Fiber offload operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffloadPhase {
    /// User initiated offload, preparing snapshot
    Serializing,
    /// Snapshot transmitted, waiting for Q-Server ack
    Transmitting,
    /// Q-Server acknowledged, Fiber executing remotely
    RunningRemote,
    /// Recall initiated (user/network trigger), waiting for state return
    Recalling,
    /// State returned and applied; Fiber running locally again
    Restored,
    /// Offload failed (network loss); Fiber recalled from checkpoint
    FailedRecalled,
}

/// An active offload record.
#[derive(Debug, Clone)]
pub struct OffloadRecord {
    pub silo_id: u64,
    pub fiber_id: u64,
    /// ID of the Q-Server handling computation (Nexus NodeId's first 8 bytes)
    pub server_node_id: u64,
    /// UNS URI of remote server: `qfa://node-id/compute`
    pub server_uri: String,
    /// Current phase
    pub phase: OffloadPhase,
    /// Snapshot used for this offload
    pub snapshot: FiberSnapshot,
    /// Tick when offload was requested
    pub started_at: u64,
    /// Tick when remote execution began
    pub remote_started_at: Option<u64>,
    /// Q-Credits cost (from Compute Auction, Nexus Phase V)
    pub credits_cost: u64,
    /// Results returned from Q-Server (Prism OIDs of output objects)
    pub result_oids: Vec<[u8; 32]>,
    /// Number of checkpoint syncs since offload started
    pub checkpoint_count: u32,
}

// ── Offload Statistics ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct OffloadStats {
    pub total_offloads: u64,
    pub successful_completions: u64,
    pub failed_recalls: u64,
    pub remote_execution_ticks: u64,
    pub bytes_transmitted: u64,
    pub prism_bytes_saved: u64,
    pub credits_spent: u64,
}

// ── Fiber Offload Engine ──────────────────────────────────────────────────────

/// The kernel Fiber serialization and offload coordinator.
pub struct FiberOffloadEngine {
    /// Active offload records: (silo_id, fiber_id) → record
    pub active: BTreeMap<(u64, u64), OffloadRecord>,
    /// Completed/failed history (last 32)
    pub history: Vec<OffloadRecord>,
    /// Maximum history entries
    pub max_history: usize,
    /// Global statistics
    pub stats: OffloadStats,
    /// Next offload ID (used as server_node_id placeholder)
    next_id: u64,
}

impl FiberOffloadEngine {
    pub fn new() -> Self {
        FiberOffloadEngine {
            active: BTreeMap::new(),
            history: Vec::new(),
            max_history: 32,
            stats: OffloadStats::default(),
            next_id: 1,
        }
    }

    /// Serialize a Fiber and initiate offload to a Q-Server.
    ///
    /// Called when user requests "Scale to Cloud" for a Silo.
    pub fn initiate_offload(
        &mut self,
        silo_id: u64,
        fiber_id: u64,
        server_node_id: u64,
        tick: u64,
    ) -> Result<(), &'static str> {
        if self.active.contains_key(&(silo_id, fiber_id)) {
            return Err("Fiber already offloaded");
        }

        crate::serial_println!(
            "[OFFLOAD] Initiating: Silo {} Fiber {} → Node {:016x}",
            silo_id, fiber_id, server_node_id
        );

        // Build synthetic snapshot (production: read from scheduler's fiber context)
        let snapshot = self.build_snapshot(silo_id, fiber_id, tick);
        let tx_bytes = snapshot.transmission_bytes();
        let prism_saved = snapshot.prism_savings_bytes();

        crate::serial_println!(
            "[OFFLOAD] Snapshot: {}B to transmit, {}B saved via Prism dedup",
            tx_bytes, prism_saved
        );

        let server_uri = {
            let mut s = "qfa://node-".to_string();
            s.push_str(&alloc::format!("{:016x}", server_node_id));
            s.push_str("/compute");
            s
        };

        let record = OffloadRecord {
            silo_id,
            fiber_id,
            server_node_id,
            server_uri: server_uri.clone(),
            phase: OffloadPhase::Serializing,
            snapshot,
            started_at: tick,
            remote_started_at: None,
            credits_cost: tx_bytes / 1024, // 1 credit per KiB (Compute Auction)
            result_oids: Vec::new(),
            checkpoint_count: 0,
        };

        self.active.insert((silo_id, fiber_id), record);
        self.stats.total_offloads += 1;
        self.stats.bytes_transmitted += tx_bytes;
        self.stats.prism_bytes_saved += prism_saved;

        crate::serial_println!(
            "[OFFLOAD] Transmitting to {} (cost: {} Q-Credits)...",
            server_uri, tx_bytes / 1024
        );
        Ok(())
    }

    /// Q-Fabric callback: Q-Server acknowledged receipt → Fiber is now running remote.
    pub fn on_remote_ack(&mut self, silo_id: u64, fiber_id: u64, tick: u64) -> bool {
        if let Some(rec) = self.active.get_mut(&(silo_id, fiber_id)) {
            rec.phase = OffloadPhase::RunningRemote;
            rec.remote_started_at = Some(tick);
            crate::serial_println!(
                "[OFFLOAD] Silo {} Fiber {} now RUNNING on remote Node {:016x}. UI proxy active.",
                silo_id, fiber_id, rec.server_node_id
            );
            true
        } else {
            false
        }
    }

    /// Initiate recall — bring the Fiber state back to local execution.
    pub fn recall(&mut self, silo_id: u64, fiber_id: u64, tick: u64) {
        if let Some(rec) = self.active.get_mut(&(silo_id, fiber_id)) {
            rec.phase = OffloadPhase::Recalling;
            crate::serial_println!(
                "[OFFLOAD] Recall initiated: Silo {} Fiber {} ← Node {:016x}",
                silo_id, fiber_id, rec.server_node_id
            );
            let _ = tick; // tick used for elapsed measurement in production
        }
    }

    /// Q-Fabric callback: restored state received from Q-Server.
    pub fn on_state_restored(
        &mut self,
        silo_id: u64,
        fiber_id: u64,
        result_oids: Vec<[u8; 32]>,
        tick: u64,
    ) -> bool {
        if let Some(mut rec) = self.active.remove(&(silo_id, fiber_id)) {
            let remote_ticks = rec.remote_started_at
                .map(|s| tick.saturating_sub(s))
                .unwrap_or(0);
            crate::serial_println!(
                "[OFFLOAD] Restored: Silo {} Fiber {} returned. Remote ran {}ticks. {} result objects.",
                silo_id, fiber_id, remote_ticks, result_oids.len()
            );
            self.stats.remote_execution_ticks += remote_ticks;
            self.stats.successful_completions += 1;
            self.stats.credits_spent += rec.credits_cost;
            rec.result_oids = result_oids;
            rec.phase = OffloadPhase::Restored;

            if self.history.len() >= self.max_history {
                self.history.remove(0);
            }
            self.history.push(rec);
            true
        } else {
            false
        }
    }

    /// Law 10: Q-Server disconnected — recall from last checkpoint.
    pub fn handle_server_failure(&mut self, silo_id: u64, fiber_id: u64, tick: u64) {
        if let Some(mut rec) = self.active.remove(&(silo_id, fiber_id)) {
            crate::serial_println!(
                "[OFFLOAD] Server FAILURE for Silo {} Fiber {}. Recalling from checkpoint {} (Law 10).",
                silo_id, fiber_id, rec.checkpoint_count
            );
            rec.phase = OffloadPhase::FailedRecalled;
            self.stats.failed_recalls += 1;
            if self.history.len() >= self.max_history { self.history.remove(0); }
            self.history.push(rec);
            let _ = tick;
        }
    }

    /// Periodic checkpoint sync of remote state back to local Prism (Law 10).
    pub fn checkpoint(&mut self, silo_id: u64, fiber_id: u64) {
        if let Some(rec) = self.active.get_mut(&(silo_id, fiber_id)) {
            if rec.phase == OffloadPhase::RunningRemote {
                rec.checkpoint_count += 1;
                crate::serial_println!(
                    "[OFFLOAD] Checkpoint #{} synced for Silo {} Fiber {}.",
                    rec.checkpoint_count, silo_id, fiber_id
                );
            }
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn build_snapshot(&mut self, silo_id: u64, fiber_id: u64, tick: u64) -> FiberSnapshot {
        // In production: read actual x86-64 register file from the scheduler Fiber struct
        // Here: build a plausible synthetic state
        let cpu = FiberCpuState {
            rsp: 0xFFFF_FFFF_0010_0000, // bottom of Silo stack
            rip: 0x0000_7FFF_1234_5678, // somewhere in the Silo's code segment
            rflags: 0x0202,              // IF set, reserved bit set
            ..Default::default()
        };

        // Simulate two memory regions: one Prism-backed (code), one heap
        let code_oid: [u8; 32] = {
            let mut o = [0u8; 32];
            o[0] = (silo_id & 0xFF) as u8;
            o[1] = (fiber_id & 0xFF) as u8;
            o
        };

        let memory_regions = alloc::vec![
            MemoryRegion {
                virt_base: 0x0000_0000_0040_0000,
                size_bytes: 512 * 1024, // 512KiB code segment
                read_only: true,
                prism_oid: Some(code_oid), // already in Prism → no transmission needed
                bytes: None,
            },
            MemoryRegion {
                virt_base: 0xFFFF_FFFF_0000_0000,
                size_bytes: 4 * 1024 * 1024, // 4MiB heap
                read_only: false,
                prism_oid: None,
                bytes: None, // production: copy heap pages here
            },
        ];

        let id = self.next_id;
        self.next_id += 1;

        FiberSnapshot {
            silo_id,
            fiber_id,
            version: 1,
            taken_at: tick,
            cpu,
            memory_regions,
            cap_tokens: alloc::vec![0x0001, 0x0002], // placeholder CapToken IDs
            session_key: {
                let mut k = [0u8; 32];
                k[0] = (id & 0xFF) as u8;
                k[1] = (tick & 0xFF) as u8;
                k
            },
            serialized_bytes: 4 * 1024 * 1024, // 4MiB (heap only; code is Prism-backed)
        }
    }
}
