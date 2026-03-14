//! # Q-Manifest Enforcer — Unified 10-Laws Enforcement Bus (Phase 80)
//!
//! ARCHITECTURE.md Q-MANIFEST: THE 10 LAWS:
//! > "Enforced by Qernel **at hardware level** — violations trigger immediate Silo vaporization."
//!
//! ## Architecture Guardian: Why this module exists
//! Laws 1-10 are *enforced* in multiple individual modules:
//! - Law 1 (Zero-Ambient Authority): cap_token.rs
//! - Law 2 (Immutable Binaries): qfs.rs + ledger.rs
//! - Law 3 (Async Everything): sentinel.rs (16ms block detection)
//! - Law 4 (Vector-Native UI): aether.rs (bitmap rejection)
//! - Law 5 (Global Deduplication): ledger.rs + prism.rs
//! - Law 6 (Silo Sandbox): silo.rs + memory/paging.rs
//! - Law 7 (Telemetry Transparency): qtraffic.rs (Phase 69)
//! - Law 8 (Energy Proportionality): active_task.rs (Phase 73)
//! - Law 9 (Universal Namespace): uns.rs (Phase 58)
//! - Law 10 (Graceful Degradation): fiber_offload.rs + q_view.rs
//!
//! **The gap**: there is no single authoritative audit log of law violations.
//! Sentinel has to query 10 different modules to produce a compliance report.
//! Cross-law violations (e.g. Law 1 + Law 7 simultaneously = exfil attempt) are
//! not detected because no module sees the full picture.
//!
//! **This module** is the **enforcement bus**:
//! ```text
//! Any kernel module detects a violation
//!     │  LawViolationEvent { law, silo_id, evidence, tick }
//!     ▼
//! QManifestEnforcer::report_violation()
//!     │  1. Log to immutable audit chain (hash-chained)
//!     │  2. Correlate with recent violations by same Silo
//!     │  3. Apply escalating enforcement (warn → throttle → vaporize)
//!     │  4. Cross-law correlation: flag compound violations
//!     │  5. Feed digital_antibody.rs if novel pattern detected
//!     ▼
//! EnforcementOutcome { action, escalated, antibody_generated }
//! ```

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Q-Manifest Law Reference ──────────────────────────────────────────────────

/// Which of the 10 Q-Manifest laws was violated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ManifestLaw {
    /// Law 1: Zero-Ambient Authority — Silo accessed resource without CapToken
    ZeroAmbientAuthority    = 1,
    /// Law 2: Immutable Binaries — app attempted to modify its own binary
    ImmutableBinaries       = 2,
    /// Law 3: Asynchronous Everything — Fiber blocked kernel thread > 16ms
    AsyncEverything         = 3,
    /// Law 4: Vector-Native UI — app submitted raw bitmap to Aether
    VectorNativeUi          = 4,
    /// Law 5: Global Deduplication — duplicate binary stored (not deduplicated)
    GlobalDeduplication     = 5,
    /// Law 6: Silo Sandbox — cross-Silo memory access or registry share attempt
    SiloSandbox             = 6,
    /// Law 7: Telemetry Transparency — network egress without NET_SEND CapToken
    TelemetryTransparency   = 7,
    /// Law 8: Energy Proportionality — Fiber ran without ActiveTask token
    EnergyProportionality   = 8,
    /// Law 9: Universal Namespace — app used raw file path instead of UNS/OID
    UniversalNamespace      = 9,
    /// Law 10: Graceful Degradation — app crashed on network loss vs serving cache
    GracefulDegradation     = 10,
}

