//! # Q-Kit SDK Widget Rate Bridge (Phase 271)
//!
//! ## Architecture Guardian: The Gap
//! `q_kit_sdk.rs` implements the Q-Kit native UI layout engine:
//! - `WidgetDesc { id, kind, style, hover, press, ... }`
//! - `WidgetEvent` — Hover, Press, Release, Focus, Blur
//!
//! **Missing link**: Widget creation was unbounded. A malicious UI Silo
//! could create millions of widgets, exhausting layout memory and
//! causing Aether compositor OOM.
//!
//! This module provides `QKitSdkWidgetRateBridge`:
//! Max 8192 widgets per Silo.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_WIDGETS_PER_SILO: u64 = 8192;

#[derive(Debug, Default, Clone)]
pub struct WidgetRateStats {
    pub allowed: u64,
    pub denied:  u64,
}

pub struct QKitSdkWidgetRateBridge {
    silo_widget_counts: BTreeMap<u64, u64>,
    pub stats:          WidgetRateStats,
}

impl QKitSdkWidgetRateBridge {
    pub fn new() -> Self {
        QKitSdkWidgetRateBridge { silo_widget_counts: BTreeMap::new(), stats: WidgetRateStats::default() }
    }

    pub fn allow_create(&mut self, silo_id: u64) -> bool {
        let count = self.silo_widget_counts.entry(silo_id).or_default();
        if *count >= MAX_WIDGETS_PER_SILO {
            self.stats.denied += 1;
            crate::serial_println!(
                "[Q-KIT] Silo {} widget quota full ({}/{})", silo_id, count, MAX_WIDGETS_PER_SILO
            );
            return false;
        }
        *count += 1;
        self.stats.allowed += 1;
        true
    }

    pub fn on_vaporize(&mut self, silo_id: u64) {
        self.silo_widget_counts.remove(&silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  QKitWidgetRateBridge: allowed={} denied={}", self.stats.allowed, self.stats.denied
        );
    }
}
