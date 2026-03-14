//! # Q-Bridge — Windows Migration & Legacy Data Ingestion (Phase 65)
//!
//! Q-Bridge is the first-boot migration tool that ingests a legacy Windows
//! installation into Qindows's native object model.
//!
//! ## ARCHITECTURE.md §"First Boot": The Bridge (Data Migration)
//! > "Q-Bridge: scans your .exe files and checks the Global Q-Ledger to
//! > see if a native Q-App version exists. Extracts settings from the old
//! > Windows Registry and converts them into the Key-Value Config Store."
//!
//! ## What Q-Bridge Does (kernel-side interface)
//! 1. **Inventory Scan**: walks a mounted Windows volume, classifying objects
//! 2. **App Matching**: checks Q-Ledger for native equivalents to found .exe files
//! 3. **Registry Extraction**: parses Windows HIVE format → Prism K-V objects
//! 4. **File Migration**: converts NTFS files to QFS CoW Ghost-Write objects
//! 5. **Deduplication Report**: shows how many GB were saved by content-addressing
//!
//! ## Architecture Guardian Note
//! Q-Bridge is a **migration utility** — it runs in a dedicated Q-Silo with
//! a time-limited `Storage(scope="legacy_volume")` capability. It has NO
//! permanent access to the target system's Prism. After migration completes,
//! the Silo is vaporized and the capability token expires.
//!
//! ## Q-Manifest Law 9: Universal Namespace
//! The Windows volume is mounted via `dev://legacy-ntfs/` in the UNS.
//! Q-Bridge never hardcodes drive letters — it addresses all objects by UNS URI.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ── File Classification ────────────────────────────────────────────────────────

/// How Q-Bridge classified a file found on the Windows volume.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyFileClass {
    /// `.exe` / `.dll` — the application binary
    ExecutableBinary,
    /// A system DLL already part of a known shared library   
    SystemLibrary,
    /// User document (doc, pdf, xlsx, etc.)
    UserDocument,
    /// Multimedia (video, audio, image)
    MediaFile,
    /// Font file
    Font,
    /// Registry hive file (NTUSER.DAT, SOFTWARE, SYSTEM)
    RegistryHive,
    /// Temporary / cache file (migrate = false)
    Temporary,
    /// Unknown / unclassified
    Unknown,
}

/// A discovered file from the Windows volume.
#[derive(Debug, Clone)]
pub struct LegacyFile {
    /// Full Windows path (e.g. `C:\Users\Dave\Documents\report.pdf`)
    pub windows_path: String,
    /// File size in bytes
    pub size_bytes: u64,
    /// Classification
    pub class: LegacyFileClass,
    /// FNV-64 content hash (for deduplication)
    pub content_hash: u64,
    /// Should this file be migrated?
    pub should_migrate: bool,
    /// If an app: was a native Q-App equivalent found in the Ledger?
    pub native_app_id: Option<String>,
}

// ── Registry Migration ────────────────────────────────────────────────────────

/// A single extracted Windows Registry key-value pair.
#[derive(Debug, Clone)]
pub struct RegistryEntry {
    /// Full registry path (e.g. `HKCU\Software\MyApp\Settings`)
    pub path: String,
    /// Value name
    pub name: String,
    /// Serialized value (REG_SZ, REG_DWORD, etc. → JSON string)
    pub json_value: String,
    /// Registry value type
    pub reg_type: RegistryType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryType {
    String,
    DWord,
    QWord,
    Binary,
    MultiString,
    Unknown,
}

// ── Migration Plan ────────────────────────────────────────────────────────────

/// The migration plan produced by the scan phase.
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    /// Files to be migrated as Prism objects
    pub files_to_migrate: Vec<LegacyFile>,
    /// Files to skip (temporaries, system binaries with native equivalents, etc.)
    pub files_to_skip: Vec<LegacyFile>,
    /// Registry entries to convert to Prism K-V store
    pub registry_entries: Vec<RegistryEntry>,
    /// Total bytes that will be migrated
    pub total_bytes_to_migrate: u64,
    /// Bytes expected to be saved by deduplication
    pub dedup_savings_estimate: u64,
    /// Number of .exe files that have a native Q-App equivalent
    pub native_app_replacements: u64,
    /// Number of files skipped as system temporaries  
    pub temp_files_skipped: u64,
}

