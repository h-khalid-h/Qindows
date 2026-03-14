//! # RTC Time-Fence Bridge (Phase 191)
//!
//! ## Architecture Guardian: The Gap
//! `rtc.rs` implements `Rtc`:
//! - `Rtc::read_time()` → DateTime — read current hardware clock
//! - `Rtc::set_time(dt: DateTime)` — set hardware clock
//! - `DateTime::to_timestamp()` → u64
//!
//! **Missing link**: `Rtc::set_time()` was callable without Admin:EXEC cap.
//! A Silo could manipulate the real clock, breaking time-critical ops
//! (certificate expiry, CapToken TTL, audit timestamps).
//!
//! This module provides `RtcTimeFenceBridge`:
//! `set_time_with_cap()` — Admin:EXEC required; `read_time()` is unrestricted.

extern crate alloc;

use crate::rtc::{Rtc, DateTime};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct RtcBridgeStats {
    pub reads:       u64,
    pub sets_allowed: u64,
    pub sets_denied:  u64,
}

pub struct RtcTimeFenceBridge {
    pub rtc:   Rtc,
    pub stats: RtcBridgeStats,
}

impl RtcTimeFenceBridge {
    pub fn new() -> Self {
        RtcTimeFenceBridge { rtc: Rtc::new(), stats: RtcBridgeStats::default() }
    }

    /// Read current hardware time — unrestricted (observing is harmless).
    pub fn read_time(&mut self) -> DateTime {
        self.stats.reads += 1;
        self.rtc.read_time()
    }

    /// Set hardware clock — requires Admin:EXEC cap.
    pub fn set_time_with_cap(
        &mut self,
        silo_id: u64,
        dt: DateTime,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.sets_denied += 1;
            crate::serial_println!("[RTC] Silo {} clock-set denied — no Admin:EXEC cap", silo_id);
            return false;
        }
        self.stats.sets_allowed += 1;
        crate::serial_println!("[RTC] Clock set by Silo {} → ts={}", silo_id, dt.to_timestamp());
        self.rtc.set_time(dt);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  RtcBridge: reads={} sets={}/{}",
            self.stats.reads, self.stats.sets_allowed, self.stats.sets_denied
        );
    }
}
