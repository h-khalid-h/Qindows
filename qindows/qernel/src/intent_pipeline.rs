//! # Intent Execution Pipeline (Phase 106)
//!
//! ## Architecture Guardian: The Gap
//! `synapse_bridge.rs` (Phase 102) delivers an `IntentResult` to the kernel.
//! `intent_router.rs` (Phase 79) converts `IntentEvent` → `DispatchAction`.
//!
//! **The missing link**: no code actually *executed* the `DispatchAction`.
//! When `IntentRouter::dispatch()` returns `QShellExecute { silo_id, command }`,
//! nothing sent that command to Q-Shell. When it returned `QViewNavigate`, nothing
//! updated the browser tab. This module closes that gap.
//!
//! ## Pipeline
//! ```text
//! synapse_bridge::poll_result()
//!     → IntentResult { category, confidence, gate_confirmed }
//!     → IntentPipeline::submit()
//!         → build IntentEvent (add context from kstate_ext)
//!         → IntentRouter::dispatch() → DispatchAction
//!         → IntentPipeline::execute_action() → Q-Ring SQ entry
//!             → Q-Shell / Aether / Browser / Sentinel
//! ```
//!
//! ## Law 1 (Zero-Ambient Authority)
//! The pipeline never acts unless `gate_confirmed == true` in IntentResult.
//! Without ThoughtGate confirmation, the action is downgraded to NoOp.

extern crate alloc;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::intent_router::{IntentRouter, IntentEvent, IntentContext, DispatchAction};
use crate::synapse_bridge::{IntentResult, IntentCategory, ConfidenceLevel};
use crate::qring_async::{QRingProcessor, SqEntry, SqOpcode};

// ── Pipeline Statistics ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct PipelineStats {
    pub results_processed: u64,
    pub gate_required_drops: u64,  // dropped: gate not confirmed
    pub actions_dispatched: u64,
    pub actions_nooped: u64,
    pub qshell_executes: u64,
    pub qview_navigates: u64,
    pub aether_focuses: u64,
    pub prism_pivots: u64,
    pub silo_spawns: u64,
    pub silo_dismissals: u64,
}

// ── Intent Pipeline ──────────────────────────────────────────────────────────

/// Wires synapse_bridge → intent_router → Q-Ring execution.
pub struct IntentPipeline {
    pub router: IntentRouter,
    pub stats: PipelineStats,
    /// Shell Silo ID for QShellExecute dispatch
    pub shell_silo_id: u64,
    /// Browser tab ID tracking (most recently active)
    pub active_tab_id: u64,
}

impl IntentPipeline {
    pub fn new(shell_silo_id: u64) -> Self {
        IntentPipeline {
            router: IntentRouter::new(shell_silo_id),
            stats: PipelineStats::default(),
            shell_silo_id,
            active_tab_id: 0,
        }
    }

    /// Process an IntentResult from the Synapse Bridge.
    /// Returns the DispatchAction taken (for telemetry / debug).
    pub fn submit(
        &mut self,
        result: IntentResult,
        context: IntentContext,
        qring: &mut QRingProcessor,
        tick: u64,
    ) -> DispatchAction {
        self.stats.results_processed += 1;

        // Law 1: ThoughtGate confirmation required for any action
        if !result.gate_confirmed {
            self.stats.gate_required_drops += 1;
            crate::serial_println!(
                "[INTENT PIPELINE] Dropped — ThoughtGate not confirmed (cat={:?})",
                result.category
            );
            return DispatchAction::NoOp { reason: "ThoughtGate not confirmed".to_string() };
        }

        // Convert IntentResult → IntentEvent
        let confidence = match result.confidence {
            ConfidenceLevel::Low    => 40u8,
            ConfidenceLevel::Medium => 70,
            ConfidenceLevel::High   => 85,
            ConfidenceLevel::Certain=> 98,
        };

        let category = convert_category(result.category);

        if category.is_none() {
            return DispatchAction::NoOp { reason: "Unknown intent category".to_string() };
        }

        let event = IntentEvent {
            category: category.unwrap(),
            confidence,
            silo_id: 4, // Synapse Silo ID
            context,
            confirmed_at: tick,
            double_confirmed: result.gate_confirmed,
        };

        // Dispatch through IntentRouter
        let action = self.router.dispatch(event);

        // Execute the action via Q-Ring
        self.execute_action(&action, qring, tick);
        self.stats.actions_dispatched += 1;

        action
    }

