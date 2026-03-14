//! # Qernel Secure Boot Chain
//!
//! Verified boot: each component in the boot chain is hashed and
//! compared against trusted measurements before execution.
//!
//! Chain of trust:
//!   Firmware → Bootloader → Qernel → Silo Images
//!
//! Supports:
//! - SHA-256 measurement of boot components
//! - PCR (Platform Configuration Register) extend model
//! - Policy-based boot decisions (enforce/audit/off)
//! - Secure boot event log
//! - Rollback protection via monotonic counters

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── Measurements ───────────────────────────────────────────────────────────

/// A SHA-256 digest.
pub type Digest = [u8; 32];

/// Boot component that can be measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootComponent {
    /// Platform firmware (UEFI)
    Firmware,
    /// Bootloader (GRUB/Limine/custom)
    Bootloader,
    /// Qernel binary
    Kernel,
    /// Init Silo (first userspace)
    InitSilo,
    /// Device driver blob
    Driver,
    /// Boot configuration
    BootConfig,
    /// ACPI tables
    AcpiTables,
    /// Kernel command line
    CommandLine,
}

/// A measurement event in the boot log.
#[derive(Debug, Clone)]
pub struct MeasurementEvent {
    /// Which PCR was extended
    pub pcr_index: u8,
    /// Boot component measured
    pub component: BootComponent,
    /// SHA-256 digest of the component
    pub digest: Digest,
    /// Component name / description
    pub description: String,
    /// Size of measured data (bytes)
    pub size: u64,
    /// Timestamp (ns since boot, 0 for firmware)
    pub timestamp: u64,
    /// Did this measurement pass policy?
    pub passed: bool,
}

/// A Platform Configuration Register (software emulation).
#[derive(Debug, Clone)]
pub struct Pcr {
    /// PCR index (0–23)
    pub index: u8,
    /// Current PCR value (extend = hash(old || new))
    pub value: Digest,
    /// How many times extended
    pub extend_count: u32,
    /// Is this PCR locked (no more extends)?
    pub locked: bool,
}

impl Pcr {
    pub fn new(index: u8) -> Self {
        Pcr {
            index,
            value: [0u8; 32],
            extend_count: 0,
            locked: false,
        }
    }

    /// Extend this PCR: new_value = SHA256(old_value || digest).
    pub fn extend(&mut self, digest: &Digest) -> bool {
        if self.locked { return false; }

        // Concatenate old value and new digest
        let mut combined = [0u8; 64];
        combined[..32].copy_from_slice(&self.value);
        combined[32..].copy_from_slice(digest);

        // Hash the concatenation (simplified — production uses real SHA-256)
        self.value = Self::simple_hash(&combined);
        self.extend_count += 1;
        true
    }

    /// Lock the PCR (no further extensions).
    pub fn lock(&mut self) {
        self.locked = true;
    }

    /// Simplified hash for no_std (production uses hardware SHA-256).
    pub fn simple_hash(data: &[u8]) -> Digest {
        let mut hash = [0u8; 32];
        let mut h: u64 = 0x6a09e667bb67ae85;
        for (i, &b) in data.iter().enumerate() {
            h = h.wrapping_mul(0x100000001b3).wrapping_add(b as u64);
            hash[i % 32] ^= (h >> ((i % 8) * 8)) as u8;
        }
        // Second pass for better diffusion
        for i in 0..32 {
            hash[i] = hash[i]
                .wrapping_add(hash[(i + 13) % 32])
                .wrapping_mul(0x9e);
        }
        hash
    }
}

// ─── Boot Policy ────────────────────────────────────────────────────────────

/// Secure boot enforcement mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPolicy {
    /// Strict: reject any untrusted component, halt boot
    Enforce,
    /// Audit: log violations but continue booting
    Audit,
    /// Off: no secure boot verification
    Disabled,
}

/// A trusted boot measurement (expected value).
#[derive(Debug, Clone)]
pub struct TrustedMeasurement {
    /// Component this applies to
    pub component: BootComponent,
    /// Expected digest
    pub expected: Digest,
    /// Human-readable label
    pub label: String,
    /// Version / rollback counter
    pub version: u64,
}

// ─── Secure Boot Engine ─────────────────────────────────────────────────────

/// Boot verification result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// Measurement matches trusted value
    Trusted,
    /// Measurement does not match
    Untrusted,
    /// No trusted measurement found (unknown component)
    Unknown,
    /// Policy disabled, not checked
    Skipped,
}

/// Secure boot statistics.
#[derive(Debug, Clone, Default)]
pub struct SecureBootStats {
    pub components_measured: u64,
    pub components_trusted: u64,
    pub components_untrusted: u64,
    pub components_unknown: u64,
    pub pcr_extends: u64,
    pub policy_violations: u64,
}

