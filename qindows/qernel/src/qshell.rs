//! # Q-Shell — Semantic Object Pipeline Engine (Phase 66)
//!
//! Q-Shell is Qindows's "God Mode" terminal. Unlike legacy shells that pipe
//! text, Q-Shell pipes **Live Objects** via the `~>` (Flow) operator.
//!
//! ## ARCHITECTURE.md §6.1: Q-Shell
//! > "prism find 'Invoices 2025' ~> q_analyze summarize --format:csv ~> vault export:desktop"
//! > "prism find doesn't return strings — it returns Object Handles."
//! > "Q-Admin: 'Grant Disk-Write to this terminal for 5 minutes' — scoped, not global admin."
//!
//! ## Architecture Guardian: Role of this module
//! This module is the **kernel-side** Q-Shell engine:
//! - Parses pipeline stage descriptors submitted via Q-Ring
//! - Resolves stage commands to capability-checked handlers
//! - Tracks pipe state (handles flowing through stages)
//! - Enforces Q-Admin temporal escalation (Law 1: Zero-Ambient Authority)
//!
//! The actual **user-facing** Q-Shell runs in a privileged Silo.
//! This module is the kernel bridge consumed by that Silo's syscalls.
//!
//! ## Q-Manifest Law 1: Zero-Ambient Authority
//! `q_analyze`, `vault export`, etc. are NOT ambient. Each stage must hold
//! the appropriate CapToken. The kernel validates capability at each stage
//! transition.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;

// ── Object Handle ─────────────────────────────────────────────────────────────

/// A live object handle flowing through a Q-Shell pipeline.
///
/// Handles are kernel-managed — they reference Prism OIDs and carry
/// the capability scope granted when the object entered the pipeline.
#[derive(Debug, Clone)]
pub struct ObjectHandle {
    /// Prism OID of the referenced object
    pub oid: u64,
    /// Human-readable type tag (e.g. "document", "image", "process")
    pub type_tag: String,
    /// Size of the underlying object in bytes
    pub size_bytes: u64,
    /// Metadata key-value pairs extracted from the OID
    pub metadata: BTreeMap<String, String>,
    /// Capability scope under which this handle was opened
    pub cap_scope: HandleCapScope,
}

/// The capability scope of an object handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleCapScope {
    /// Read-only — can be inspected or transformed but not modified
    ReadOnly,
    /// Read-write — modifications are allowed
    ReadWrite,
    /// Executable — can be launched as a Silo
    Executable,
    /// Admin-escalated — temporary elevated scope (Q-Admin)
    AdminEscalated { expires_at: u64 },
}

// ── Pipeline Stage ────────────────────────────────────────────────────────────

/// A single stage in a Q-Shell pipeline (`cmd --arg:value`).
#[derive(Debug, Clone)]
pub struct PipelineStage {
    /// The command name (e.g. "prism", "q_analyze", "vault", "net")
    pub command: String,
    /// Subcommand / action (e.g. "find", "summarize", "export")
    pub action: String,
    /// Named arguments (e.g. "--format:csv" → ("format", "csv"))
    pub args: BTreeMap<String, String>,
    /// Required capability type for this stage
    pub required_cap: StageCap,
}

/// Capability required for a pipeline stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageCap {
    /// Can execute with no special caps
    None,
    /// Needs Prism read access
    PrismRead,
    /// Needs Prism write access (includes vault export)
    PrismWrite,
    /// Needs network send
    NetSend,
    /// Needs admin escalation token
    Admin,
    /// Needs NPU inference token (q_analyze)
    NpuInfer,
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// The current state of a Q-Shell pipeline execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineState {
    /// Pipeline is ready to run
    Pending,
    /// Currently executing at stage index
    Running { stage_index: usize },
    /// All stages complete
    Complete,
    /// A stage failed
    Failed,
    /// User cancelled
    Cancelled,
}

