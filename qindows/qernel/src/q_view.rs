//! # Q-View Browser — Websites as Native Q-Silos (Phase 74)
//!
//! ARCHITECTURE.md §5.4:
//! > "Websites = Remote Q-Apps streamed as native Q-Kit widget trees"
//! > "Rendered by the same Aether vector engine as local apps → 0ms scroll lag"
//! > "No separate browser process — websites are first-class Silos"
//!
//! ## Architecture Guardian: How Q-View differs from a traditional browser
//! ```text
//! Traditional browser                 Q-View
//! ─────────────────────────────────   ──────────────────────────────────────
//! HTML/JS/CSS → layout engine         Q-View Server compiles to Q-Kit tree
//! Large renderer process (Chrome=2GB) Each site = isolated Q-Silo (~50MB)
//! Separate scroll thread              Aether native: 0ms scroll lag
//! All sites share one Cookie jar      Each Silo has isolated K-V (Law 6)
//! "Tab crashes" affect all tabs       Silo vaporize = only that tab dies
//! ```
//!
//! ## Architecture Guardian: Layering
//! ```text
//! Q-View Client (Aether UI)
//!     │  QViewRequest (navigate to URL)
//!     ▼
//! QViewEngine (this module)
//!     │  1. Resolve via Q-Fabric (qfa://host)
//!     │  2. Receive Q-Kit WidgetTree from Q-View Server
//!     │  3. Spawn a fresh Q-Silo for this site (Law 6)
//!     │  4. Submit WidgetTree to Aether (scene graph)
//!     │  5. Route user input back to Silo
//!     ▼
//! Q-Silo (site-isolated, no ambient authority)
//! ```
//!
//! ## Security (Q-Manifest Laws)
//! - **Law 1**: Site Silo starts with zero capabilities
//! - **Law 6**: No cross-site memory; no shared cookie store
//! - **Law 7**: NET_SEND required for any network requests the site makes
//! - **Law 9**: Site resources addressed via `qfa://host/path` UNS URIs

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;
use alloc::format;

// ── Q-Kit Widget Tree (simplified) ───────────────────────────────────────────

/// A Q-Kit widget type — the atoms of Aether's scene graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetKind {
    Text,
    Button,
    Image,
    Container,
    Input,
    Scroll,
    Link,
    Video,
    Canvas,
}

/// A single Q-Kit widget node in the scene graph.
#[derive(Debug, Clone)]
pub struct WidgetNode {
    /// Unique ID within this page's scene graph
    pub widget_id: u32,
    /// Widget type
    pub kind: WidgetKind,
    /// Layout: x, y, width, height (in pixels at 1x density)
    pub rect: (i32, i32, u32, u32),
    /// Text content (for Text/Button/Input)
    pub content: String,
    /// UNS URI for resources (images/videos use `qfa://` or `prism://`)
    pub resource_uri: Option<String>,
    /// Child widget IDs (in render order)
    pub children: Vec<u32>,
    /// CSS-equivalent style string (Q-Kit uses SDF shaders — Law 4 compliant)
    pub style: String,
    /// Whether this widget triggers a UNS navigate on interaction
    pub navigate_to: Option<String>,
}

/// The complete Q-Kit widget tree for a rendered page.
#[derive(Debug, Clone, Default)]
pub struct QKitTree {
    /// All widget nodes: widget_id → node
    pub nodes: BTreeMap<u32, WidgetNode>,
    /// Root widget IDs (top-level layout)
    pub roots: Vec<u32>,
    /// Page title
    pub title: String,
    /// Page UNS URI
    pub url: String,
    /// Total number of widgets in the tree
    pub widget_count: u32,
    /// Tree version (incremented on each server push)
    pub version: u32,
}

impl QKitTree {
    pub fn new(url: &str, title: &str) -> Self {
        QKitTree {
            nodes: BTreeMap::new(),
            roots: Vec::new(),
            title: title.to_string(),
            url: url.to_string(),
            widget_count: 0,
            version: 1,
        }
    }

    pub fn add_widget(&mut self, widget: WidgetNode) {
        let id = widget.widget_id;
        if !widget.children.is_empty() || widget.widget_id < 10 {
            self.roots.push(id);
        }
        self.nodes.insert(id, widget);
        self.widget_count += 1;
    }

    /// Apply a delta update from the server (avoids full retransmission).
    pub fn apply_delta(&mut self, changed_widgets: Vec<WidgetNode>, removed_ids: Vec<u32>) {
        for id in removed_ids {
            self.nodes.remove(&id);
        }
        for widget in changed_widgets {
            let id = widget.widget_id;
            self.nodes.insert(id, widget);
        }
        self.version += 1;
        self.widget_count = self.nodes.len() as u32;
    }
}

