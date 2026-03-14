//! # Q-View Browser — Websites as Native Q-Silos (Phase 96)
//!
//! ARCHITECTURE.md §5.4 — Q-View Browser:
//! > "Websites = Remote Q-Apps streamed as native Q-Kit widget trees"
//! > "Rendered by the same Aether vector engine as local apps → 0ms scroll lag"
//! > "No separate browser process — websites are first-class Silos"
//!
//! ## Architecture Guardian: Why this is different from a traditional browser
//!
//! Traditional browsers (Chrome, Firefox):
//! - Run a full HTML/CSS/JS engine in-process
//! - Maintain their own compositor, networking, JavaScript runtime
//! - The "browser" is effectively a second OS inside the OS → massive footprint
//!
//! Q-View Browser model:
//! ```text
//! Q-Server (remote or local):
//!   - Receives URL from Qindows device
//!   - Runs the server-side Q-App runtime (JS/WASM, HTML→Q-Kit transform)
//!   - Produces a Q-Kit widget tree (same format as native Qindows apps)
//!   - Streams widget tree updates via Q-Fabric (not video, not HTML)
//!
//! Q-View on device:
//!   - Receives Q-Kit widget tree via QFabric connection
//!   - Hands it directly to Aether compositor
//!   - Aether renders with SDF precision — exactly like a native app
//!   - Input events: device captures → Q-Fabric → Q-Server
//! ```
//!
//! ## Benefits
//! 1. **Zero scroll lag**: widgets are vectors, Aether recomputes anything instantly
//! 2. **No JS sandbox complexity**: JS is on the server, device runs no JS
//! 3. **No tracker surface**: network requests go through Q-Server, device never touches ad networks
//! 4. **Native feel**: a website IS a Qindows Silo — same Tab key navigation, A11y, Law 1 caps
//!
//! ## Law Compliance
//! - **Law 1**: Website Silo gets zero caps at spawn — explicitly granted NET_RECV only
//! - **Law 7**: All traffic flows through qtraffic.rs monitoring
//! - **Law 9**: URLs stored as UNS URIs `qfa://qserver:8443/prism//http/example.com/page`
//! - **Law 10**: Page cached as Prism Shadow Object for offline viewing

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Page State ────────────────────────────────────────────────────────────────

/// Current state of a browsed page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageState {
    /// Connecting to Q-Server or fetching page
    Loading,
    /// Widget tree received, painting
    Rendering,
    /// Fully interactive
    Interactive,
    /// Connection lost — showing cached (Law 10)
    Offline,
    /// Error (DNS fail, TLS fail, server error)
    Error,
    /// Page is done, deallocating
    Closing,
}

// ── Q-Kit Widget Node (compact kernel representation) ─────────────────────────

/// Widget type — what the Q-Server serialised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WidgetKind {
    Root,
    Container,
    Text,
    Button,
    TextInput,
    Image { oid: [u8; 32] },      // image loaded into Prism cache
    Video { stream_oid: u64 },    // live video stream handle
    Canvas,                        // arbitrary SDF drawing surface
    ScrollRegion,
    Table,
    Link { href_oid: [u8; 32] },  // links stored as Prism OIDs of URLs
}

/// One widget node in the remote-rendered tree.
#[derive(Debug, Clone)]
pub struct QKitNode {
    pub id: u32,
    pub kind: WidgetKind,
    pub parent: Option<u32>,
    pub children: Vec<u32>,
    /// Text content for Text/Button/TextInput nodes
    pub text: Option<String>,
    /// Bounding box [x, y, w, h] in widget-local coordinates
    pub rect: [f32; 4],
    /// Computed opacity (from CSS opacity/visibility)
    pub opacity: f32,
    /// Background colour: 0xRRGGBBAA
    pub bg_color: u32,
    /// Text colour: 0xRRGGBBAA
    pub text_color: u32,
    /// Font size in px
    pub font_size: f32,
    /// Accessibility role (A11y: feeds into aether_a11y.rs)
    pub a11y_role: u8,
    /// Law 9 UNS URI for this node's primary resource
    pub uns_uri: Option<String>,
}

// ── Navigation Entry ──────────────────────────────────────────────────────────

/// One entry in the navigation history stack (Law 10: offline fallback).
#[derive(Debug, Clone)]
pub struct NavEntry {
    /// URL as UNS URI
    pub uns_uri: String,
    /// Prism OID of cached offline version (Shadow Object from Ghost-Write)
    pub cache_oid: Option<[u8; 32]>,
    /// Tick when this page was visited
    pub visited_at: u64,
    /// Page title
    pub title: String,
    /// Whether this entry has a valid offline cache
    pub has_offline: bool,
}

// ── Q-Server Connection ───────────────────────────────────────────────────────

