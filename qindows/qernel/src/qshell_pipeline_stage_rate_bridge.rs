//! # QShell Pipeline Stage Rate Bridge (Phase 276)
//!
//! ## Architecture Guardian: The Gap
//! `qshell.rs` implements Q-Shell execution pipeline:
//! - `Pipeline { stages: Vec<PipelineStage>, state: PipelineState }`
//! - `PipelineStage { cap: StageCap }`
//! - `StageCap` — Read, Write, Admin, Network, ...
//!
//! **Missing link**: Pipeline stages were added without count limit.
//! A Q-Shell script could construct an arbitrarily long pipeline chain,
//! causing deep recursion and stack overflow in the pipeline executor.
//!
//! This module provides `QShellPipelineStageRateBridge`:
//! Max 64 stages per Q-Shell pipeline.

extern crate alloc;

const MAX_PIPELINE_STAGES: u64 = 64;

#[derive(Debug, Default, Clone)]
pub struct PipelineStageRateStats {
    pub stages_allowed: u64,
    pub stages_denied:  u64,
}

pub struct QShellPipelineStageRateBridge {
    pub stats: PipelineStageRateStats,
}

impl QShellPipelineStageRateBridge {
    pub fn new() -> Self {
        QShellPipelineStageRateBridge { stats: PipelineStageRateStats::default() }
    }

    pub fn allow_add_stage(&mut self, current_stage_count: u64) -> bool {
        if current_stage_count >= MAX_PIPELINE_STAGES {
            self.stats.stages_denied += 1;
            crate::serial_println!(
                "[QSHELL] Pipeline stage limit reached ({}/{})", current_stage_count, MAX_PIPELINE_STAGES
            );
            return false;
        }
        self.stats.stages_allowed += 1;
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  PipelineStageBridge: allowed={} denied={}",
            self.stats.stages_allowed, self.stats.stages_denied
        );
    }
}
