//! # Silo Events — Lifecycle Event Bus (Phase 85)
//!
//! ARCHITECTURE.md §2 + §7:
//! > "Qernel manages Silo lifecycle — spawn, vaporize, snapshot, migrate"
//! > "Sentinel monitors every Silo"
//!
//! ## Architecture Guardian: Why an event bus?
//! Currently, Silo lifecycle transitions trigger ad-hoc calls to individual modules.
//! As Qindows grows (now at Phase 85), the call graph has become:
//!
//! ```text
//! Silo spawn  ──► black_box::register_silo()
//!              ──► active_task::register_silo()
//!              ──► q_manifest_enforcer::... (future)
//!              ──► compute_auction::... (future)
//!              ──► telemetry::... (future)
//! ```
//!
//! This creates **tight coupling** — every new module that cares about Silo lifecycle
//! must be manually wired into the spawn/vaporize paths. This violates the Architecture
//! Guardian principle of **loose coupling**.
//!
//! **Solution**: A lightweight publish-subscribe event bus.
//! Any module registers a handler. Silo lifecycle events are published once.
//! No module needs to know which others are listening.
//!
//! ```text
//! SiloManager::spawn_silo(oid) → SiloEventBus::publish(SiloEvent::Spawned { silo_id, binary_oid })
//!                                     │  route to registered handlers
//!                                     ├──► BlackBoxRecorder::on_silo_spawned()
//!                                     ├──► ActiveTaskManager::on_silo_spawned()
//!                                     ├──► QManifestEnforcer::on_silo_spawned()
//!                                     └──► (any future module, zero changes to SiloManager)
//! ```
//!
//! ## Event Types
//! - `Spawned`: binary launched, Silo allocated, CR3 assigned
//! - `Vaporized`: terminated (cause attached)
//! - `Suspended`: deep-sleeping (Law 8, or user minimized)
//! - `Resumed`: woken from sleep
//! - `Migrated`: Silo's compute context moved to Q-Server (FiberOffload)
//! - `Snapshotted`: CoW checkpoint saved (q_silo_fork.rs)
//! - `CapGranted` / `CapRevoked`: capability token lifecycle events

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Event Payloads ────────────────────────────────────────────────────────────

/// A Silo lifecycle event.
#[derive(Debug, Clone)]
pub enum SiloEvent {
    /// Silo successfully spawned and running
    Spawned {
        silo_id: u64,
        binary_oid: [u8; 32],
        spawn_tick: u64,
        /// Initial capability set
        initial_caps: Vec<u64>,
        /// Parent Silo ID (None = kernel-direct spawn)
        parent_silo: Option<u64>,
    },
    /// Silo forcibly terminated
    Vaporized {
        silo_id: u64,
        tick: u64,
        cause: VaporizeCause,
        /// If a Post-Mortem Object was generated, its OID
        post_mortem_oid: Option<[u8; 32]>,
    },
    /// Silo Fibers deep-sleeping (Law 8 or user suspend)
    Suspended {
        silo_id: u64,
        tick: u64,
        reason: SuspendReason,
    },
    /// Silo woken from deep-sleep
    Resumed {
        silo_id: u64,
        tick: u64,
        suspended_for_ticks: u64,
    },
    /// Silo's compute context migrated to Q-Server (FiberOffload)
    Migrated {
        silo_id: u64,
        fiber_id: u64,
        server_node_id: u64,
        tick: u64,
    },
    /// FiberOffload returned — Silo running locally again
    Recalled {
        silo_id: u64,
        fiber_id: u64,
        tick: u64,
    },
    /// CoW fork created (q_silo_fork.rs)
    Forked {
        parent_silo_id: u64,
        child_silo_id: u64,
        tick: u64,
    },
    /// Capability granted to Silo
    CapGranted {
        silo_id: u64,
        cap_id: u64,
        cap_name: String,
        granted_by: u64,
        tick: u64,
    },
    /// Capability revoked from Silo
    CapRevoked {
        silo_id: u64,
        cap_id: u64,
        cap_name: String,
        revoked_by: u64,
        tick: u64,
        reason: String,
    },
}

/// Reason for Silo vaporization (mirrors black_box::VaporizationCause for bus use).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaporizeCause {
    LawViolation,
    PageFault,
    MemoryExhaustion,
    StackOverflow,
    SpinLoop,
    BinaryTampered,
    ExplicitTermination,
    UserRequested,
    SentinelAnomaly,
}

impl VaporizeCause {
    pub fn label(self) -> &'static str {
        match self {
            Self::LawViolation       => "Law Violation",
            Self::PageFault          => "Unhandled Page Fault",
            Self::MemoryExhaustion   => "Out of Memory",
            Self::StackOverflow      => "Stack Overflow",
            Self::SpinLoop           => "Spin Loop (Law 3/8)",
            Self::BinaryTampered     => "Binary Tampered (Law 2)",
            Self::ExplicitTermination => "Terminated by Peer",
            Self::UserRequested      => "User Requested",
            Self::SentinelAnomaly    => "Sentinel AI Anomaly",
        }
    }
}

