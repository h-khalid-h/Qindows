//! # Intent Router — Complete Q-Synapse Neural Intent Pipeline (Phase 79)
//!
//! ARCHITECTURE.md §6.2 — Q-Synapse BCI Neural Pipeline:
//!
//! ```text
//! BCI Hardware (EEG / Implant)
//!      │  raw microvolt stream
//!      ▼
//! SignalPipeline: denoise → NPU embed → classify          (synapse.rs Phase 60)
//!      │  NeuralPattern (256-bit hash + confidence)
//!      ▼
//! NeuralBindingTable: pattern_hash → IntentCategory       (synapse.rs Phase 60)
//!      │  matched binding (confidence ≥ threshold)
//!      ▼
//! ThoughtGate: double-tap mental handshake (2s window)    (synapse.rs Phase 60)
//!      │  confirmed intent
//!      ▼
//! IntentEvent → Q-Shell / Aether executes action          ← THIS MODULE (Phase 79)
//! ```
//!
//! ## Architecture Guardian: What was missing
//! `synapse.rs` (Phase 60) correctly classifies a `NeuralPattern` into an `IntentCategory`
//! and confirms it via ThoughtGate. But the final step — routing a confirmed IntentEvent
//! to the correct kernel subsystem — was a stub (`"genesis"` string match only).
//!
//! This module provides the full **intent → action** dispatch layer:
//! - Navigate → Q-View (Phase 74) navigate tab
//! - Execute → Q-Shell (Phase 66) new pipeline stage
//! - OpenShell → spawn Q-Shell Silo
//! - Focus → Aether window focus (scene graph)
//! - Dismiss → vaporize / minimize Silo
//! - Pivot → Prism search (Phase 72) for related content
//! - Abort → Sentinel: terminate current Silo
//! - Custom → user-registered intent bindings
//!
//! ## Privacy (Q-Manifest, immutable)
//! The IntentRouter receives **ONLY** a de-personalized `IntentCategory` + confidence.
//! No raw neural data, no thought content, no personality info ever reaches here.
//! This boundary is enforced by `synapse.rs` — the IntentRouter cannot request more.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::format;

// ── Intent Categories ─────────────────────────────────────────────────────────

/// Confirmed intent categories from Q-Synapse ThoughtGate.
/// Matches ARCHITECTURE.md §6.2 exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntentCategory {
    /// Navigate to a location (URL, Prism OID, UNS URI)
    Navigate,
    /// Focus on a specific window or Silo
    Focus,
    /// Execute a Q-Shell command or pipeline
    Execute,
    /// Dismiss / minimize / close the current focused Silo
    Dismiss,
    /// Pivot — search for contextually related content via Prism
    Pivot,
    /// Open a new Q-Shell terminal
    OpenShell,
    /// Abort the current operation (Sentinel: graceful stop)
    Abort,
    /// User-registered custom intent binding
    Custom(u32),
}

impl IntentCategory {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Navigate  => "Navigate",
            Self::Focus     => "Focus",
            Self::Execute   => "Execute",
            Self::Dismiss   => "Dismiss",
            Self::Pivot     => "Pivot",
            Self::OpenShell => "OpenShell",
            Self::Abort     => "Abort",
            Self::Custom(_) => "Custom",
        }
    }
}

// ── Intent Event ──────────────────────────────────────────────────────────────

/// A fully confirmed intent from the ThoughtGate.
#[derive(Debug, Clone)]
pub struct IntentEvent {
    /// Category of the intent
    pub category: IntentCategory,
    /// Confidence score from ThoughtGate (0-100; only events ≥75 reach the router)
    pub confidence: u8,
    /// Originating Silo (Q-Synapse's Silo ID)
    pub silo_id: u64,
    /// Context data (e.g. hovered object OID, current URL, clipboard preview)
    pub context: IntentContext,
    /// Kernel tick when ThoughtGate was confirmed
    pub confirmed_at: u64,
    /// Whether the user performed the "double-tap" handshake (always true here)
    pub double_confirmed: bool,
}

