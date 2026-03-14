//! # First Boot — Qindows Setup Wizard State Machine (Phase 68)
//!
//! The First Boot wizard is the "Transition Point" from a blank machine to
//! a fully operational Qindows node.  
//!
//! ## ARCHITECTURE.md §"First Boot" Experience
//! The sequence described in the spec:
//! > 1. Instant-On Greeting: Aether fades in, "Identity is the key. Who are you?"
//! > 2. Q-Bridge: scan Windows drive, migrate data, convert registry
//! > 3. Privacy Perimeter: user selects Monolith / Ghost / Flow tier
//! > 4. Neural Tuning: detect NPU, calibrate Q-Synapse input model
//! > 5. Memory Flattening: compress OS state into NVMe fast-cache
//!
//! ## Architecture Guardian: State Machine Design
//! Each step is a distinct `FirstBootStep` variant. Steps are LINEAR —
//! a user cannot skip steps or go backward. The wizard is owned by the
//! very first Q-Silo spawned (the Setup Silo). Once `Complete`, the
//! Setup Silo vaporizes and the standard Q-Shell Silo is launched.
//!
//! This module provides the **kernel-side state tracker**. The Aether
//! compositor renders the actual visuals from the scene graph submitted
//! by the Setup Silo via the Q-Ring.

extern crate alloc;
use alloc::string::String;
use alloc::string::ToString;
use crate::identity::CapabilityTier;


// ── Setup Steps ───────────────────────────────────────────────────────────────

/// The ordered steps of the First Boot wizard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstBootStep {
    /// §1 — Aether generative background, identity creation prompt
    Greeting,
    /// §2 — Q-Bridge migration scan (optional: user may skip)
    DataMigration,
    /// §3 — User selects Monolith / Ghost / Flow capability tier
    PrivacyPerimeter,
    /// §4 — NPU detection and Q-Synapse neural calibration
    NeuralCalibration,
    /// §5 — OS state compression to NVMe fast-cache layer
    MemoryFlattening,
    /// All steps complete — Setup Silo will vaporize
    Complete,
}

impl FirstBootStep {
    /// Advance to the next step.
    pub fn next(self) -> Self {
        match self {
            Self::Greeting          => Self::DataMigration,
            Self::DataMigration     => Self::PrivacyPerimeter,
            Self::PrivacyPerimeter  => Self::NeuralCalibration,
            Self::NeuralCalibration => Self::MemoryFlattening,
            Self::MemoryFlattening  => Self::Complete,
            Self::Complete          => Self::Complete,
        }
    }

    pub fn is_complete(self) -> bool { self == Self::Complete }
}

// ── Hardware Profile ──────────────────────────────────────────────────────────

/// Detected hardware relevant to first-boot decisions.
#[derive(Debug, Clone)]
pub struct HardwareProfile {
    /// Number of physical CPU cores
    pub cpu_core_count: u8,
    /// Total physical RAM (MiB)
    pub ram_mib: u32,
    /// NVMe capacity (GiB)
    pub nvme_gib: u32,
    /// NVMe fast-cache (Optane/SLC) available (GiB, 0 if none)
    pub nvme_fastcache_gib: u32,
    /// NPU detected and available?
    pub npu_present: bool,
    /// NPU TOPS (tera-operations per second, 0 if absent)
    pub npu_tops: u8,
    /// TPM 3.0 enclave present?
    pub tpm_present: bool,
    /// Estimated GPU VRAM (MiB)
    pub gpu_vram_mib: u32,
    /// BCI hardware detected (EEG/implant via Q-Synapse)?
    pub bci_present: bool,
}

impl HardwareProfile {
    /// Returns `true` if this machine is suitable for the Ghost tier.
    /// Ghost tier requires: NPU + TPM + at least 8GB RAM.
    pub fn supports_ghost_tier(&self) -> bool {
        self.npu_present && self.tpm_present && self.ram_mib >= 8192
    }

    /// Returns the recommended capability tier for this hardware.
    pub fn recommended_tier(&self) -> CapabilityTier {
        if self.supports_ghost_tier() {
            CapabilityTier::Ghost
        } else if self.tpm_present {
            CapabilityTier::Monolith
        } else {
            CapabilityTier::Flow
        }
    }
}