/// Represents an active Q-Fabric connection to a Q-Server for browsing.
#[derive(Debug, Clone)]
pub struct QServerConn {
    /// Q-Fabric NodeId of the Q-Server
    pub server_node: u64,
    /// Q-Fabric stream ID for this page session
    pub stream_id: u64,
    /// Established at tick
    pub connected_at: u64,
    /// Bytes received from server (for qtraffic.rs law 7 accounting)
    pub bytes_received: u64,
    /// Bytes sent to server (input events)
    pub bytes_sent: u64,
    /// Round-trip time in ticks
    pub rtt_ticks: u64,
}

// ── Browser Tab ───────────────────────────────────────────────────────────────

/// One browser tab — a website Silo session.
pub struct QViewTab {
    /// The Silo ID this tab is running in
    pub silo_id: u64,
    /// Current URL (as UNS URI)
    pub current_url: String,
    /// Page state
    pub state: PageState,
    /// Widget tree: node_id → QKitNode
    pub widgets: BTreeMap<u32, QKitNode>,
    /// Navigation history (back/forward)
    pub history: Vec<NavEntry>,
    pub history_pos: usize,
    /// Active Q-Server connection
    pub connection: Option<QServerConn>,
    /// Scroll position [x, y] in logical pixels
    pub scroll: [f32; 2],
    /// Zoom factor (1.0 = 100%, 2.0 = 200%)
    pub zoom: f32,
    /// Can go back?
    pub can_back: bool,
    /// Can go forward?
    pub can_forward: bool,
}

impl QViewTab {
    pub fn new(silo_id: u64) -> Self {
        QViewTab {
            silo_id,
            current_url: String::new(),
            state: PageState::Loading,
            widgets: BTreeMap::new(),
            history: Vec::new(),
            history_pos: 0,
            connection: None,
            scroll: [0.0, 0.0],
            zoom: 1.0,
            can_back: false,
            can_forward: false,
        }
    }

    /// Apply a widget tree update from the Q-Server (delta update).
    pub fn apply_widget_update(&mut self, nodes: Vec<QKitNode>) {
        for node in nodes { self.widgets.insert(node.id, node); }
        self.state = PageState::Interactive;
    }

    /// Remove a subtree of widgets (for dynamic content removal).
    pub fn remove_subtree(&mut self, root_id: u32) {
        let mut to_remove = Vec::new();
        let mut queue = Vec::new();
        queue.push(root_id);
        while let Some(id) = queue.pop() {
            to_remove.push(id);
            if let Some(node) = self.widgets.get(&id) {
                queue.extend(node.children.iter().copied());
            }
        }
        for id in to_remove { self.widgets.remove(&id); }
    }

    /// Navigate to a new URL. Returns true if Q-Server connection should be initiated.
    pub fn navigate_to(&mut self, url: &str, tick: u64) -> bool {
        // Push current page into history
        if !self.current_url.is_empty() {
            let entry = NavEntry {
                uns_uri: self.current_url.clone(),
                cache_oid: None,
                visited_at: tick,
                title: "Previous Page".to_string(),
                has_offline: false,
            };
            if self.history_pos < self.history.len() {
                self.history.truncate(self.history_pos + 1);
            }
            self.history.push(entry);
            self.history_pos = self.history.len().saturating_sub(1);
        }

        self.current_url = url.to_string();
        self.state = PageState::Loading;
        self.widgets.clear();
        self.scroll = [0.0, 0.0];
        self.can_back = self.history_pos > 0;
        self.can_forward = self.history_pos + 1 < self.history.len();
        true // caller should initiate Q-Fabric connection
    }

    /// Go back in history. Returns the URL to navigate to, or None.
    pub fn go_back(&mut self) -> Option<&str> {
        if self.history_pos == 0 { return None; }
        self.history_pos -= 1;
        self.can_back = self.history_pos > 0;
        self.can_forward = true;
        Some(&self.history[self.history_pos].uns_uri)
    }

    /// Go forward in history.
    pub fn go_forward(&mut self) -> Option<&str> {
        if self.history_pos + 1 >= self.history.len() { return None; }
        self.history_pos += 1;
        self.can_back = true;
        self.can_forward = self.history_pos + 1 < self.history.len();
        Some(&self.history[self.history_pos].uns_uri)
    }

    /// Cache the current page to Prism (for Law 10 offline support).
    pub fn cache_current(&mut self, cache_oid: [u8; 32]) {
        if self.history_pos < self.history.len() {
            self.history[self.history_pos].cache_oid = Some(cache_oid);
            self.history[self.history_pos].has_offline = true;
        }
    }