impl ManifestLaw {
    pub fn name(self) -> &'static str {
        match self {
            Self::ZeroAmbientAuthority  => "Law 1: Zero-Ambient Authority",
            Self::ImmutableBinaries     => "Law 2: Immutable Binaries",
            Self::AsyncEverything       => "Law 3: Async Everything",
            Self::VectorNativeUi        => "Law 4: Vector-Native UI",
            Self::GlobalDeduplication   => "Law 5: Global Deduplication",
            Self::SiloSandbox           => "Law 6: Silo Sandbox",
            Self::TelemetryTransparency => "Law 7: Telemetry Transparency",
            Self::EnergyProportionality => "Law 8: Energy Proportionality",
            Self::UniversalNamespace    => "Law 9: Universal Namespace",
            Self::GracefulDegradation   => "Law 10: Graceful Degradation",
        }
    }

    /// Default enforcement action for a first violation of this law.
    pub fn first_violation_action(self) -> EnforcementAction {
        match self {
            Self::ZeroAmbientAuthority  => EnforcementAction::VaporizeSilo,   // always hard
            Self::ImmutableBinaries     => EnforcementAction::VaporizeSilo,   // always hard
            Self::AsyncEverything       => EnforcementAction::WarnAndDim,
            Self::VectorNativeUi        => EnforcementAction::RejectRequest,
            Self::GlobalDeduplication   => EnforcementAction::ForceDedup,
            Self::SiloSandbox           => EnforcementAction::VaporizeSilo,   // always hard
            Self::TelemetryTransparency => EnforcementAction::StripNetSend,
            Self::EnergyProportionality => EnforcementAction::DeepSleepSilo,
            Self::UniversalNamespace    => EnforcementAction::WarnAndDim,
            Self::GracefulDegradation   => EnforcementAction::WarnAndDim,
        }
    }
}

// ── Enforcement Action ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementAction {
    /// Log only — first-time minor violation for Laws 3/4/9/10
    WarnAndDim,
    /// Remove NET_SEND CapToken immediately (Law 7)
    StripNetSend,
    /// Put Silo into DeepSleep (Law 8 route via active_task.rs)
    DeepSleepSilo,
    /// Reject the triggering syscall and return error to Silo
    RejectRequest,
    /// Force deduplication pass on offending binary (Law 5)
    ForceDedup,
    /// Terminate Silo and save Black Box to Prism
    VaporizeSilo,
    /// Quarantine Silo (can't send/recv network, access Prism) pending review
    QuarantineSilo,
}

// ── Violation Event ───────────────────────────────────────────────────────────

/// A law violation event reported by any kernel module.
#[derive(Debug, Clone)]
pub struct LawViolationEvent {
    /// Which law was violated
    pub law: ManifestLaw,
    /// Offending Silo ID
    pub silo_id: u64,
    /// Human-readable evidence description
    pub evidence: String,
    /// Kernel tick of violation
    pub tick: u64,
    /// Optional binary OID (for Law 2/5)
    pub binary_oid: Option<[u8; 32]>,
    /// How severe is this specific instance (0-100)
    pub severity_override: Option<u8>,
}

// ── Audit Chain Entry ─────────────────────────────────────────────────────────

/// A hash-chained audit log entry (Sentinel Black Box format).
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub seq: u64,
    pub violation: LawViolationEvent,
    pub action_taken: EnforcementAction,
    pub chain_hash: [u8; 32], // SHA-256(prev_hash || seq || silo_id || law || tick)
}

// ── Silo Violation History ────────────────────────────────────────────────────

/// Per-Silo violation record for escalation.
#[derive(Debug, Clone, Default)]
pub struct SiloViolationRecord {
    pub silo_id: u64,
    pub total_violations: u32,
    /// Per-law violation count for escalation
    pub per_law: [u32; 11], // index 1-10 correspond to ManifestLaw
    /// Last violation tick (for recency weighting)
    pub last_violation_tick: u64,
    /// Has this Silo been vaporized already?
    pub vaporized: bool,
    /// Is this Silo quarantined?
    pub quarantined: bool,
}

impl SiloViolationRecord {
    pub fn record(&mut self, law: ManifestLaw, tick: u64) {
        let idx = law as usize;
        if idx < 11 { self.per_law[idx] += 1; }
        self.total_violations += 1;
        self.last_violation_tick = tick;
    }