impl MigrationPlan {
    pub fn new() -> Self {
        MigrationPlan {
            files_to_migrate: Vec::new(),
            files_to_skip: Vec::new(),
            registry_entries: Vec::new(),
            total_bytes_to_migrate: 0,
            dedup_savings_estimate: 0,
            native_app_replacements: 0,
            temp_files_skipped: 0,
        }
    }
}

// ── Migration Statistics ──────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct BridgeStats {
    pub files_scanned: u64,
    pub files_migrated: u64,
    pub registry_keys_converted: u64,
    pub dedup_bytes_saved: u64,
    pub native_apps_found: u64,
    pub migration_errors: u64,
}

// ── Migration Phase ───────────────────────────────────────────────────────────

/// Current phase of the Q-Bridge migration process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    /// Scanning the Windows volume
    Scanning,
    /// Building the migration plan
    Planning,
    /// Waiting for user confirmation
    AwaitingConfirmation,
    /// Writing objects to Prism
    Migrating,
    /// Extracting and converting registry
    ConvertingRegistry,
    /// Migration complete — Silo will be vaporized
    Complete,
    /// Migration aborted by user or Sentinel
    Aborted,
}

// ── Q-Bridge Engine ───────────────────────────────────────────────────────────

/// The Q-Bridge migration engine (kernel-side state).
pub struct QBridge {
    /// Target Windows volume UNS path (e.g. `dev://ntfs-vol-0/`)
    pub volume_uns: String,
    /// Owning Silo (migration Silo, time-limited cap)
    pub migration_silo: u64,
    /// Current phase
    pub phase: MigrationPhase,
    /// Generated migration plan
    pub plan: Option<MigrationPlan>,
    /// Known native Q-App IDs for matching (from Q-Ledger)
    pub known_native_apps: BTreeMap<String, String>, // exe_name → q_app_id
    /// Stats
    pub stats: BridgeStats,
}

impl QBridge {
    pub fn new(volume_uns: String, migration_silo: u64) -> Self {
        QBridge {
            volume_uns,
            migration_silo,
            phase: MigrationPhase::Scanning,
            plan: None,
            known_native_apps: BTreeMap::new(),
            stats: BridgeStats::default(),
        }
    }

    /// Register a known exe→Q-App mapping (from Q-Ledger native app database).
    pub fn register_native_mapping(&mut self, exe_name: String, q_app_id: String) {
        self.known_native_apps.insert(exe_name, q_app_id);
    }

    /// Classify a file by its extension.
    fn classify(path: &str, size: u64) -> LegacyFileClass {
        let lower = path.to_lowercase();
        if lower.ends_with(".tmp") || lower.ends_with(".log") || lower.contains("\\temp\\") {
            return LegacyFileClass::Temporary;
        }
        if lower.ends_with(".exe") { return LegacyFileClass::ExecutableBinary; }
        if lower.ends_with(".dll") { return LegacyFileClass::SystemLibrary; }
        if lower.ends_with("ntuser.dat") || lower.ends_with("software") || lower.ends_with("system") {
            return LegacyFileClass::RegistryHive;
        }
        if lower.ends_with(".mp4") || lower.ends_with(".mkv")
            || lower.ends_with(".mp3") || lower.ends_with(".jpg") || lower.ends_with(".png") {
            return LegacyFileClass::MediaFile;
        }
        if lower.ends_with(".ttf") || lower.ends_with(".otf") {
            return LegacyFileClass::Font;
        }
        if lower.ends_with(".pdf") || lower.ends_with(".docx")
            || lower.ends_with(".xlsx") || lower.ends_with(".txt") {
            return LegacyFileClass::UserDocument;
        }
        LegacyFileClass::Unknown
    }