impl Default for HardwareProfile {
    fn default() -> Self {
        HardwareProfile {
            cpu_core_count: 1,
            ram_mib: 256,
            nvme_gib: 0,
            nvme_fastcache_gib: 0,
            npu_present: false,
            npu_tops: 0,
            tpm_present: false,
            gpu_vram_mib: 0,
            bci_present: false,
        }
    }
}

impl FirstBootState {
    /// Construct with a concrete HardwareProfile (used during actual first boot).
    pub fn new(hardware: HardwareProfile, setup_silo: u64, tick: u64) -> Self {
        crate::serial_println!("[FIRSTBOOT] Setup wizard starting.");
        crate::serial_println!(
            "[FIRSTBOOT] Hardware: {} cores, {}MB RAM, NPU={}, TPM={}, BCI={}",
            hardware.cpu_core_count, hardware.ram_mib,
            hardware.npu_present, hardware.tpm_present, hardware.bci_present
        );
        FirstBootState {
            current_step: FirstBootStep::Greeting,
            hardware,
            user_name: None,
            selected_tier: None,
            migration_summary: None,
            neural_cal: None,
            flatten_result: None,
            setup_silo,
            started_at: tick,
        }
    }

    /// Compat shim for genesis bridge: is the wizard done?
    pub fn check_completed(&self) -> bool { self.is_complete() }

    /// Compat shim for genesis bridge: advance the wizard one step (ignores arg).
    pub fn step(&mut self, _tick: u64) -> FirstBootStep {
        if !self.is_complete() { self.advance(); }
        self.current_step
    }
}

impl Default for FirstBootState {
    fn default() -> Self {
        FirstBootState::new(HardwareProfile::default(), 0, 0)
    }
}

/// Summary of what the Q-Bridge found during the data migration step.
#[derive(Debug, Clone)]
pub struct MigrationSummary {
    /// Total files found on the Windows volume
    pub files_found: u64,
    /// Files migrated as Prism objects
    pub files_migrated: u64,
    /// Registry keys converted to K-V store
    pub registry_keys_converted: u64,
    /// Windows drive size (GB)
    pub original_size_gb: u64,
    /// Qindows Prism size after dedup (GB)
    pub deduplicated_size_gb: u64,
    /// Native Q-Apps found to replace Windows apps
    pub native_replacements: u64,
    /// Was migration skipped?
    pub skipped: bool,
}

// ── Neural Calibration Result ─────────────────────────────────────────────────

/// Result of NPU-based Q-Synapse calibration.
#[derive(Debug, Clone)]
pub struct NeuralCalibrationResult {
    /// Was an NPU found?
    pub npu_found: bool,
    /// Was a BCI device calibrated?
    pub bci_calibrated: bool,
    /// Baseline confidence threshold derived from calibration
    pub baseline_confidence: f32,
    /// Number of initial neural patterns recorded
    pub patterns_recorded: u8,
    /// Estimated latency of neural-to-intent pipeline (ms)
    pub estimated_latency_ms: f32,
}

impl Default for NeuralCalibrationResult {
    fn default() -> Self {
        NeuralCalibrationResult {
            npu_found: false,
            bci_calibrated: false,
            baseline_confidence: 0.82,
            patterns_recorded: 0,
            estimated_latency_ms: 5.0,
        }
    }
}

// ── Memory Flatten Result ─────────────────────────────────────────────────────

/// Result of the NVMe fast-cache OS state flattening.
#[derive(Debug, Clone)]
pub struct MemoryFlattenResult {
    /// Qernel state size before compression (KiB)
    pub uncompressed_kib: u32,
    /// Qernel state size after Zstd compression (KiB)
    pub compressed_kib: u32,
    /// Was fast-cache (Optane/SLC) used?
    pub fastcache_used: bool,
    /// Estimated boot time after flattening (ms)
    pub estimated_boot_ms: u32,
}

// ── First Boot State Machine ──────────────────────────────────────────────────

/// The overall state of the first boot wizard.
#[derive(Debug, Clone)]
pub struct FirstBootState {
    /// Current wizard step
    pub current_step: FirstBootStep,
    /// Detected hardware profile
    pub hardware: HardwareProfile,
    /// User-entered display name
    pub user_name: Option<String>,
    /// Selected capability tier
    pub selected_tier: Option<CapabilityTier>,
    /// Migration summary (filled if migration ran)
    pub migration_summary: Option<MigrationSummary>,
    /// Neural calibration result
    pub neural_cal: Option<NeuralCalibrationResult>,
    /// Memory flatten result
    pub flatten_result: Option<MemoryFlattenResult>,
    /// Setup Silo ID
    pub setup_silo: u64,
    /// Kernel tick when boot wizard started
    pub started_at: u64,
}