// ── Q-View Tab ────────────────────────────────────────────────────────────────

/// State of a single Q-View browser tab (each is an isolated Q-Silo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabState {
    /// Connecting to Q-View server
    Connecting,
    /// Loading the widget tree
    Loading,
    /// Fully rendered and interactive
    Active,
    /// Background tab — deep-sleeping (Law 8: no ActiveTask token)
    Suspended,
    /// An error occurred (Silo quarantined)
    Error,
}

#[derive(Debug, Clone)]
pub struct QViewTab {
    /// Tab ID (also the Silo ID)
    pub tab_id: u64,
    /// Current URL (UNS URI: `qfa://hostname/path` or `prism://oid`)
    pub url: String,
    /// Current page title
    pub title: String,
    /// Current render state
    pub state: TabState,
    /// The widget tree received from Q-View server
    pub tree: Option<QKitTree>,
    /// History stack (Law 10: offline= serve last cached tree)
    pub history: Vec<String>,
    /// Cached tree OIDs for offline fallback (Law 10)
    pub offline_cache: BTreeMap<String, u32>, // url → tree version
    /// Data transmitted by this tab in bytes (for Law 7 accounting)
    pub bytes_sent: u64,
    /// Page load time (ticks from request to first paint)
    pub load_time_ticks: u64,
    /// Tick of last user interaction (for suspension decision)
    pub last_interaction_tick: u64,
}

impl QViewTab {
    pub fn new(tab_id: u64, tick: u64) -> Self {
        QViewTab {
            tab_id,
            url: String::new(),
            title: "New Tab".to_string(),
            state: TabState::Connecting,
            tree: None,
            history: Vec::new(),
            offline_cache: BTreeMap::new(),
            bytes_sent: 0,
            load_time_ticks: 0,
            last_interaction_tick: tick,
        }
    }

    /// Navigate to a new URL.
    pub fn navigate(&mut self, url: &str, tick: u64) {
        if !self.url.is_empty() {
            self.history.push(self.url.clone());
        }
        self.url = url.to_string();
        self.state = TabState::Loading;
        self.last_interaction_tick = tick;
        crate::serial_println!(
            "[QVIEW] Tab {} navigating to: {}", self.tab_id, url
        );
    }

    /// Receive a Q-Kit tree from the Q-View server.
    pub fn receive_tree(&mut self, tree: QKitTree, load_ticks: u64) {
        self.title = tree.title.clone();
        // Cache URL for offline fallback (Law 10)
        self.offline_cache.insert(self.url.clone(), tree.version);
        self.tree = Some(tree);
        self.state = TabState::Active;
        self.load_time_ticks = load_ticks;
        crate::serial_println!(
            "[QVIEW] Tab {} loaded: \"{}\" in {}ticks", self.tab_id, self.title, load_ticks
        );
    }

    /// Suspend a background tab (Law 8).
    pub fn suspend(&mut self) {
        self.state = TabState::Suspended;
        // Drop the widget tree from RAM — Aether keeps a frozen screenshot
        // Only the offline_cache entry remains
        self.tree = None;
        crate::serial_println!("[QVIEW] Tab {} suspended (Law 8: no ActiveTask).", self.tab_id);
    }

    /// Resume a suspended tab.
    pub fn resume(&mut self, tick: u64) {
        self.state = TabState::Loading;
        self.last_interaction_tick = tick;
        crate::serial_println!("[QVIEW] Tab {} resuming...", self.tab_id);
    }

    /// Go back in history.
    pub fn go_back(&mut self, tick: u64) -> Option<&str> {
        if let Some(prev) = self.history.pop() {
            self.url = prev;
            self.state = TabState::Loading;
            self.last_interaction_tick = tick;
            Some(&self.url)
        } else {
            None
        }
    }
}

// ── Q-View Engine Statistics ──────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct QViewStats {
    pub tabs_opened: u64,
    pub tabs_closed: u64,
    pub navigations: u64,
    pub trees_received: u64,
    pub tabs_suspended: u64,
    pub offline_falls: u64, // Law 10: navigations served from cache
    pub total_widgets_rendered: u64,
}

// ── Q-View Engine ─────────────────────────────────────────────────────────────