    /// Execute a DispatchAction by injecting Q-Ring submissions.
    fn execute_action(&mut self, action: &DispatchAction, qring: &mut QRingProcessor, tick: u64) {
        match action {
            DispatchAction::QShellExecute { silo_id, command } => {
                self.stats.qshell_executes += 1;
                // Submit IpcSend to deliver command string to Q-Shell Silo
                let sqe = SqEntry {
                    opcode: SqOpcode::IpcSend as u16,
                    flags: 0,
                    user_data: tick,
                    addr: *silo_id,
                    len: command.len() as u32,
                    aux: 0xCAFE, // Q-Shell command channel magic
                };
                self.inject(self.shell_silo_id, sqe, qring);
                crate::serial_println!(
                    "[INTENT PIPELINE] QShellExecute: silo={} cmd={}", silo_id, command
                );
            }

            DispatchAction::QViewNavigate { tab_id, uri } => {
                self.stats.qview_navigates += 1;
                self.active_tab_id = *tab_id;
                let sqe = SqEntry {
                    opcode: SqOpcode::IpcSend as u16,
                    flags: 0,
                    user_data: tick,
                    addr: *tab_id,
                    len: uri.len() as u32,
                    aux: 0xB401, // Q-View navigate channel
                };
                self.inject(self.shell_silo_id, sqe, qring);
                crate::serial_println!("[INTENT PIPELINE] QViewNavigate: tab={} uri={}", tab_id, uri);
            }

            DispatchAction::AetherFocus { silo_id } => {
                self.stats.aether_focuses += 1;
                let sqe = SqEntry {
                    opcode: SqOpcode::AetherSubmit as u16,
                    flags: 0,
                    user_data: tick,
                    addr: *silo_id,
                    len: 0,
                    aux: 0xAF01, // Aether focus command
                };
                self.inject(self.shell_silo_id, sqe, qring);
            }

            DispatchAction::PrismPivot { query, silo_id } => {
                self.stats.prism_pivots += 1;
                let sqe = SqEntry {
                    opcode: SqOpcode::PrismQuery as u16,
                    flags: 0,
                    user_data: tick,
                    addr: *silo_id,
                    len: query.len() as u32,
                    aux: 0,
                };
                self.inject(self.shell_silo_id, sqe, qring);
                crate::serial_println!("[INTENT PIPELINE] PrismPivot: query={}", query);
            }

            DispatchAction::SpawnShell => {
                self.stats.silo_spawns += 1;
                let sqe = SqEntry {
                    opcode: SqOpcode::SiloSpawn as u16,
                    flags: 0,
                    user_data: tick,
                    addr: 0, // shell binary OID (known at init)
                    len: 0,
                    aux: 0,
                };
                self.inject(self.shell_silo_id, sqe, qring);
                crate::serial_println!("[INTENT PIPELINE] SpawnShell requested");
            }

            DispatchAction::DismissSilo { silo_id } | DispatchAction::AbortCurrentSilo { silo_id } => {
                self.stats.silo_dismissals += 1;
                let sqe = SqEntry {
                    opcode: SqOpcode::SiloVaporize as u16,
                    flags: 0,
                    user_data: tick,
                    addr: *silo_id,
                    len: 0,
                    aux: 0xDEAD, // graceful termination token
                };
                self.inject(self.shell_silo_id, sqe, qring);
                crate::serial_println!("[INTENT PIPELINE] DismissSilo: {}", silo_id);
            }

            DispatchAction::CustomHandler { binding_id } => {
                crate::serial_println!("[INTENT PIPELINE] CustomHandler: binding={}", binding_id);
            }

            DispatchAction::NoOp { reason } => {
                self.stats.actions_nooped += 1;
                crate::serial_println!("[INTENT PIPELINE] NoOp: {}", reason);
            }
        }
    }

    fn inject(&self, silo_id: u64, sqe: SqEntry, qring: &mut QRingProcessor) {
        if !qring.rings.contains_key(&silo_id) {
            qring.register_silo(silo_id);
        }
        if let Some(ring) = qring.rings.get_mut(&silo_id) {
            if !ring.submit(sqe) {
                crate::serial_println!("[INTENT PIPELINE] Q-Ring full for Silo {}", silo_id);
            }
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IntentPipeline: processed={} dispatched={} noop={} shell={} nav={} focus={}",
            self.stats.results_processed, self.stats.actions_dispatched,
            self.stats.actions_nooped, self.stats.qshell_executes,
            self.stats.qview_navigates, self.stats.aether_focuses
        );
    }
}

// ── Category Conversion ───────────────────────────────────────────────────────

fn convert_category(cat: IntentCategory) -> Option<crate::intent_router::IntentCategory> {
    use crate::intent_router::IntentCategory as RouterCat;
    Some(match cat {
        IntentCategory::Navigate  => RouterCat::Navigate,
        IntentCategory::Focus     => RouterCat::Focus,
        IntentCategory::Execute   => RouterCat::Execute,
        IntentCategory::Dismiss   => RouterCat::Dismiss,
        IntentCategory::Pivot     => RouterCat::Pivot,
        IntentCategory::OpenShell => RouterCat::OpenShell,
        IntentCategory::Abort     => RouterCat::Abort,
        IntentCategory::Custom    => RouterCat::Custom(0),
        IntentCategory::Idle      => return None,
    })
}
