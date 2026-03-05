//! # Q-View — Websites as Native Remote Q-Apps
//!
//! The browser is not an OS layer (Section 5). Q-View renders
//! websites as native Remote Q-Apps using the Aether vector engine.
//! Web content streams through Q-Proxy and is rendered as native
//! Q-Kit components instead of HTML bitmaps.
//!
//! Architecture:
//! - DOM stream arrives via Q-Proxy (DoH + onion routing)
//! - Layout engine converts CSS → Q-Kit layout primitives
//! - SDF text replaces bitmap fonts
//! - Each tab runs in its own Silo (hardware isolation)
//! - JavaScript executes in a Wasm sandbox

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Tab state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabState {
    Loading,
    Active,
    Background,
    Suspended,
    Crashed,
}

/// Navigation entry (history item).
#[derive(Debug, Clone)]
pub struct NavEntry {
    pub url: String,
    pub title: String,
    pub timestamp: u64,
}

/// A Q-View tab.
#[derive(Debug, Clone)]
pub struct Tab {
    /// Tab ID
    pub id: u64,
    /// Silo ID (isolation boundary)
    pub silo_id: u64,
    /// Current URL
    pub url: String,
    /// Page title
    pub title: String,
    /// State
    pub state: TabState,
    /// Navigation history
    pub history: Vec<NavEntry>,
    /// Current history index
    pub history_idx: usize,
    /// Memory usage (bytes)
    pub memory: u64,
    /// Is secure (HTTPS)?
    pub secure: bool,
    /// Blocked trackers count
    pub trackers_blocked: u64,
    /// Proxy circuit ID
    pub circuit_id: Option<u64>,
}

/// Content security policy.
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    /// Block third-party cookies
    pub block_cookies: bool,
    /// Block trackers
    pub block_trackers: bool,
    /// Force HTTPS
    pub force_https: bool,
    /// Block JavaScript (extreme privacy)
    pub block_js: bool,
    /// Maximum script execution time (ms)
    pub script_timeout_ms: u64,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        SecurityPolicy {
            block_cookies: true,
            block_trackers: true,
            force_https: true,
            block_js: false,
            script_timeout_ms: 10_000,
        }
    }
}

/// Q-View statistics.
#[derive(Debug, Clone, Default)]
pub struct ViewStats {
    pub tabs_opened: u64,
    pub tabs_closed: u64,
    pub pages_loaded: u64,
    pub trackers_blocked: u64,
    pub bytes_downloaded: u64,
    pub scripts_sandboxed: u64,
}

/// The Q-View Manager.
pub struct QView {
    /// Open tabs
    pub tabs: BTreeMap<u64, Tab>,
    /// Active tab ID
    pub active_tab: Option<u64>,
    /// Next tab ID
    next_tab_id: u64,
    /// Security policy
    pub policy: SecurityPolicy,
    /// Statistics
    pub stats: ViewStats,
}

impl QView {
    pub fn new() -> Self {
        QView {
            tabs: BTreeMap::new(),
            active_tab: None,
            next_tab_id: 1,
            policy: SecurityPolicy::default(),
            stats: ViewStats::default(),
        }
    }

    /// Open a new tab.
    pub fn open_tab(&mut self, url: &str, silo_id: u64, now: u64) -> u64 {
        let id = self.next_tab_id;
        self.next_tab_id += 1;

        let secure = url.starts_with("https://") || self.policy.force_https;

        let mut tab = Tab {
            id,
            silo_id,
            url: String::from(url),
            title: String::from("Loading..."),
            state: TabState::Loading,
            history: Vec::new(),
            history_idx: 0,
            memory: 0,
            secure,
            trackers_blocked: 0,
            circuit_id: None,
        };

        tab.history.push(NavEntry {
            url: String::from(url),
            title: String::new(),
            timestamp: now,
        });

        self.tabs.insert(id, tab);
        if self.active_tab.is_none() {
            self.active_tab = Some(id);
        }
        self.stats.tabs_opened += 1;
        id
    }

    /// Navigate a tab to a new URL.
    pub fn navigate(&mut self, tab_id: u64, url: &str, now: u64) -> Result<(), &'static str> {
        let tab = self.tabs.get_mut(&tab_id).ok_or("Tab not found")?;

        // Truncate forward history if we're not at the end
        if tab.history_idx + 1 < tab.history.len() {
            tab.history.truncate(tab.history_idx + 1);
        }

        tab.url = String::from(url);
        tab.title = String::from("Loading...");
        tab.state = TabState::Loading;
        tab.secure = url.starts_with("https://") || self.policy.force_https;

        tab.history.push(NavEntry {
            url: String::from(url),
            title: String::new(),
            timestamp: now,
        });
        tab.history_idx = tab.history.len() - 1;

        self.stats.pages_loaded += 1;
        Ok(())
    }

    /// Go back in history.
    pub fn go_back(&mut self, tab_id: u64) -> Result<(), &'static str> {
        let tab = self.tabs.get_mut(&tab_id).ok_or("Tab not found")?;
        if tab.history_idx == 0 { return Err("No back history"); }
        tab.history_idx -= 1;
        tab.url = tab.history[tab.history_idx].url.clone();
        tab.state = TabState::Loading;
        Ok(())
    }

    /// Go forward in history.
    pub fn go_forward(&mut self, tab_id: u64) -> Result<(), &'static str> {
        let tab = self.tabs.get_mut(&tab_id).ok_or("Tab not found")?;
        if tab.history_idx + 1 >= tab.history.len() { return Err("No forward history"); }
        tab.history_idx += 1;
        tab.url = tab.history[tab.history_idx].url.clone();
        tab.state = TabState::Loading;
        Ok(())
    }

    /// Close a tab.
    pub fn close_tab(&mut self, tab_id: u64) {
        self.tabs.remove(&tab_id);
        if self.active_tab == Some(tab_id) {
            self.active_tab = self.tabs.keys().next().copied();
        }
        self.stats.tabs_closed += 1;
    }

    /// Switch active tab.
    pub fn switch_tab(&mut self, tab_id: u64) {
        if let Some(old) = self.active_tab {
            if let Some(tab) = self.tabs.get_mut(&old) {
                tab.state = TabState::Background;
            }
        }
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.state = TabState::Active;
            self.active_tab = Some(tab_id);
        }
    }

    /// Mark page as loaded.
    pub fn on_page_loaded(&mut self, tab_id: u64, title: &str, memory: u64) {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.title = String::from(title);
            tab.state = TabState::Active;
            tab.memory = memory;
            if let Some(entry) = tab.history.get_mut(tab.history_idx) {
                entry.title = String::from(title);
            }
        }
    }

    /// Record a blocked tracker.
    pub fn block_tracker(&mut self, tab_id: u64) {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.trackers_blocked += 1;
        }
        self.stats.trackers_blocked += 1;
    }
}