/// Context attached to an intent — helps the router disambiguate.
#[derive(Debug, Clone, Default)]
pub struct IntentContext {
    /// OID of the currently focused Prism object (if any)
    pub focused_oid: Option<[u8; 32]>,
    /// Currently focused Silo ID (Aether's focus window)
    pub focused_silo: Option<u64>,
    /// Current clipboard content (hash only — never raw text for privacy)
    pub clipboard_hash: Option<[u8; 32]>,
    /// Most recent Q-Shell pipeline text
    pub shell_context: Option<String>,
    /// UNS URI under cursor (for Navigate via hover)
    pub hovered_uri: Option<String>,
}

// ── Dispatch Action ───────────────────────────────────────────────────────────

/// What the IntentRouter decided to do with the event.
#[derive(Debug, Clone)]
pub enum DispatchAction {
    /// Navigate Q-View tab to this UNS URI
    QViewNavigate { tab_id: u64, uri: String },
    /// Focus a specific Silo in Aether
    AetherFocus { silo_id: u64 },
    /// Execute a Q-Shell command in a Silo
    QShellExecute { silo_id: u64, command: String },
    /// Spawn a new Q-Shell Silo
    SpawnShell,
    /// Minimize/close focused Silo
    DismissSilo { silo_id: u64 },
    /// Trigger Prism semantic search with context query
    PrismPivot { query: String, silo_id: u64 },
    /// Abort current Sentinel-graceful stop
    AbortCurrentSilo { silo_id: u64 },
    /// Invoke a custom registered handler
    CustomHandler { binding_id: u32 },
    /// No action taken (confidence too low, or no registered handler)
    NoOp { reason: String },
}

// ── Custom Binding ────────────────────────────────────────────────────────────

/// A user-registered custom intent binding.
#[derive(Debug, Clone)]
pub struct CustomBinding {
    pub binding_id: u32,
    /// Label shown in Aether's binding manager UI
    pub label: String,
    /// The Q-Shell pipeline to execute when this binding fires
    pub shell_command: String,
    /// Which Silo to execute it in (System Silo = 1 if not specified)
    pub target_silo: u64,
    /// Times this binding has been fired
    pub fire_count: u64,
}

// ── Router Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct IntentRouterStats {
    pub total_events: u64,
    pub navigate_dispatched: u64,
    pub execute_dispatched: u64,
    pub focus_dispatched: u64,
    pub dismiss_dispatched: u64,
    pub pivot_dispatched: u64,
    pub shell_spawned: u64,
    pub abort_dispatched: u64,
    pub custom_dispatched: u64,
    pub low_confidence_dropped: u64,
    pub no_context_noops: u64,
}

// ── Intent Router ─────────────────────────────────────────────────────────────

/// The kernel Intent → Action dispatch layer.
/// Connects Q-Synapse ThoughtGate output to Qindows subsystems.
pub struct IntentRouter {
    /// Custom bindings registered by the user: binding_id → CustomBinding
    pub custom_bindings: BTreeMap<u32, CustomBinding>,
    /// Next custom binding ID
    next_binding_id: u32,
    /// Minimum confidence to act on (events below this are silently dropped)
    pub confidence_threshold: u8,
    /// History of the last 64 dispatched events + actions (for Aether's Intent Log)
    pub history: Vec<(IntentEvent, DispatchAction)>,
    /// Max history size
    pub max_history: usize,
    /// Statistics
    pub stats: IntentRouterStats,
    /// Active Q-View tab (for Navigate dispatch)
    pub active_view_tab: Option<u64>,
    /// Currently focused Silo in Aether
    pub focused_silo: Option<u64>,
    /// Q-Shell Silo ID
    pub shell_silo_id: u64,
}

impl IntentRouter {
    pub fn new(shell_silo_id: u64) -> Self {
        IntentRouter {
            custom_bindings: BTreeMap::new(),
            next_binding_id: 1,
            confidence_threshold: 75,
            history: Vec::new(),
            max_history: 64,
            stats: IntentRouterStats::default(),
            active_view_tab: None,
            focused_silo: None,
            shell_silo_id,
        }
    }