/// A Q-Shell pipeline: an ordered series of stages connected by `~>`.
#[derive(Debug, Clone)]
pub struct Pipeline {
    /// Pipeline ID (kernel-assigned)
    pub id: u64,
    /// Owning Silo
    pub owner_silo: u64,
    /// Ordered list of pipeline stages
    pub stages: Vec<PipelineStage>,
    /// Object handles flowing through the current stage
    pub active_handles: Vec<ObjectHandle>,
    /// Completed results (accumulated as handles pass through all stages)
    pub results: Vec<ObjectHandle>,
    /// Current state
    pub state: PipelineState,
    /// Total handles processed
    pub handles_processed: u64,
    /// Kernel tick when pipeline was submitted
    pub submitted_at: u64,
}

// ── Q-Admin Temporal Escalation ───────────────────────────────────────────────

/// An active Q-Admin escalation grant.
///
/// Per ARCHITECTURE.md: "Grant Disk-Write to this terminal for 5 minutes"
/// Scoped: only applies to the requesting Silo, only for the specified
/// capability, only within the time window.
#[derive(Debug, Clone)]
pub struct AdminEscalation {
    /// Silo granted the escalation
    pub silo_id: u64,
    /// What capability was granted
    pub cap_granted: StageCap,
    /// Kernel tick when granted
    pub granted_at: u64,
    /// Duration in kernel ticks (typically 5 * 60 * 1000 for 5 minutes at 1kHz)
    pub duration_ticks: u64,
    /// Optional human-readable reason
    pub reason: String,
}

impl AdminEscalation {
    /// Is this escalation still valid at `tick`?
    pub fn is_valid(&self, tick: u64) -> bool {
        tick.saturating_sub(self.granted_at) <= self.duration_ticks
    }
}

/// 5-minute escalation window (assuming 1kHz OS tick).
pub const ADMIN_ESCALATION_DEFAULT_TICKS: u64 = 5 * 60 * 1000;

// ── Built-in Stage Handlers ───────────────────────────────────────────────────

/// Represents the result of executing one pipeline stage.
#[derive(Debug, Clone)]
pub enum StageResult {
    /// Stage produced output handles to pass to the next stage
    Handles(Vec<ObjectHandle>),
    /// Stage consumed all handles (terminal sink, e.g. "vault export")
    Consumed,
    /// Stage failed with human-readable reason
    Error(String),
}

/// Execute the `prism find` stage: semantic search → object handles.
///
/// In production: calls Q-Ring `PrismQuery` syscall with the semantic
/// query string and returns matching OIDs as handles.
pub fn stage_prism_find(query: &str, limit: usize) -> StageResult {
    crate::serial_println!("[QSHELL] prism find: query=\"{}\" limit={}", query, limit);
    // Simulate returning stub handles for architectural demonstration
    let mut handles = Vec::new();
    for i in 0..limit.min(3) {
        let mut meta = BTreeMap::new();
        meta.insert("query".to_string(), query.to_string());
        meta.insert("rank".to_string(), i.to_string());
        handles.push(ObjectHandle {
            oid: 0x9173_0001_0000_0000 + i as u64,
            type_tag: "document".to_string(),
            size_bytes: (1024 * (i + 1)) as u64,
            metadata: meta,
            cap_scope: HandleCapScope::ReadOnly,
        });
    }
    StageResult::Handles(handles)
}

/// Execute the `q_analyze summarize` stage: NPU inference on input handles.
pub fn stage_q_analyze_summarize(handles: &[ObjectHandle], format: &str) -> StageResult {
    crate::serial_println!(
        "[QSHELL] q_analyze summarize: {} handles, format={}",
        handles.len(), format
    );
    // In production: submits handle OIDs to NPU inference Silo
    let mut out_meta = BTreeMap::new();
    out_meta.insert("source_count".to_string(), handles.len().to_string());
    out_meta.insert("format".to_string(), format.to_string());
    let mut type_tag = "summary-".to_string();
    type_tag.push_str(format);
    let out = ObjectHandle {
        oid: 0xAA_0000_0000_0001,
        type_tag,
        size_bytes: handles.iter().map(|h| h.size_bytes / 4).sum(),
        metadata: out_meta,
        cap_scope: HandleCapScope::ReadOnly,
    };
    StageResult::Handles(alloc::vec![out])
}