/// The Secure Boot Engine.
pub struct SecureBoot {
    /// Platform Configuration Registers
    pub pcrs: Vec<Pcr>,
    /// Boot event log
    pub event_log: Vec<MeasurementEvent>,
    /// Trusted measurement database
    pub trusted_db: Vec<TrustedMeasurement>,
    /// Current policy
    pub policy: BootPolicy,
    /// Rollback counter (monotonic, stored in TPM/NV)
    pub rollback_counter: u64,
    /// Is the boot chain intact? (all components verified)
    pub chain_intact: bool,
    /// Statistics
    pub stats: SecureBootStats,
}

impl SecureBoot {
    pub fn new(policy: BootPolicy) -> Self {
        let mut pcrs = Vec::with_capacity(24);
        for i in 0..24 {
            pcrs.push(Pcr::new(i));
        }

        SecureBoot {
            pcrs,
            event_log: Vec::new(),
            trusted_db: Vec::new(),
            policy,
            rollback_counter: 0,
            chain_intact: true,
            stats: SecureBootStats::default(),
        }
    }

    /// Register a trusted measurement.
    pub fn add_trusted(&mut self, component: BootComponent, digest: Digest, label: &str, version: u64) {
        self.trusted_db.push(TrustedMeasurement {
            component,
            expected: digest,
            label: String::from(label),
            version,
        });
    }

    /// Measure a boot component.
    pub fn measure(
        &mut self,
        component: BootComponent,
        data: &[u8],
        description: &str,
        now: u64,
    ) -> VerifyResult {
        if self.policy == BootPolicy::Disabled {
            return VerifyResult::Skipped;
        }

        // Compute digest
        let digest = Pcr::simple_hash(data);

        // Determine PCR index by component type
        let pcr_index = match component {
            BootComponent::Firmware => 0,
            BootComponent::Bootloader => 1,
            BootComponent::Kernel => 2,
            BootComponent::InitSilo => 3,
            BootComponent::Driver => 4,
            BootComponent::BootConfig => 5,
            BootComponent::AcpiTables => 6,
            BootComponent::CommandLine => 7,
        };

        // Extend PCR
        if (pcr_index as usize) < self.pcrs.len() {
            self.pcrs[pcr_index as usize].extend(&digest);
            self.stats.pcr_extends += 1;
        }

        // Verify against trusted database
        let result = self.verify_digest(component, &digest);
        let passed = result == VerifyResult::Trusted;

        // Record event
        self.event_log.push(MeasurementEvent {
            pcr_index,
            component,
            digest,
            description: String::from(description),
            size: data.len() as u64,
            timestamp: now,
            passed,
        });

        // Update stats
        self.stats.components_measured += 1;
        match result {
            VerifyResult::Trusted => self.stats.components_trusted += 1,
            VerifyResult::Untrusted => {
                self.stats.components_untrusted += 1;
                self.stats.policy_violations += 1;
                if self.policy == BootPolicy::Enforce {
                    self.chain_intact = false;
                }
            }
            VerifyResult::Unknown => self.stats.components_unknown += 1,
            VerifyResult::Skipped => {}
        }

        result
    }

    /// Check digest against trusted database.
    fn verify_digest(&self, component: BootComponent, digest: &Digest) -> VerifyResult {
        let matches: Vec<&TrustedMeasurement> = self.trusted_db.iter()
            .filter(|t| t.component == component)
            .collect();

        if matches.is_empty() {
            return VerifyResult::Unknown;
        }

        if matches.iter().any(|t| &t.expected == digest) {
            VerifyResult::Trusted
        } else {
            VerifyResult::Untrusted
        }
    }

    /// Check rollback protection.
    pub fn check_rollback(&self, component_version: u64) -> bool {
        component_version >= self.rollback_counter
    }

    /// Advance the rollback counter (after successful boot).
    pub fn advance_rollback(&mut self, new_min: u64) {
        if new_min > self.rollback_counter {
            self.rollback_counter = new_min;
        }
    }

    /// Lock all boot-phase PCRs (called after init).
    pub fn lock_boot_pcrs(&mut self) {
        for pcr in &mut self.pcrs[..8] {
            pcr.lock();
        }
    }

    /// Is the boot chain fully verified and intact?
    pub fn is_trusted(&self) -> bool {
        self.chain_intact && self.stats.components_untrusted == 0
    }

    /// Get a summary of the boot state.
    pub fn summary(&self) -> String {
        alloc::format!(
            "SecureBoot[{:?}]: {} measured, {} trusted, {} untrusted, chain={}",
            self.policy,
            self.stats.components_measured,
            self.stats.components_trusted,
            self.stats.components_untrusted,
            if self.chain_intact { "INTACT" } else { "BROKEN" },
        )
    }
}
