//! # Digital Antibody — Sentinel Global Immunization System (Phase 76)
//!
//! ARCHITECTURE.md §7 — Global Immunization:
//! > "When Sentinel detects a new attack pattern, it generates a **Digital Antibody**
//! >  and broadcasts to all Q-Mesh nodes via Nexus."
//! > "Global propagation target: **<300ms**"
//!
//! ## Architecture Guardian: Design
//! ```text
//! Sentinel (sentinel.rs)
//!     │  detects novel attack (exploit, anomalous cap usage, covert channel)
//!     ▼
//! AntibodyGenerator::generate(threat)   ← this module
//!     │  1. Fingerprint the attack pattern (behavioural signature)
//!     │  2. Encode as AntibodyPayload (compact, Nexus-broadcastable)
//!     │  3. Sign with qernel Ed25519 key (TPM-backed in production)
//!     │  4. Add to LocalImmunityRegistry (applied instantly on this node)
//!     ▼
//! Nexus broadcast (nexus.rs delegates Q-Fabric send)
//!     │  Q-Fabric: QUIC multicast via 5G + satellite + mesh-Wi-Fi
//!     │  Target: all reachable Q-Mesh peers
//!     │  SLA: <300ms to >95% of connected nodes
//!     ▼
//! Remote nodes: AntibodyReceiver::apply(payload)
//!     │  Verify signature, check version, ingest
//!     └──► LocalImmunityRegistry updated on every peer
//! ```
//!
//! ## Threat Categories
//! - `CapabilityEscalation`: Silo acquired cap it was never granted  
//! - `CovertChannel`: Q-Traffic Shannon entropy spike (Phase 69 detects → Phase 76 immunizes)
//! - `MemoryCorruption`: Out-of-bounds access caught by hardware page guard
//! - `BinaryTampering`: Prism O-ID hash mismatch (Law 2 violation)
//! - `DenialOfService`: Fiber spin-loop or resource exhaustion pattern
//! - `LateralMovement`: Cross-Silo IPC spoofing attempt
//! - `ExfilAttempt`: Rapid large-object reads before NET_SEND (data exfiltration pattern)

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Threat Category ───────────────────────────────────────────────────────────

/// The class of attack a Digital Antibody was generated to counter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ThreatCategory {
    /// Silo gained capability it was never granted (Law 1 violation)
    CapabilityEscalation,
    /// Covert channel detected via Shannon entropy spike (Law 7 + Q-Traffic)
    CovertChannel,
    /// Hardware page fault outside Silo bounds (memory isolation breach)
    MemoryCorruption,
    /// Prism O-ID hash mismatch — binary was tampered (Law 2 violation)
    BinaryTampering,
    /// CPU/memory/NVMe exhaustion spin-loop (Law 3/8 violation)
    DenialOfService,
    /// IPC message spoofed with forged Silo ID (Law 6 violation)
    LateralMovement,
    /// Rapid mass-Prism-read before NET_SEND spike (exfiltration pattern)
    ExfilAttempt,
    /// Unknown/composite threat
    Unknown,
}

impl ThreatCategory {
    /// Severity: 0 (low) to 100 (critical)
    pub fn severity(self) -> u8 {
        match self {
            Self::CapabilityEscalation => 95,
            Self::CovertChannel        => 80,
            Self::MemoryCorruption     => 90,
            Self::BinaryTampering      => 98,
            Self::DenialOfService      => 70,
            Self::LateralMovement      => 88,
            Self::ExfilAttempt         => 85,
            Self::Unknown              => 50,
        }
    }

    /// Recommended Sentinel action on match.
    pub fn action(self) -> AntibodyAction {
        match self {
            Self::CapabilityEscalation => AntibodyAction::VaporizeSilo,
            Self::CovertChannel        => AntibodyAction::QuarantineAndAlert,
            Self::MemoryCorruption     => AntibodyAction::VaporizeSilo,
            Self::BinaryTampering      => AntibodyAction::BlacklistBinary,
            Self::DenialOfService      => AntibodyAction::ThrottleSilo,
            Self::LateralMovement      => AntibodyAction::VaporizeSilo,
            Self::ExfilAttempt         => AntibodyAction::StripNetSend,
            Self::Unknown              => AntibodyAction::AlertOnly,
        }
    }
}