    /// Simulate scanning a legacy Windows volume.
    ///
    /// In production: walks `dev://ntfs-vol-0/` via the UNS device resolver,
    /// reads directory entries from the NTFS MFT, hashes file contents via the
    /// IOMMU-safe DMA read path.
    pub fn scan_volume(&mut self, file_list: &[(&str, u64, u64)]) -> MigrationPlan {
        self.phase = MigrationPhase::Planning;
        let mut plan = MigrationPlan::new();

        for &(path, size, content_hash) in file_list {
            let class = Self::classify(path, size);
            self.stats.files_scanned += 1;

            let should_migrate = !matches!(class,
                LegacyFileClass::Temporary | LegacyFileClass::SystemLibrary
            );

            // Check for native Q-App equivalent
            let native_id: Option<String> = if class == LegacyFileClass::ExecutableBinary {
                let basename = path.rfind('\\')
                    .map(|i| &path[i+1..])
                    .unwrap_or(path)
                    .to_lowercase();
                let found = self.known_native_apps.get(&basename).cloned();
                if found.is_some() {
                    plan.native_app_replacements += 1;
                    self.stats.native_apps_found += 1;
                }
                found
            } else { None };

            let file = LegacyFile {
                windows_path: path.into(),
                size_bytes: size,
                class,
                content_hash,
                should_migrate,
                native_app_id: native_id,
            };

            if should_migrate {
                plan.total_bytes_to_migrate += size;
                plan.files_to_migrate.push(file);
            } else {
                plan.temp_files_skipped += 1;
                plan.files_to_skip.push(file);
            }
        }

        // Estimate dedup savings (simplified: 30% average for user documents)
        plan.dedup_savings_estimate = plan.total_bytes_to_migrate * 3 / 10;

        crate::serial_println!(
            "[BRIDGE] Scan complete: {} files to migrate, {} skipped, {} native replacements.",
            plan.files_to_migrate.len(), plan.temp_files_skipped, plan.native_app_replacements
        );
        crate::serial_println!(
            "[BRIDGE] Estimated: {}MB to migrate, {}MB savings via dedup.",
            plan.total_bytes_to_migrate / 1_048_576,
            plan.dedup_savings_estimate / 1_048_576
        );

        self.phase = MigrationPhase::AwaitingConfirmation;
        self.plan = Some(plan.clone());
        plan
    }

    /// Begin the actual migration (called after user confirmation).
    ///
    /// Each file becomes a Prism Ghost-Write object. Registry entries
    /// become Silo-private K-V store entries. Returns migration summary.
    pub fn execute_migration(&mut self) -> Result<BridgeStats, &'static str> {
        let plan = self.plan.as_ref().ok_or("Q-Bridge: no migration plan")?;
        self.phase = MigrationPhase::Migrating;

        for file in &plan.files_to_migrate {
            // In production: Q-Ring PrismWrite call with file content
            crate::serial_println!(
                "[BRIDGE] Migrating: {} ({} bytes) → Prism",
                file.windows_path, file.size_bytes
            );
            self.stats.files_migrated += 1;
            self.stats.dedup_bytes_saved += if file.class == LegacyFileClass::UserDocument {
                file.size_bytes * 3 / 10
            } else { 0 };
        }

        self.phase = MigrationPhase::ConvertingRegistry;
        for entry in &plan.registry_entries {
            // In production: writes to Silo K-V store via PrismWrite
            self.stats.registry_keys_converted += 1;
        }

        self.phase = MigrationPhase::Complete;
        crate::serial_println!(
            "[BRIDGE] Migration COMPLETE. {} files, {} reg keys. {}MB dedup saved.",
            self.stats.files_migrated,
            self.stats.registry_keys_converted,
            self.stats.dedup_bytes_saved / 1_048_576
        );
        crate::serial_println!(
            "[BRIDGE] Migration Silo will now be vaporized (zero residue)."
        );

        Ok(self.stats.clone())
    }

    /// Abort the migration (user cancelled or Sentinel detected anomaly).
    pub fn abort(&mut self, reason: &'static str) {
        self.phase = MigrationPhase::Aborted;
        crate::serial_println!("[BRIDGE] Migration ABORTED: {}", reason);
    }
}