    // ── Core Dispatch ─────────────────────────────────────────────────────────

    /// Route a confirmed IntentEvent to the appropriate kernel subsystem.
    ///
    /// This is the function called by `synapse.rs` after ThoughtGate confirmation.
    pub fn dispatch(&mut self, event: IntentEvent) -> DispatchAction {
        self.stats.total_events += 1;

        // Confidence gate
        if event.confidence < self.confidence_threshold {
            self.stats.low_confidence_dropped += 1;
            let action = DispatchAction::NoOp {
                reason: format!("confidence {} < threshold {}", event.confidence, self.confidence_threshold),
            };
            crate::serial_println!(
                "[INTENT] Dropped: {:?} confidence={} < {}",
                event.category, event.confidence, self.confidence_threshold
            );
            return action;
        }

        let action = match event.category {
            IntentCategory::Navigate  => self.dispatch_navigate(&event),
            IntentCategory::Focus     => self.dispatch_focus(&event),
            IntentCategory::Execute   => self.dispatch_execute(&event),
            IntentCategory::Dismiss   => self.dispatch_dismiss(&event),
            IntentCategory::Pivot     => self.dispatch_pivot(&event),
            IntentCategory::OpenShell => self.dispatch_open_shell(),
            IntentCategory::Abort     => self.dispatch_abort(&event),
            IntentCategory::Custom(id) => self.dispatch_custom(id),
        };

        crate::serial_println!(
            "[INTENT] Dispatched: {:?} → {:?} (confidence={})",
            event.category.name(), core::mem::discriminant(&action), event.confidence
        );

        // Record history
        if self.history.len() >= self.max_history { self.history.remove(0); }
        self.history.push((event, action.clone()));

        action
    }

    // ── Per-Category Dispatch Handlers ────────────────────────────────────────

    fn dispatch_navigate(&mut self, event: &IntentEvent) -> DispatchAction {
        self.stats.navigate_dispatched += 1;
        let uri = event.context.hovered_uri.clone()
            .unwrap_or_else(|| "prism://search:recent".to_string());
        let tab_id = self.active_view_tab.unwrap_or(1);
        crate::serial_println!("[INTENT] Navigate → QView tab={} uri={}", tab_id, uri);
        DispatchAction::QViewNavigate { tab_id, uri }
    }

    fn dispatch_focus(&mut self, event: &IntentEvent) -> DispatchAction {
        self.stats.focus_dispatched += 1;
        let silo_id = event.context.focused_silo.unwrap_or_else(|| {
            // Default: focus the Silo that generated the intent
            event.silo_id
        });
        self.focused_silo = Some(silo_id);
        crate::serial_println!("[INTENT] Focus → Aether focus silo={}", silo_id);
        DispatchAction::AetherFocus { silo_id }
    }

    fn dispatch_execute(&mut self, event: &IntentEvent) -> DispatchAction {
        self.stats.execute_dispatched += 1;
        let command = event.context.shell_context.clone()
            .unwrap_or_else(|| "q_shell help".to_string());
        let silo_id = self.shell_silo_id;
        crate::serial_println!("[INTENT] Execute → QShell silo={} cmd=\"{}\"", silo_id, command);
        DispatchAction::QShellExecute { silo_id, command }
    }

    fn dispatch_dismiss(&mut self, event: &IntentEvent) -> DispatchAction {
        self.stats.dismiss_dispatched += 1;
        let silo_id = self.focused_silo.unwrap_or(event.silo_id);
        crate::serial_println!("[INTENT] Dismiss → silo={}", silo_id);
        DispatchAction::DismissSilo { silo_id }
    }