/// The kernel Q-View engine — manages all browser tabs as Q-Silos.
pub struct QViewEngine {
    /// All open tabs: tab_id → QViewTab
    pub tabs: BTreeMap<u64, QViewTab>,
    /// Currently focused tab ID
    pub active_tab: Option<u64>,
    /// Next tab ID
    next_tab_id: u64,
    /// Statistics
    pub stats: QViewStats,
    /// Maximum inactive ticks before a tab is auto-suspended (Law 8)
    pub suspension_threshold_ticks: u64,
}

impl QViewEngine {
    pub fn new() -> Self {
        QViewEngine {
            tabs: BTreeMap::new(),
            active_tab: None,
            next_tab_id: 1,
            stats: QViewStats::default(),
            suspension_threshold_ticks: 30_000, // 30 seconds
        }
    }

    /// Open a new tab (spawns a Q-Silo — simulated here).
    pub fn open_tab(&mut self, tick: u64) -> u64 {
        let tab_id = self.next_tab_id;
        self.next_tab_id += 1;
        let tab = QViewTab::new(tab_id, tick);
        self.tabs.insert(tab_id, tab);
        self.active_tab = Some(tab_id);
        self.stats.tabs_opened += 1;
        crate::serial_println!("[QVIEW] Tab {} opened (Silo={}). Total: {}",
            tab_id, tab_id, self.tabs.len());
        tab_id
    }

    /// Navigate a tab to a URL.
    pub fn navigate(&mut self, tab_id: u64, url: &str, tick: u64) -> bool {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            tab.navigate(url, tick);
            self.stats.navigations += 1;
            true
        } else {
            false
        }
    }

    /// Deliver a received Q-Kit widget tree to a tab (called by Q-Fabric handler).
    pub fn deliver_tree(&mut self, tab_id: u64, tree: QKitTree, request_tick: u64, tick: u64) -> bool {
        let load_ticks = tick.saturating_sub(request_tick);
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            self.stats.total_widgets_rendered += tree.widget_count as u64;
            self.stats.trees_received += 1;
            tab.receive_tree(tree, load_ticks);
            true
        } else {
            false
        }
    }

    /// Close a tab and vaporize its Silo.
    pub fn close_tab(&mut self, tab_id: u64) {
        if self.tabs.remove(&tab_id).is_some() {
            if self.active_tab == Some(tab_id) {
                self.active_tab = self.tabs.keys().next_back().copied();
            }
            self.stats.tabs_closed += 1;
            crate::serial_println!("[QVIEW] Tab {} closed + Silo vaporized.", tab_id);
        }
    }

    /// Focus a tab (requests ActiveTask token — Law 8).
    pub fn focus_tab(&mut self, tab_id: u64, tick: u64) {
        if let Some(tab) = self.tabs.get_mut(&tab_id) {
            if tab.state == TabState::Suspended {
                tab.resume(tick);
            }
            tab.last_interaction_tick = tick;
        }
        self.active_tab = Some(tab_id);
    }

    /// Law 8: suspend all non-focused tabs that exceed inactivity threshold.
    pub fn enforce_suspension(&mut self, tick: u64) {
        let active = self.active_tab;
        let threshold = self.suspension_threshold_ticks;
        for tab in self.tabs.values_mut() {
            if Some(tab.tab_id) == active { continue; }
            if tab.state == TabState::Active {
                let inactive_for = tick.saturating_sub(tab.last_interaction_tick);
                if inactive_for > threshold {
                    tab.suspend();
                    self.stats.tabs_suspended += 1;
                }
            }
        }
    }

    /// Build a synthetic Q-Kit tree for demonstration/testing.
    pub fn demo_tree(url: &str, title: &str) -> QKitTree {
        let mut tree = QKitTree::new(url, title);
        tree.add_widget(WidgetNode {
            widget_id: 1,
            kind: WidgetKind::Container,
            rect: (0, 0, 1920, 1080),
            content: String::new(),
            resource_uri: None,
            children: alloc::vec![2, 3],
            style: "background: var(--q-glass);".to_string(),
            navigate_to: None,
        });
        tree.add_widget(WidgetNode {
            widget_id: 2,
            kind: WidgetKind::Text,
            rect: (100, 100, 600, 60),
            content: title.to_string(),
            resource_uri: None,
            children: Vec::new(),
            style: "font-size: 32px; font-weight: bold;".to_string(),
            navigate_to: None,
        });
        tree.add_widget(WidgetNode {
            widget_id: 3,
            kind: WidgetKind::Button,
            rect: (100, 200, 200, 50),
            content: "Open Q-Shell".to_string(),
            resource_uri: None,
            children: Vec::new(),
            style: "style: ButtonStyle::GlassMorph;".to_string(),
            navigate_to: Some("qshell://new".to_string()),
        });
        tree
    }
}