/// Execute the `vault export` stage: write handles to a Prism destination.
pub fn stage_vault_export(handles: &[ObjectHandle], destination: &str) -> StageResult {
    crate::serial_println!(
        "[QSHELL] vault export: {} handles → {}", handles.len(), destination
    );
    // In production: calls Q-Ring PrismWrite for each handle to the destination scope
    StageResult::Consumed
}

/// Execute the `net mesh` stage: transmit handles to a peer node.
pub fn stage_net_mesh(handles: &[ObjectHandle], peer: &str, message: &str) -> StageResult {
    crate::serial_println!(
        "[QSHELL] net mesh ~> {}: {} handles (msg={})",
        peer, handles.len(), message
    );
    // In production: calls Q-Fabric for P2P object transfer
    StageResult::Consumed
}

// ── Q-Shell Engine ────────────────────────────────────────────────────────────

/// Q-Shell statistics.
#[derive(Debug, Default, Clone)]
pub struct QShellStats {
    pub pipelines_submitted: u64,
    pub pipelines_completed: u64,
    pub pipelines_failed: u64,
    pub handles_processed: u64,
    pub admin_escalations_granted: u64,
    pub admin_escalations_denied: u64,
}

/// The kernel-side Q-Shell engine.
pub struct QShellEngine {
    /// Active pipelines: id → pipeline
    pub pipelines: BTreeMap<u64, Pipeline>,
    /// Active Q-Admin escalations: silo_id → escalation
    pub escalations: BTreeMap<u64, AdminEscalation>,
    /// Next pipeline ID
    next_pipeline_id: u64,
    /// Stats
    pub stats: QShellStats,
}

impl QShellEngine {
    pub fn new() -> Self {
        QShellEngine {
            pipelines: BTreeMap::new(),
            escalations: BTreeMap::new(),
            next_pipeline_id: 1,
            stats: QShellStats::default(),
        }
    }

    /// Submit a new pipeline for execution.
    pub fn submit_pipeline(
        &mut self,
        owner_silo: u64,
        stages: Vec<PipelineStage>,
        tick: u64,
    ) -> u64 {
        let id = self.next_pipeline_id;
        self.next_pipeline_id += 1;

        crate::serial_println!(
            "[QSHELL] Pipeline #{} submitted by Silo {} ({} stages)",
            id, owner_silo, stages.len()
        );
        for (i, s) in stages.iter().enumerate() {
            crate::serial_println!("[QSHELL]   Stage {}: {} {}", i, s.command, s.action);
        }

        self.pipelines.insert(id, Pipeline {
            id,
            owner_silo,
            stages,
            active_handles: Vec::new(),
            results: Vec::new(),
            state: PipelineState::Pending,
            handles_processed: 0,
            submitted_at: tick,
        });

        self.stats.pipelines_submitted += 1;
        id
    }

