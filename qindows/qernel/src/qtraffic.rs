//! # Q-Traffic — Network Traffic Flow Visualizer & Law 7 Telemetry Monitor (Phase 69)
//!
//! Q-Traffic implements Q-Manifest **Law 7: Telemetry Transparency**.
//!
//! ## ARCHITECTURE.md §5 + Q-Manifest Law 7
//! > "No network egress without `NET_SEND` token; user sees live Traffic Flow visualizer."
//! > Every byte that crosses the machine boundary must be:
//! >   a) Authorized by a `NET_SEND` CapToken
//! >   b) Visible in real-time via the Q-Traffic dashboard
//!
//! ## Architecture Guardian: Layering
//! ```text
//! Q-Fabric (qfabric.rs)          ← actual QUIC transport
//!     │ reports FlowEvent to ↓
//! Q-Traffic (this module)         ← per-Silo accounting + Law 7 enforcement
//!     │ exposes TrafficSnapshot to ↓
//! Aether (aether.rs)              ← renders Traffic Flow dashboard
//! ```
//!
//! This module does NOT open sockets or move bytes — it only **accounts**.
//! The Sentinel calls `check_law7()` before any Q-Fabric send.
//!
//! ## Q-Manifest Law 7 Enforcement
//! - Any Silo attempting to send without a `NET_SEND` token → VAPORIZE
//! - Any Silo exceeding its rate limit → token stripped (not vaporized)
//! - Covert channel detection: uniform-payload entropy spike → quarantine

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use alloc::string::ToString;

// ── Flow Event ────────────────────────────────────────────────────────────────

/// Direction of a network flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowDirection {
    Egress,  // outbound
    Ingress, // inbound
}

/// Protocol of a network flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowProto {
    Quic,
    QFabric,   // Qindows-native mesh
    Dns,
    Other(u8),
}

/// A single observed network flow event from Q-Fabric.
#[derive(Debug, Clone)]
pub struct FlowEvent {
    /// Generating Silo
    pub silo_id: u64,
    /// Kernel tick when this flow was recorded
    pub tick: u64,
    /// Direction
    pub direction: FlowDirection,
    /// Bytes in this event
    pub bytes: u64,
    /// Protocol
    pub proto: FlowProto,
    /// Is this traffic encrypted?
    pub encrypted: bool,
    /// Destination (UNS URI or IP string — anonymized in Ghost tier)
    pub destination: String,
    /// Shannon entropy of payload (0.0–8.0) — high entropy = possibly covert channel
    pub payload_entropy: f32,
}

// ── Per-Silo Traffic Account ──────────────────────────────────────────────────

/// Rolling traffic statistics for one Silo.
#[derive(Debug, Clone, Default)]
pub struct SiloTrafficAccount {
    pub silo_id: u64,
    /// Total bytes sent (lifetime)
    pub egress_bytes_total: u64,
    /// Total bytes received (lifetime)
    pub ingress_bytes_total: u64,
    /// Bytes sent in the current 1-second window
    pub egress_bytes_window: u64,
    /// Rate limit in bytes/sec (0 = unlimited, set at CapToken grant)
    pub rate_limit_bps: u64,
    /// Number of rate-limit violations
    pub rate_violations: u64,
    /// Number of times this Silo's NET_SEND token was stripped
    pub token_strips: u64,
    /// Last 16 flow events (ring buffer for dashboard)
    pub recent_flows: Vec<FlowEvent>,
    /// Has this Silo sent any unencrypted traffic?
    pub unencrypted_seen: bool,
    /// High-entropy event count (potential covert channel)
    pub entropy_spikes: u64,
    /// Human-readable app ID (from Q-Ledger, populated on first flow)
    pub app_label: Option<String>,
}

impl SiloTrafficAccount {
    pub fn new(silo_id: u64) -> Self {
        SiloTrafficAccount { silo_id, ..Default::default() }
    }

    /// Record a flow event, updating rolling stats.
    pub fn record_flow(&mut self, ev: FlowEvent) {
        match ev.direction {
            FlowDirection::Egress  => {
                self.egress_bytes_total  += ev.bytes;
                self.egress_bytes_window += ev.bytes;
            }
            FlowDirection::Ingress => self.ingress_bytes_total += ev.bytes,
        }
        if !ev.encrypted { self.unencrypted_seen = true; }
        if ev.payload_entropy > 7.5 { self.entropy_spikes += 1; }

        // Keep ring buffer of 16 most recent flows
        if self.recent_flows.len() >= 16 { self.recent_flows.remove(0); }
        self.recent_flows.push(ev);
    }

    /// Reset the 1-second sliding window (called by timer at each tick boundary).
    pub fn reset_window(&mut self) {
        self.egress_bytes_window = 0;
    }

    /// Check if this tick's window has exceeded the rate limit.
    pub fn is_rate_exceeded(&self) -> bool {
        self.rate_limit_bps > 0 && self.egress_bytes_window > self.rate_limit_bps
    }
}

// ── Law 7 Verdict ─────────────────────────────────────────────────────────────

/// Result of a Law 7 pre-send check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Law7Verdict {
    /// Traffic is authorized — proceed
    Allow,
    /// Missing NET_SEND token — Sentinel must VAPORIZE the Silo
    VaporizeNoToken,
    /// Rate limit exceeded — strip NET_SEND token (no vaporize)
    StripToken,
    /// Covert channel detected (entropy spike) — quarantine + alert
    QuarantineCovert,
}