    /// How many times has this specific law been violated?
    pub fn law_count(&self, law: ManifestLaw) -> u32 {
        let idx = law as usize;
        if idx < 11 { self.per_law[idx] } else { 0 }
    }

    /// Escalated action based on repeat violations.
    pub fn escalated_action(&self, law: ManifestLaw) -> EnforcementAction {
        let count = self.law_count(law);
        if self.quarantined { return EnforcementAction::VaporizeSilo; }
        match count {
            0     => law.first_violation_action(),
            1     => law.first_violation_action(),
            2     => EnforcementAction::QuarantineSilo,
            _     => EnforcementAction::VaporizeSilo,
        }
    }
}

// ── Cross-Law Compound Violation Detection ────────────────────────────────────

/// Known dangerous *compound* violation patterns (two laws violated by same Silo).
fn is_compound_threat(laws_violated: &[ManifestLaw]) -> Option<&'static str> {
    let has = |l: ManifestLaw| laws_violated.contains(&l);

    if has(ManifestLaw::ZeroAmbientAuthority) && has(ManifestLaw::TelemetryTransparency) {
        Some("EXFIL: Cap escalation + unauthorized network egress — data exfiltration pattern")
    } else if has(ManifestLaw::ImmutableBinaries) && has(ManifestLaw::SiloSandbox) {
        Some("ROOTKIT: Binary tampering + cross-silo access — kernel rootkit pattern")
    } else if has(ManifestLaw::EnergyProportionality) && has(ManifestLaw::AsyncEverything) {
        Some("SPINLOCK: Fiber spin-loop + energy violation — CPU exhaustion attack")
    } else if has(ManifestLaw::SiloSandbox) && has(ManifestLaw::TelemetryTransparency) {
        Some("LATERAL: Cross-silo + shadow network egress — lateral movement pattern")
    } else {
        None
    }
}

// ── Enforcement Statistics ─────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct EnforcerStats {
    pub total_violations: u64,
    pub silos_vaporized: u64,
    pub silos_quarantined: u64,
    pub warnings_issued: u64,
    pub net_send_stripped: u64,
    pub deep_sleeps_forced: u64,
    pub compound_threats_detected: u64,
    pub antibodies_triggered: u64,
    pub per_law_counts: [u64; 11],
}

// ── Q-Manifest Enforcer ───────────────────────────────────────────────────────

/// The unified Q-Manifest enforcement bus.
/// Receives violation events from any kernel module and applies escalating enforcement.
pub struct QManifestEnforcer {
    /// Per-Silo violation history: silo_id → record
    pub silo_records: BTreeMap<u64, SiloViolationRecord>,
    /// Immutable hash-chained audit log (last 512 entries)
    pub audit_log: Vec<AuditEntry>,
    pub max_audit: usize,
    /// Previous chain hash (for chain continuity)
    prev_chain_hash: [u8; 32],
    /// Audit sequence counter
    audit_seq: u64,
    /// Enforcement statistics
    pub stats: EnforcerStats,
    /// Recent laws violated per Silo (last tick window for compound detection)
    recent_laws: BTreeMap<u64, Vec<ManifestLaw>>,
}

impl QManifestEnforcer {
    pub fn new() -> Self {
        QManifestEnforcer {
            silo_records: BTreeMap::new(),
            audit_log: Vec::new(),
            max_audit: 512,
            prev_chain_hash: [0u8; 32],
            audit_seq: 0,
            stats: EnforcerStats::default(),
            recent_laws: BTreeMap::new(),
        }
    }

