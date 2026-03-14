//! # Genesis Protocol — Architecture Bridge Shim
//!
//! `genesis.rs` is a backward-compatibility bridge. The original prototype
//! `GenesisProtocol` struct was split into two focused modules:
//!
//! - `firstboot.rs` (Phase 68): First Boot wizard (OOBE, user identity, migration)
//! - `nexus.rs` (Phase 61): Global Mesh network phases (Beacon → Compute Auction)
//!
//! Callers that used `crate::genesis::GenesisProtocol` should migrate to:
//! - `crate::firstboot::FirstBootState` for boot wizard state
//! - `crate::nexus::NexusRouter` for mesh coordination
//!
//! This shim re-exports the closest equivalent so existing code compiles
//! while a gradual refactor proceeds.

// Re-export wizard state as the "GenesisProtocol" alias for kstate.rs compatibility
pub use crate::firstboot::FirstBootState as GenesisProtocol;
pub use crate::firstboot::{FirstBootStep, HardwareProfile, MigrationSummary};

// Re-export nexus mesh types
pub use crate::nexus::{NodeId, NexusRouter, NexusStats};