// ── Traffic Snapshot ──────────────────────────────────────────────────────────

/// A snapshot of the entire-system traffic for the Aether dashboard.
#[derive(Debug, Clone, Default)]
pub struct TrafficSnapshot {
    /// Per-Silo rolling egress bytes (last second)
    pub silo_egress_bps: BTreeMap<u64, u64>,
    /// Total system egress bytes/sec
    pub total_egress_bps: u64,
    /// Total system ingress bytes/sec
    pub total_ingress_bps: u64,
    /// Number of Silos with active network flows
    pub active_silos: u64,
    /// Global unencrypted-traffic alert flag
    pub unencrypted_alert: bool,
    /// Number of covert-channel quarantines this session
    pub covert_quarantines: u64,
}

// ── Q-Traffic Engine ──────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TrafficStats {
    pub total_flows_recorded: u64,
    pub law7_allows: u64,
    pub law7_vaporizations: u64,
    pub law7_token_strips: u64,
    pub law7_quarantines: u64,
}

/// The Q-Traffic kernel accounting engine (Law 7 enforcer).
pub struct QTrafficEngine {
    /// Per-Silo traffic accounts
    pub accounts: BTreeMap<u64, SiloTrafficAccount>,
    /// Set of Silo IDs that currently hold a valid NET_SEND token
    pub net_send_authorized: BTreeMap<u64, ()>,
    /// Global stats
    pub stats: TrafficStats,
    /// Covert channel entropy spike threshold
    pub entropy_quarantine_threshold: u64, // strikes before quarantine
}

impl QTrafficEngine {
    pub fn new() -> Self {
        QTrafficEngine {
            accounts: BTreeMap::new(),
            net_send_authorized: BTreeMap::new(),
            stats: TrafficStats::default(),
            entropy_quarantine_threshold: 3,
        }
    }

    /// Called when a Silo is granted a NET_SEND CapToken.
    pub fn authorize_silo(&mut self, silo_id: u64, rate_limit_bps: u64) {
        self.net_send_authorized.insert(silo_id, ());
        let account = self.accounts.entry(silo_id).or_insert_with(|| SiloTrafficAccount::new(silo_id));
        account.rate_limit_bps = rate_limit_bps;
        crate::serial_println!(
            "[TRAFFIC] Silo {} authorized for NET_SEND (rate={}bps).", silo_id, rate_limit_bps
        );
    }

    /// Called when a Silo's NET_SEND token is revoked.
    pub fn revoke_silo(&mut self, silo_id: u64) {
        self.net_send_authorized.remove(&silo_id);
        if let Some(acc) = self.accounts.get_mut(&silo_id) {
            acc.token_strips += 1;
        }
        crate::serial_println!("[TRAFFIC] Silo {} NET_SEND token REVOKED.", silo_id);
    }

    /// Pre-send Law 7 check — called by Q-Fabric before any egress.
    pub fn check_law7(&self, silo_id: u64) -> Law7Verdict {
        // Must have a NET_SEND token
        if !self.net_send_authorized.contains_key(&silo_id) {
            return Law7Verdict::VaporizeNoToken;
        }
        if let Some(acc) = self.accounts.get(&silo_id) {
            // Rate limit exceeded
            if acc.is_rate_exceeded() {
                return Law7Verdict::StripToken;
            }
            // Covert channel detection
            if acc.entropy_spikes >= self.entropy_quarantine_threshold {
                return Law7Verdict::QuarantineCovert;
            }
        }
        Law7Verdict::Allow
    }

    /// Record a flow event (called by Q-Fabric on every send/receive).
    pub fn record_flow(&mut self, ev: FlowEvent) {
        self.stats.total_flows_recorded += 1;
        let silo_id = ev.silo_id;
        let account = self.accounts.entry(silo_id).or_insert_with(|| SiloTrafficAccount::new(silo_id));
        account.record_flow(ev);
    }

    /// Reset all per-second windows (called by 1Hz timer interrupt).
    pub fn tick_window_reset(&mut self) {
        for acc in self.accounts.values_mut() {
            acc.reset_window();
        }
    }

    /// Build a snapshot for the Aether Traffic Flow dashboard.
    pub fn snapshot(&self) -> TrafficSnapshot {
        let mut snap = TrafficSnapshot::default();
        for (silo_id, acc) in &self.accounts {
            if acc.egress_bytes_window > 0 || acc.ingress_bytes_total > 0 {
                snap.active_silos += 1;
                snap.silo_egress_bps.insert(*silo_id, acc.egress_bytes_window);
                snap.total_egress_bps  += acc.egress_bytes_window;
                snap.total_ingress_bps += acc.ingress_bytes_total;
            }
            if acc.unencrypted_seen { snap.unencrypted_alert = true; }
        }
        snap.covert_quarantines = self.stats.law7_quarantines;
        snap
    }

    /// Remove a Silo's account on vaporization.
    pub fn remove_silo(&mut self, silo_id: u64) {
        self.accounts.remove(&silo_id);
        self.net_send_authorized.remove(&silo_id);
    }
}