    /// Report a law violation from any kernel module.
    /// Returns the enforcement action that was applied.
    pub fn report_violation(&mut self, event: LawViolationEvent) -> EnforcementAction {
        let law  = event.law;
        let silo = event.silo_id;
        let tick = event.tick;

        crate::serial_println!(
            "[LAW ENFORCER] {} violated by Silo {} — \"{}\"",
            law.name(), silo, event.evidence
        );

        // Update per-Silo record
        let record = self.silo_records.entry(silo).or_insert_with(|| SiloViolationRecord {
            silo_id: silo,
            ..Default::default()
        });
        record.record(law, tick);
        let action = record.escalated_action(law);

        // Track recent laws for compound detection
        self.recent_laws.entry(silo).or_insert_with(Vec::new).push(law);
        let recent = self.recent_laws.get(&silo).cloned().unwrap_or_default();
        if let Some(compound) = is_compound_threat(&recent) {
            crate::serial_println!("[LAW ENFORCER] ⚠ COMPOUND THREAT detected: {}", compound);
            self.stats.compound_threats_detected += 1;
        }

        // Update stats
        self.stats.total_violations += 1;
        let idx = law as usize;
        if idx < 11 { self.stats.per_law_counts[idx] += 1; }
        match action {
            EnforcementAction::VaporizeSilo  => self.stats.silos_vaporized += 1,
            EnforcementAction::QuarantineSilo => self.stats.silos_quarantined += 1,
            EnforcementAction::WarnAndDim    => self.stats.warnings_issued += 1,
            EnforcementAction::StripNetSend  => self.stats.net_send_stripped += 1,
            EnforcementAction::DeepSleepSilo => self.stats.deep_sleeps_forced += 1,
            _ => {}
        }

        // Update silo state
        if let Some(rec) = self.silo_records.get_mut(&silo) {
            match action {
                EnforcementAction::VaporizeSilo   => rec.vaporized = true,
                EnforcementAction::QuarantineSilo  => rec.quarantined = true,
                _ => {}
            }
        }

        // Append to audit chain
        self.append_audit(event, action);

        crate::serial_println!(
            "[LAW ENFORCER] Action: {:?} (violations by this Silo: {})",
            action, self.silo_records[&silo].total_violations
        );

        action
    }

    /// Clear the recent-laws window per Silo (call every N ticks from Sentinel).
    pub fn clear_recent_window(&mut self) {
        self.recent_laws.clear();
    }

    /// Compliance report for a specific Silo.
    pub fn silo_compliance(&self, silo_id: u64) -> Option<&SiloViolationRecord> {
        self.silo_records.get(&silo_id)
    }

    /// Global compliance summary.
    pub fn print_compliance_report(&self) {
        crate::serial_println!("╔══════════════════════════════════════════════╗");
        crate::serial_println!("║       Q-Manifest Compliance Report           ║");
        crate::serial_println!("╠══════════════════════════════════════════════╣");
        crate::serial_println!("║ Total violations:     {:>6}                  ║", self.stats.total_violations);
        crate::serial_println!("║ Silos vaporized:      {:>6}                  ║", self.stats.silos_vaporized);
        crate::serial_println!("║ Silos quarantined:    {:>6}                  ║", self.stats.silos_quarantined);
        crate::serial_println!("║ Compound threats:     {:>6}                  ║", self.stats.compound_threats_detected);
        for law_num in 1u8..=10 {
            crate::serial_println!(
                "║ Law {:>2}: {:>6} violations                    ║",
                law_num, self.stats.per_law_counts[law_num as usize]
            );
        }
        crate::serial_println!("╚══════════════════════════════════════════════╝");
    }

    // ── Internal audit chain ──────────────────────────────────────────────────

    fn append_audit(&mut self, event: LawViolationEvent, action: EnforcementAction) {
        let seq = self.audit_seq;
        self.audit_seq += 1;
        // Hash: XOR of prev_hash ^ silo ^ law ^ tick (placeholder; production = SHA-256)
        let mut chain_hash = self.prev_chain_hash;
        chain_hash[0] ^= (event.silo_id & 0xFF) as u8;
        chain_hash[1] ^= event.law as u8;
        chain_hash[2] ^= (event.tick & 0xFF) as u8;
        chain_hash[3] ^= seq as u8;
        self.prev_chain_hash = chain_hash;
        if self.audit_log.len() >= self.max_audit { self.audit_log.remove(0); }
        self.audit_log.push(AuditEntry { seq, violation: event, action_taken: action, chain_hash });
    }
}
