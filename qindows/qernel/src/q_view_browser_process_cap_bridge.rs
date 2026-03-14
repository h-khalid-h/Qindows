//! # Q-View Browser Process Cap Bridge (Phase 275)
//!
//! ## Architecture Guardian: The Gap
//! `q_view_browser.rs` implements Q-View Browser:
//! - Each website loaded in a separate Q-Silo
//! - Tab management, navigation, content sandboxing
//!
//! **Missing link**: Browser tab spawning (each a new Silo) was
//! uncapped. A user or malicious website could trigger 1000s of
//! tab spawns, exhausting the Silo pool and denying service to
//! other applications.
//!
//! This module provides `QViewBrowserProcessCapBridge`:
//! Max 32 browser tab Silos per browser session.

extern crate alloc;
use alloc::collections::BTreeMap;

const MAX_TABS_PER_SESSION: u64 = 32;

#[derive(Debug, Default, Clone)]
pub struct BrowserTabCapStats {
    pub spawns_allowed: u64,
    pub spawns_denied:  u64,
}

pub struct QViewBrowserProcessCapBridge {
    session_tab_counts: BTreeMap<u64, u64>, // browser_silo_id → tab count
    pub stats:          BrowserTabCapStats,
}

impl QViewBrowserProcessCapBridge {
    pub fn new() -> Self {
        QViewBrowserProcessCapBridge { session_tab_counts: BTreeMap::new(), stats: BrowserTabCapStats::default() }
    }

    pub fn allow_tab_spawn(&mut self, browser_silo_id: u64) -> bool {
        let count = self.session_tab_counts.entry(browser_silo_id).or_default();
        if *count >= MAX_TABS_PER_SESSION {
            self.stats.spawns_denied += 1;
            crate::serial_println!(
                "[Q-VIEW] Browser {} tab quota full ({}/{})", browser_silo_id, count, MAX_TABS_PER_SESSION
            );
            return false;
        }
        *count += 1;
        self.stats.spawns_allowed += 1;
        true
    }

    pub fn on_tab_closed(&mut self, browser_silo_id: u64) {
        let count = self.session_tab_counts.entry(browser_silo_id).or_default();
        *count = count.saturating_sub(1);
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  BrowserTabCapBridge: allowed={} denied={}",
            self.stats.spawns_allowed, self.stats.spawns_denied
        );
    }
}