/// What the immunity system does when it detects a pattern match.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntibodyAction {
    /// Immediately terminate and black-box the offending Silo
    VaporizeSilo,
    /// Isolate but keep running; alert operator
    QuarantineAndAlert,
    /// Mark the binary OID as untrusted; refuse to load it
    BlacklistBinary,
    /// Remove NET_SEND CapToken from offending Silo
    StripNetSend,
    /// Reduce CPU/memory quota for offending Silo
    ThrottleSilo,
    /// Log and alert; do not terminate
    AlertOnly,
}

// ── Behavioural Signature ─────────────────────────────────────────────────────

/// A compact, hashable fingerprint of an attack pattern.
/// Designed to be small enough for rapid Nexus broadcast.
#[derive(Debug, Clone)]
pub struct BehaviouralSignature {
    /// Attack category
    pub category: ThreatCategory,
    /// 32-byte behavioural hash (e.g. syscall sequence hash + memory access pattern)
    pub pattern_hash: [u8; 32],
    /// Optional: binary OID involved (for BinaryTampering)
    pub binary_oid: Option<[u8; 32]>,
    /// Optional: syscall sequence that triggered this (up to 8 IDs)
    pub syscall_sequence: [u32; 8],
    /// Optional: approximate Silo memory layout fingerprint
    pub mem_layout_hash: u64,
    /// Confidence score (0-100) from Sentinel's ML model
    pub confidence: u8,
}

impl BehaviouralSignature {
    pub fn simple(category: ThreatCategory, pattern_hash: [u8; 32]) -> Self {
        BehaviouralSignature {
            category,
            pattern_hash,
            binary_oid: None,
            syscall_sequence: [0; 8],
            mem_layout_hash: 0,
            confidence: 90,
        }
    }

    /// Compute a match score against an observed behaviour pattern.
    /// Returns 0 (no match) to 100 (exact match).
    pub fn match_score(&self, observed_hash: &[u8; 32]) -> u8 {
        // Hamming distance: count matching bytes
        let matching = self.pattern_hash.iter()
            .zip(observed_hash.iter())
            .filter(|(a, b)| a == b)
            .count();
        ((matching * 100) / 32) as u8
    }
}

// ── Antibody Payload ──────────────────────────────────────────────────────────

/// The over-the-wire payload broadcast via Nexus.
/// Compact: fits in a single Q-Fabric datagram (<1500B).
#[derive(Debug, Clone)]
pub struct AntibodyPayload {
    /// Unique antibody ID (generated on originating node)
    pub antibody_id: u64,
    /// Originating node ID (first 8 bytes of Nexus NodeId)
    pub origin_node: u64,
    /// Which Qindows version this applies to (prevents false positives on old versions)
    pub qindows_version: u32,
    /// The threat signature
    pub signature: BehaviouralSignature,
    /// Recommended response action
    pub action: AntibodyAction,
    /// When this was generated (originating node's tick)
    pub generated_at: u64,
    /// TTL hops — decremented by each relay node; stops relaying at 0
    pub ttl: u8,
    /// Ed25519 signature over (antibody_id || origin_node || pattern_hash)
    /// Preventing antibody spoofing. 64 bytes.
    pub signature_bytes: [u8; 64],
    /// Severity level
    pub severity: u8,
    /// Human-readable description (≤128 chars for on-wire efficiency)
    pub description: String,
}

impl AntibodyPayload {
    /// Approximate serialized size in bytes.
    pub fn wire_size(&self) -> usize {
        8 + 8 + 4 + 32 + 1 + 8 + 1 + 64 + 1 + self.description.len().min(128)
    }
}

// ── Local Immunity Registry ───────────────────────────────────────────────────

