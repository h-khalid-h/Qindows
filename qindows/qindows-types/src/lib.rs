//! # Qindows Shared Types
//!
//! Common types, traits, and constants shared across all Qindows crates.
//! This is the foundation that enables cross-crate integration.

#![no_std]

extern crate alloc;

pub mod math_ext;
pub mod silo;
pub mod capability;
pub mod ipc;
pub mod error;
pub mod boot;