/// Reason for Silo suspension.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuspendReason {
    Law8NoActiveTask,       // Law 8: no ActiveTask token
    UserMinimized,          // User minimized the window
    QViewBackgroundTab,     // Q-View tab moved to background
    FiberOffloadPending,    // Waiting for FiberOffload migration
    LowBattery,             // Power governor triggered
}

impl SiloEvent {
    /// Extract the Silo ID from any event variant.
    pub fn silo_id(&self) -> u64 {
        match self {
            Self::Spawned { silo_id, .. }    => *silo_id,
            Self::Vaporized { silo_id, .. }  => *silo_id,
            Self::Suspended { silo_id, .. }  => *silo_id,
            Self::Resumed { silo_id, .. }    => *silo_id,
            Self::Migrated { silo_id, .. }   => *silo_id,
            Self::Recalled { silo_id, .. }   => *silo_id,
            Self::Forked { parent_silo_id, .. } => *parent_silo_id,
            Self::CapGranted { silo_id, .. } => *silo_id,
            Self::CapRevoked { silo_id, .. } => *silo_id,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Spawned { .. }   => "Spawned",
            Self::Vaporized { .. } => "Vaporized",
            Self::Suspended { .. } => "Suspended",
            Self::Resumed { .. }   => "Resumed",
            Self::Migrated { .. }  => "Migrated",
            Self::Recalled { .. }  => "Recalled",
            Self::Forked { .. }    => "Forked",
            Self::CapGranted { .. }=> "CapGranted",
            Self::CapRevoked { .. }=> "CapRevoked",
        }
    }
}

// ── Handler ───────────────────────────────────────────────────────────────────

/// Handler ID type (used to unsubscribe).
pub type HandlerId = u32;

/// Event filter: which event variants a handler subscribes to.
#[derive(Debug, Clone, Copy)]
pub enum EventFilter {
    /// Receive all events
    All,
    /// Only Spawned events
    Spawned,
    /// Only Vaporized events
    Vaporized,
    /// Only CapGranted + CapRevoked events
    Capabilities,
    /// Only suspend/resume events (Law 8 monitoring)
    PowerState,
    /// Only migrate/recall events (FiberOffload)
    Offload,
}

impl EventFilter {
    pub fn matches(&self, event: &SiloEvent) -> bool {
        match self {
            Self::All        => true,
            Self::Spawned    => matches!(event, SiloEvent::Spawned { .. }),
            Self::Vaporized  => matches!(event, SiloEvent::Vaporized { .. }),
            Self::Capabilities => matches!(event,
                SiloEvent::CapGranted { .. } | SiloEvent::CapRevoked { .. }),
            Self::PowerState => matches!(event,
                SiloEvent::Suspended { .. } | SiloEvent::Resumed { .. }),
            Self::Offload    => matches!(event,
                SiloEvent::Migrated { .. } | SiloEvent::Recalled { .. }),
        }
    }
}

/// A registered handler entry.
struct HandlerEntry {
    pub id: HandlerId,
    pub filter: EventFilter,
    pub label: String, // for debugging
    /// Queued events for this handler (handler polls asynchronously)
    pub queue: VecDeque<SiloEvent>,
    pub max_queue: usize,
}

use alloc::collections::VecDeque;

impl HandlerEntry {
    fn new(id: HandlerId, filter: EventFilter, label: &str) -> Self {
        HandlerEntry {
            id,
            filter,
            label: label.to_string(),
            queue: VecDeque::new(),
            max_queue: 64,
        }
    }

    fn deliver(&mut self, event: &SiloEvent) {
        if self.filter.matches(event) {
            if self.queue.len() >= self.max_queue { self.queue.pop_front(); }
            self.queue.push_back(event.clone());
        }
    }
}

// ── Silo Registry ─────────────────────────────────────────────────────────────

/// Runtime state of a known Silo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiloState {
    Running,
    Suspended,
    Migrated,
    Vaporized,
}

#[derive(Debug, Clone)]
pub struct SiloRecord {
    pub silo_id: u64,
    pub binary_oid: [u8; 32],
    pub state: SiloState,
    pub spawn_tick: u64,
    pub parent_silo: Option<u64>,
}

// ── Event Bus Statistics ──────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct EventBusStats {
    pub events_published: u64,
    pub events_delivered: u64,
    pub spawned: u64,
    pub vaporized: u64,
    pub suspended: u64,
    pub resumed: u64,
    pub migrated: u64,
    pub cap_events: u64,
}

// ── Silo Event Bus ────────────────────────────────────────────────────────────