    fn dispatch_pivot(&mut self, event: &IntentEvent) -> DispatchAction {
        self.stats.pivot_dispatched += 1;
        // Build a semantic pivot query from the focused object or shell context
        let query = if let Some(ctx) = &event.context.shell_context {
            format!("related:{}", ctx)
        } else if event.context.focused_oid.is_some() {
            "related:focused_object".to_string()
        } else {
            "recent".to_string()
        };
        crate::serial_println!("[INTENT] Pivot → Prism query=\"{}\"", query);
        self.stats.pivot_dispatched += 0; // already incremented above
        DispatchAction::PrismPivot { query, silo_id: event.silo_id }
    }

    fn dispatch_open_shell(&mut self) -> DispatchAction {
        self.stats.shell_spawned += 1;
        crate::serial_println!("[INTENT] OpenShell → spawning new Q-Shell Silo");
        DispatchAction::SpawnShell
    }

    fn dispatch_abort(&mut self, event: &IntentEvent) -> DispatchAction {
        self.stats.abort_dispatched += 1;
        let silo_id = self.focused_silo.unwrap_or(event.silo_id);
        crate::serial_println!("[INTENT] Abort → Sentinel graceful stop silo={}", silo_id);
        DispatchAction::AbortCurrentSilo { silo_id }
    }

    fn dispatch_custom(&mut self, binding_id: u32) -> DispatchAction {
        self.stats.custom_dispatched += 1;
        if let Some(binding) = self.custom_bindings.get_mut(&binding_id) {
            binding.fire_count += 1;
            crate::serial_println!(
                "[INTENT] Custom binding #{} \"{}\" → cmd=\"{}\"",
                binding_id, binding.label, binding.shell_command
            );
            DispatchAction::CustomHandler { binding_id }
        } else {
            DispatchAction::NoOp { reason: format!("no binding for id={}", binding_id) }
        }
    }

    // ── Custom Binding Management ─────────────────────────────────────────────

    /// Register a custom intent binding (callable from Aether's binding manager).
    pub fn register_custom(
        &mut self,
        label: &str,
        shell_command: &str,
        target_silo: u64,
    ) -> u32 {
        let binding_id = self.next_binding_id;
        self.next_binding_id += 1;
        self.custom_bindings.insert(binding_id, CustomBinding {
            binding_id,
            label: label.to_string(),
            shell_command: shell_command.to_string(),
            target_silo,
            fire_count: 0,
        });
        crate::serial_println!(
            "[INTENT] Custom binding registered: id={} label=\"{}\"", binding_id, label
        );
        binding_id
    }

    /// Unregister a custom binding.
    pub fn unregister_custom(&mut self, binding_id: u32) -> bool {
        self.custom_bindings.remove(&binding_id).is_some()
    }

    // ── Context Updates ───────────────────────────────────────────────────────

    /// Update the active Q-View tab (called by QViewEngine on tab focus).
    pub fn set_active_view_tab(&mut self, tab_id: u64) {
        self.active_view_tab = Some(tab_id);
    }

    /// Update the Aether focused Silo (called by Aether on window focus change).
    pub fn set_focused_silo(&mut self, silo_id: u64) {
        self.focused_silo = Some(silo_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Q-Synapse Intent Router (§6.2)     ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Total events:  {:>6}                 ║", self.stats.total_events);
        crate::serial_println!("║ Navigate:      {:>6}                 ║", self.stats.navigate_dispatched);
        crate::serial_println!("║ Focus:         {:>6}                 ║", self.stats.focus_dispatched);
        crate::serial_println!("║ Execute:       {:>6}                 ║", self.stats.execute_dispatched);
        crate::serial_println!("║ Pivot:         {:>6}                 ║", self.stats.pivot_dispatched);
        crate::serial_println!("║ OpenShell:     {:>6}                 ║", self.stats.shell_spawned);
        crate::serial_println!("║ Custom:        {:>6}                 ║", self.stats.custom_dispatched);
        crate::serial_println!("║ Dropped (low conf): {:>4}            ║", self.stats.low_confidence_dropped);
        crate::serial_println!("║ Custom bindings:{:>5}                ║", self.custom_bindings.len());
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