/// Tracks all antibodies applied to this node (local + received from Nexus).
pub struct LocalImmunityRegistry {
    /// All known antibodies: antibody_id → payload
    pub antibodies: BTreeMap<u64, AntibodyPayload>,
    /// Blacklisted binary OIDs (from BinaryTampering antibodies)
    pub blacklisted_oids: Vec<[u8; 32]>,
    /// Quarantined Silo IDs (from QuarantineAndAlert actions)
    pub quarantined_silos: Vec<u64>,
    /// Total antibodies received from Nexus peers
    pub received_from_peers: u64,
    /// Total antibodies generated locally
    pub locally_generated: u64,
    /// Total prevented attacks (signature matches that triggered action)
    pub attacks_prevented: u64,
}

impl LocalImmunityRegistry {
    pub fn new() -> Self {
        LocalImmunityRegistry {
            antibodies: BTreeMap::new(),
            blacklisted_oids: Vec::new(),
            quarantined_silos: Vec::new(),
            received_from_peers: 0,
            locally_generated: 0,
            attacks_prevented: 0,
        }
    }

    /// Apply an antibody (local or received). Returns true if new.
    pub fn apply(&mut self, payload: AntibodyPayload, from_peer: bool) -> bool {
        if self.antibodies.contains_key(&payload.antibody_id) {
            return false; // de-duplicate
        }
        crate::serial_println!(
            "[ANTIBODY] Applied: id={} cat={:?} sev={} action={:?} (peer={})",
            payload.antibody_id, payload.signature.category,
            payload.severity, payload.action, from_peer
        );
        // Side effects by action
        match payload.action {
            AntibodyAction::BlacklistBinary => {
                if let Some(oid) = payload.signature.binary_oid {
                    if !self.blacklisted_oids.contains(&oid) {
                        self.blacklisted_oids.push(oid);
                    }
                }
            }
            _ => {}
        }
        if from_peer { self.received_from_peers += 1; }
        else         { self.locally_generated += 1; }
        self.antibodies.insert(payload.antibody_id, payload);
        true
    }

    /// Check if a binary OID is blacklisted.
    pub fn is_binary_blacklisted(&self, oid: &[u8; 32]) -> bool {
        self.blacklisted_oids.contains(oid)
    }

    /// Scan a behavioural hash against all known antibodies.
    /// Returns the most severe matching antibody (if any, score ≥ 80).
    pub fn scan(&mut self, observed_hash: &[u8; 32]) -> Option<&AntibodyPayload> {
        let mut best_id: Option<u64> = None;
        let mut best_score = 80u8; // minimum match threshold
        for (id, ab) in &self.antibodies {
            let score = ab.signature.match_score(observed_hash);
            if score >= best_score {
                best_score = score;
                best_id = Some(*id);
            }
        }
        if let Some(id) = best_id {
            self.attacks_prevented += 1;
            self.antibodies.get(&id)
        } else {
            None
        }
    }
}

// ── Antibody Generator ────────────────────────────────────────────────────────

/// Generates antibodies from Sentinel threat reports and queues for broadcast.
pub struct AntibodyGenerator {
    /// Next antibody ID
    next_id: u64,
    /// This node's ID (first 8 bytes)
    pub node_id: u64,
    /// Qindows version number
    pub version: u32,
    /// Antibodies pending broadcast to Nexus
    pub broadcast_queue: Vec<AntibodyPayload>,
    /// Local immunity registry
    pub registry: LocalImmunityRegistry,
    /// Total antibodies ever generated
    pub total_generated: u64,
    /// Ticks since last broadcast (target: <300ms = <300 ticks)
    pub ticks_since_broadcast: u64,
}

impl AntibodyGenerator {
    pub fn new(node_id: u64) -> Self {
        AntibodyGenerator {
            next_id: 1,
            node_id,
            version: 10000, // v1.0.0
            broadcast_queue: Vec::new(),
            registry: LocalImmunityRegistry::new(),
            total_generated: 0,
            ticks_since_broadcast: 0,
        }
    }