/// Lightweight publish-subscribe Silo lifecycle event bus.
pub struct SiloEventBus {
    /// Registered handlers: handler_id → entry
    handlers: BTreeMap<HandlerId, HandlerEntry>,
    next_handler_id: HandlerId,
    /// Active Silo registry: silo_id → record
    pub silos: BTreeMap<u64, SiloRecord>,
    /// Statistics
    pub stats: EventBusStats,
}

impl SiloEventBus {
    pub fn new() -> Self {
        SiloEventBus {
            handlers: BTreeMap::new(),
            next_handler_id: 1,
            silos: BTreeMap::new(),
            stats: EventBusStats::default(),
        }
    }

    // ── Subscription Management ───────────────────────────────────────────────

    /// Register a handler for a class of events. Returns HandlerId for polling / unsubscribing.
    pub fn subscribe(&mut self, filter: EventFilter, label: &str) -> HandlerId {
        let id = self.next_handler_id;
        self.next_handler_id += 1;
        self.handlers.insert(id, HandlerEntry::new(id, filter, label));
        crate::serial_println!("[SILO BUS] Handler {} registered: \"{}\" filter={:?}", id, label, filter);
        id
    }

    /// Unsubscribe a handler.
    pub fn unsubscribe(&mut self, id: HandlerId) {
        self.handlers.remove(&id);
    }

    // ── Publishing ────────────────────────────────────────────────────────────

    /// Publish a Silo lifecycle event to all matching handlers.
    pub fn publish(&mut self, event: SiloEvent) {
        self.stats.events_published += 1;

        // Update Silo registry
        self.update_registry(&event);

        // Log the event
        crate::serial_println!(
            "[SILO BUS] Event: {} (silo={})", event.name(), event.silo_id()
        );

        // Update stats
        match &event {
            SiloEvent::Spawned { .. }   => self.stats.spawned += 1,
            SiloEvent::Vaporized { .. } => self.stats.vaporized += 1,
            SiloEvent::Suspended { .. } => self.stats.suspended += 1,
            SiloEvent::Resumed { .. }   => self.stats.resumed += 1,
            SiloEvent::Migrated { .. } | SiloEvent::Recalled { .. } => self.stats.migrated += 1,
            SiloEvent::CapGranted { .. } | SiloEvent::CapRevoked { .. } => self.stats.cap_events += 1,
            _ => {}
        }

        // Deliver to all matching handlers
        let mut delivered = 0u64;
        for handler in self.handlers.values_mut() {
            handler.deliver(&event);
            delivered += 1;
        }
        self.stats.events_delivered += delivered;
    }

    // ── Polling ────────────────────────────────────────────────────────────────

    /// Poll for new events (non-blocking). Returns queued events for this handler.
    pub fn poll(&mut self, handler_id: HandlerId) -> Vec<SiloEvent> {
        self.handlers.get_mut(&handler_id)
            .map(|h| h.queue.drain(..).collect())
            .unwrap_or_default()
    }

    // ── Direct Silo State Queries ─────────────────────────────────────────────

    /// Get current state of a Silo.
    pub fn silo_state(&self, silo_id: u64) -> Option<SiloState> {
        self.silos.get(&silo_id).map(|r| r.state)
    }

    /// All currently running Silos.
    pub fn running_silos(&self) -> Vec<u64> {
        self.silos.values()
            .filter(|r| r.state == SiloState::Running)
            .map(|r| r.silo_id)
            .collect()
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn update_registry(&mut self, event: &SiloEvent) {
        match event {
            SiloEvent::Spawned { silo_id, binary_oid, spawn_tick, parent_silo, .. } => {
                self.silos.insert(*silo_id, SiloRecord {
                    silo_id: *silo_id,
                    binary_oid: *binary_oid,
                    state: SiloState::Running,
                    spawn_tick: *spawn_tick,
                    parent_silo: *parent_silo,
                });
            }
            SiloEvent::Vaporized { silo_id, .. } => {
                if let Some(r) = self.silos.get_mut(silo_id) { r.state = SiloState::Vaporized; }
            }
            SiloEvent::Suspended { silo_id, .. } => {
                if let Some(r) = self.silos.get_mut(silo_id) { r.state = SiloState::Suspended; }
            }
            SiloEvent::Resumed { silo_id, .. } => {
                if let Some(r) = self.silos.get_mut(silo_id) { r.state = SiloState::Running; }
            }
            SiloEvent::Migrated { silo_id, .. } => {
                if let Some(r) = self.silos.get_mut(silo_id) { r.state = SiloState::Migrated; }
            }
            SiloEvent::Recalled { silo_id, .. } => {
                if let Some(r) = self.silos.get_mut(silo_id) { r.state = SiloState::Running; }
            }
            _ => {}
        }
    }
}
