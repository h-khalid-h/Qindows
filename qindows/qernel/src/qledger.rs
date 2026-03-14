//! # Q-Ledger (canonical module in `ledger.rs` — Phase 63)
//!
//! This file is a backward-compatibility re-export shim.
//! All logic lives in `ledger.rs` which supersedes the original stub here.
//!
//! Architecture Guardian: keeping this file as a thin alias ensures that
//! any existing `use crate::qledger::*` references in older modules
//! continue to compile while `ledger.rs` is the single source of truth.
pub use crate::ledger::*;