    /// Generate a Digital Antibody from a Sentinel-reported threat.
    ///
    /// Called by Sentinel immediately on threat detection — should be fast (<1ms).
    pub fn generate(
        &mut self,
        category: ThreatCategory,
        pattern_hash: [u8; 32],
        binary_oid: Option<[u8; 32]>,
        description: &str,
        tick: u64,
    ) -> u64 {
        let antibody_id = (self.node_id << 24) | self.next_id;
        self.next_id += 1;

        let mut sig = BehaviouralSignature::simple(category, pattern_hash);
        sig.binary_oid = binary_oid;
        sig.confidence = 92; // Sentinel-generated = high confidence

        // Ed25519 signature (production: TPM signs; here: XOR placeholder)
        let mut sig_bytes = [0u8; 64];
        for (i, &b) in pattern_hash.iter().enumerate() {
            sig_bytes[i]    = b ^ (antibody_id as u8);
            sig_bytes[i+32] = b.wrapping_add(i as u8);
        }

        let payload = AntibodyPayload {
            antibody_id,
            origin_node: self.node_id,
            qindows_version: self.version,
            signature: sig,
            action: category.action(),
            generated_at: tick,
            ttl: 16, // propagate up to 16 hops through Nexus mesh
            signature_bytes: sig_bytes,
            severity: category.severity(),
            description: {
                let mut s = description.to_string();
                s.truncate(128);
                s
            },
        };

        crate::serial_println!(
            "[ANTIBODY] Generated: id={:#x} cat={:?} sev={} size={}B → queued for Nexus broadcast",
            antibody_id, category, category.severity(), payload.wire_size()
        );

        // Apply locally first (instant protection on originating node)
        self.registry.apply(payload.clone(), false);
        // Queue for Nexus broadcast
        self.broadcast_queue.push(payload);
        self.total_generated += 1;

        antibody_id
    }

    /// Receive an antibody from a Nexus peer. Verify, apply, and re-relay if TTL > 0.
    pub fn receive_from_peer(&mut self, mut payload: AntibodyPayload) -> bool {
        // Version check
        if payload.qindows_version != self.version { return false; }
        // Signature verification (production: Ed25519 verify; here: simple check)
        let ok = payload.signature_bytes[0] != 0;
        if !ok {
            crate::serial_println!("[ANTIBODY] Rejected: invalid signature from peer.");
            return false;
        }
        let is_new = self.registry.apply(payload.clone(), true);
        if is_new && payload.ttl > 0 {
            // Re-relay with decremented TTL (mesh propagation)
            payload.ttl -= 1;
            self.broadcast_queue.push(payload);
        }
        is_new
    }

    /// Drain broadcast queue — called by Nexus/Q-Fabric handler each tick.
    /// Returns payloads to transmit. Target: queue drained within 300 ticks.
    pub fn drain_broadcast_queue(&mut self) -> Vec<AntibodyPayload> {
        let queue = core::mem::take(&mut self.broadcast_queue);
        if !queue.is_empty() {
            crate::serial_println!(
                "[ANTIBODY] Broadcasting {} antibody/antibodies via Nexus.", queue.len()
            );
            self.ticks_since_broadcast = 0;
        }
        queue
    }

    /// Q-Silo launch gate: check binary OID before spawning.
    pub fn check_binary(&self, oid: &[u8; 32]) -> bool {
        !self.registry.is_binary_blacklisted(oid)
    }

    /// Scan a runtime behaviour hash — called by Sentinel per-tick.
    pub fn scan_behaviour(&mut self, observed_hash: &[u8; 32]) -> Option<AntibodyAction> {
        self.registry.scan(observed_hash).map(|ab| ab.action)
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║    Digital Antibody System (§7)      ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Locally generated: {:>6}             ║", self.total_generated);
        crate::serial_println!("║ From peers:        {:>6}             ║", self.registry.received_from_peers);
        crate::serial_println!("║ Known antibodies:  {:>6}             ║", self.registry.antibodies.len());
        crate::serial_println!("║ Attacks prevented: {:>6}             ║", self.registry.attacks_prevented);
        crate::serial_println!("║ Blacklisted bins:  {:>6}             ║", self.registry.blacklisted_oids.len());
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