impl FirstBootState {
    // ── Step Handlers ─────────────────────────────────────────────────────────

    /// §1 Complete Greeting: user entered their identity name.
    pub fn complete_greeting(&mut self, user_name: String) {
        crate::serial_println!(
            "[FIRSTBOOT] Greeting complete. Identity: \"{}\"", user_name
        );
        self.user_name = Some(user_name);
        self.advance();
    }

    /// §2 Complete Migration (or skip it).
    pub fn complete_migration(&mut self, summary: Option<MigrationSummary>) {
        match &summary {
            Some(s) if !s.skipped => crate::serial_println!(
                "[FIRSTBOOT] Migration complete: {} files → {}GB (was {}GB, {}GB saved via dedup).",
                s.files_migrated, s.deduplicated_size_gb, s.original_size_gb,
                s.original_size_gb.saturating_sub(s.deduplicated_size_gb)
            ),
            _ => crate::serial_println!("[FIRSTBOOT] Migration skipped."),
        }
        self.migration_summary = summary;
        self.advance();
    }

    /// §3 Privacy perimeter: user selected capability tier.
    pub fn select_capability_tier(&mut self, tier: CapabilityTier) {
        crate::serial_println!("[FIRSTBOOT] Capability tier selected: {:?}", tier);
        self.selected_tier = Some(tier);
        self.advance();
    }

    /// §4 Neural calibration complete.
    pub fn complete_neural_calibration(&mut self, result: NeuralCalibrationResult) {
        if result.bci_calibrated {
            crate::serial_println!(
                "[FIRSTBOOT] Neural calibration complete: {} patterns, {:.0}ms latency.",
                result.patterns_recorded, result.estimated_latency_ms
            );
        } else if result.npu_found {
            crate::serial_println!("[FIRSTBOOT] NPU tuned. No BCI device detected.");
        } else {
            crate::serial_println!("[FIRSTBOOT] No NPU or BCI. Using software fallback.");
        }
        self.neural_cal = Some(result);
        self.advance();
    }

    /// §5 Memory flattening complete.
    pub fn complete_memory_flatten(&mut self, result: MemoryFlattenResult) {
        crate::serial_println!(
            "[FIRSTBOOT] Memory flattened: {}KiB → {}KiB (Zstd). Estimated boot: {}ms.",
            result.uncompressed_kib, result.compressed_kib, result.estimated_boot_ms
        );
        if result.fastcache_used {
            crate::serial_println!("[FIRSTBOOT] State written to NVMe fast-cache (Optane/SLC).");
        }
        self.flatten_result = Some(result);
        self.advance();
    }

    /// Internal: advance to the next step.
    fn advance(&mut self) {
        self.current_step = self.current_step.next();
        crate::serial_println!(
            "[FIRSTBOOT] → Step: {:?}", self.current_step
        );
        if self.current_step.is_complete() {
            self.finalize();
        }
    }

    /// Called when all steps are done.
    fn finalize(&self) {
        let name = self.user_name.as_deref().unwrap_or("User");
        let tier = self.selected_tier.unwrap_or(CapabilityTier::Monolith);
        crate::serial_println!("════════════════════════════════════════════════");
        crate::serial_println!(" Welcome to Qindows, {}!", name);
        crate::serial_println!(" Capability Tier:  {:?}", tier);
        if let Some(m) = &self.migration_summary {
            if !m.skipped {
                crate::serial_println!(
                    " Migration: {}GB → {}GB ({}GB saved via dedup)",
                    m.original_size_gb, m.deduplicated_size_gb,
                    m.original_size_gb.saturating_sub(m.deduplicated_size_gb)
                );
                crate::serial_println!(" {} legacy apps replaced with native Q-Apps.", m.native_replacements);
            }
        }
        if let Some(r) = &self.flatten_result {
            crate::serial_println!(" Estimated boot time: {}ms.", r.estimated_boot_ms);
        }
        crate::serial_println!(" Setup Silo will be vaporized. Q-Shell launching.");
        crate::serial_println!("════════════════════════════════════════════════");
    }

    /// Has the wizard completed all steps?
    pub fn is_complete(&self) -> bool { self.current_step.is_complete() }
}