    /// Fall back to cached version (Law 10 — offline).
    pub fn go_offline(&mut self) -> Option<[u8; 32]> {
        self.state = PageState::Offline;
        if self.history_pos < self.history.len() {
            self.history[self.history_pos].cache_oid
        } else { None }
    }
}

// ── Browser Statistics ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct BrowserStats {
    pub tabs_opened: u64,
    pub tabs_closed: u64,
    pub pages_loaded: u64,
    pub widgets_received: u64,
    pub offline_fallbacks: u64,   // Law 10 invocations
    pub bytes_received: u64,
    pub bytes_sent: u64,
    pub cache_hits: u64,
}

// ── Q-View Browser Engine ─────────────────────────────────────────────────────

/// Q-View Browser — manages website Silos and Q-Server connections.
pub struct QViewBrowser {
    /// Active tabs: silo_id → tab
    pub tabs: BTreeMap<u64, QViewTab>,
    /// Currently focused tab
    pub active_tab: Option<u64>,
    /// Statistics
    pub stats: BrowserStats,
    /// Default Q-Server NodeId (resolution server for URLs)
    pub default_qserver: Option<u64>,
}

impl QViewBrowser {
    pub fn new() -> Self {
        QViewBrowser {
            tabs: BTreeMap::new(),
            active_tab: None,
            stats: BrowserStats::default(),
            default_qserver: None,
        }
    }

    /// Open a new browser tab for a Silo.
    pub fn open_tab(&mut self, silo_id: u64, initial_url: Option<&str>, tick: u64) {
        let mut tab = QViewTab::new(silo_id);
        if let Some(url) = initial_url {
            tab.navigate_to(url, tick);
        }
        self.tabs.insert(silo_id, tab);
        self.active_tab = Some(silo_id);
        self.stats.tabs_opened += 1;
        crate::serial_println!(
            "[Q-VIEW BROWSER] Tab opened: Silo {} URL={:?}",
            silo_id, initial_url.unwrap_or("about:blank")
        );
    }

    /// Close a tab and release Silo resource.
    pub fn close_tab(&mut self, silo_id: u64) {
        self.tabs.remove(&silo_id);
        if self.active_tab == Some(silo_id) {
            self.active_tab = self.tabs.keys().next().copied();
        }
        self.stats.tabs_closed += 1;
    }

    /// Receive a widget tree delta from Q-Server for a tab.
    pub fn receive_widget_update(&mut self, silo_id: u64, nodes: Vec<QKitNode>) {
        let count = nodes.len();
        if let Some(tab) = self.tabs.get_mut(&silo_id) {
            tab.apply_widget_update(nodes);
            self.stats.widgets_received += count as u64;
            self.stats.pages_loaded += 1;
        }
    }

    /// Update server connection stats (called from qtraffic.rs law 7 hook).
    pub fn update_traffic(&mut self, silo_id: u64, bytes_rx: u64, bytes_tx: u64, rtt: u64) {
        if let Some(tab) = self.tabs.get_mut(&silo_id) {
            if let Some(conn) = &mut tab.connection {
                conn.bytes_received += bytes_rx;
                conn.bytes_sent += bytes_tx;
                conn.rtt_ticks = rtt;
            }
            self.stats.bytes_received += bytes_rx;
            self.stats.bytes_sent += bytes_tx;
        }
    }

    /// Connection lost — invoke Law 10 offline fallback.
    pub fn connection_lost(&mut self, silo_id: u64) -> Option<[u8; 32]> {
        self.stats.offline_fallbacks += 1;
        self.tabs.get_mut(&silo_id)?.go_offline()
    }

    /// Scroll a tab's viewport.
    pub fn scroll(&mut self, silo_id: u64, dx: f32, dy: f32) {
        if let Some(tab) = self.tabs.get_mut(&silo_id) {
            tab.scroll[0] += dx * tab.zoom;
            tab.scroll[1] += dy * tab.zoom;
        }
    }

    /// Set zoom level on a tab.
    pub fn set_zoom(&mut self, silo_id: u64, zoom: f32) {
        if let Some(tab) = self.tabs.get_mut(&silo_id) {
            tab.zoom = zoom.max(0.25).min(4.0);
            crate::serial_println!("[Q-VIEW BROWSER] Silo {} zoom={:.1}×", silo_id, tab.zoom);
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Q-View Browser (§5.4)              ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Active tabs:   {:>6}                ║", self.tabs.len());
        crate::serial_println!("║ Pages loaded:  {:>6}                ║", self.stats.pages_loaded);
        crate::serial_println!("║ Widgets rcvd:  {:>6}K               ║", self.stats.widgets_received / 1000);
        crate::serial_println!("║ Offline hits:  {:>6}                ║", self.stats.offline_fallbacks);
        crate::serial_println!("║ RX: {:>8} KB                    ║", self.stats.bytes_received / 1024);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