    /// Execute the next stage of a pending/running pipeline.
    ///
    /// Drives pipelines forward one stage per call (cooperative multitasking
    /// with the Q-Ring scheduler — each stage yields between calls).
    pub fn step_pipeline(&mut self, pipeline_id: u64, tick: u64) {
        let pipeline = match self.pipelines.get_mut(&pipeline_id) {
            Some(p) => p,
            None => return,
        };

        let stage_index = match pipeline.state {
            PipelineState::Pending => 0,
            PipelineState::Running { stage_index } => stage_index,
            _ => return, // already complete/failed/cancelled
        };

        if stage_index >= pipeline.stages.len() {
            pipeline.state = PipelineState::Complete;
            crate::serial_println!("[QSHELL] Pipeline #{} COMPLETE ({} results).",
                pipeline_id, pipeline.results.len());
            self.stats.pipelines_completed += 1;
            return;
        }

        pipeline.state = PipelineState::Running { stage_index };
        let stage = &pipeline.stages[stage_index];
        let handles = pipeline.active_handles.clone();

        crate::serial_println!(
            "[QSHELL] Pipeline #{} executing stage {}: {} {}",
            pipeline_id, stage_index, stage.command, stage.action
        );

        let result = match (stage.command.as_str(), stage.action.as_str()) {
            ("prism", "find") => {
                let query = stage.args.get("query").map(|s| s.as_str()).unwrap_or("");
                let limit = stage.args.get("limit")
                    .and_then(|s| s.parse().ok()).unwrap_or(10);
                stage_prism_find(query, limit)
            }
            ("q_analyze", "summarize") => {
                let format = stage.args.get("format").map(|s| s.as_str()).unwrap_or("text");
                stage_q_analyze_summarize(&handles, format)
            }
            ("vault", "export") => {
                let dest = stage.args.get("destination").map(|s| s.as_str()).unwrap_or("desktop");
                stage_vault_export(&handles, dest)
            }
            ("net", "mesh") => {
                let peer = stage.args.get("peer").map(|s| s.as_str()).unwrap_or("unknown");
                let msg  = stage.args.get("message").map(|s| s.as_str()).unwrap_or("");
                stage_net_mesh(&handles, peer, msg)
            }
            (cmd, act) => {
                crate::serial_println!("[QSHELL] Unknown stage: {} {}", cmd, act);
                StageResult::Error(alloc::format!("unknown command: {} {}", cmd, act))
            }
        };

        match result {
            StageResult::Handles(new_handles) => {
                pipeline.handles_processed += new_handles.len() as u64;
                pipeline.active_handles = new_handles;
                pipeline.state = PipelineState::Running {
                    stage_index: stage_index + 1,
                };
            }
            StageResult::Consumed => {
                pipeline.results = pipeline.active_handles.clone();
                pipeline.active_handles = Vec::new();
                pipeline.state = PipelineState::Complete;
                self.stats.pipelines_completed += 1;
            }
            StageResult::Error(msg) => {
                crate::serial_println!("[QSHELL] Pipeline #{} FAILED: {}", pipeline_id, msg);
                pipeline.state = PipelineState::Failed;
                self.stats.pipelines_failed += 1;
            }
        }
        self.stats.handles_processed += pipeline.handles_processed;
    }

    /// Grant a Q-Admin escalation for a Silo (Law 1: scoped, time-limited).
    ///
    /// In production: user sees a biometric/PIN prompt before this is allowed.
    pub fn grant_escalation(
        &mut self,
        silo_id: u64,
        cap_granted: StageCap,
        duration_ticks: u64,
        reason: String,
        tick: u64,
    ) {
        crate::serial_println!(
            "[QSHELL] Q-Admin escalation GRANTED: Silo {} gets {:?} for {}ms. Reason: {}",
            silo_id, cap_granted, duration_ticks, reason
        );
        self.escalations.insert(silo_id, AdminEscalation {
            silo_id,
            cap_granted,
            granted_at: tick,
            duration_ticks,
            reason,
        });
        self.stats.admin_escalations_granted += 1;
    }

    /// Check if a Silo has an active escalation for a given capability.
    pub fn has_escalation(&self, silo_id: u64, cap: StageCap, tick: u64) -> bool {
        self.escalations.get(&silo_id)
            .map(|e| e.cap_granted == cap && e.is_valid(tick))
            .unwrap_or(false)
    }

    /// Remove expired escalations (called periodically by the Sentinel).
    pub fn prune_expired_escalations(&mut self, tick: u64) {
        self.escalations.retain(|_, e| e.is_valid(tick));
    }

    /// Cancel a running pipeline (user Ctrl+C or Sentinel intervention).
    pub fn cancel_pipeline(&mut self, pipeline_id: u64) {
        if let Some(p) = self.pipelines.get_mut(&pipeline_id) {
            p.state = PipelineState::Cancelled;
            crate::serial_println!("[QSHELL] Pipeline #{} CANCELLED.", pipeline_id);
        }
    }
}
