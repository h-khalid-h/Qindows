//! # Q-Bridge — Legacy Data Migration
//!
//! First-boot migration tool (Section 10.1). Deep-scans legacy Windows
//! drives, deduplicates files into Prism Q-Objects, and translates
//! settings into the Qegistry.
//!
//! Migration flow:
//! 1. Detect legacy drives (NTFS partitions)
//! 2. Scan all files → build content hash index
//! 3. Deduplicate: 450 GB typical → ~310 GB of Q-Objects
//! 4. Import user settings (Registry → Qegistry)
//! 5. Create Prism semantic index (tag by content type)
//! 6. Optionally preserve original partition (read-only snapshot)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Migration state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationState {
    /// Not started
    Idle,
    /// Scanning legacy drive
    Scanning,
    /// Deduplicating files
    Deduplicating,
    /// Importing settings
    ImportingSettings,
    /// Building semantic index
    Indexing,
    /// Complete
    Complete,
    /// Failed
    Failed,
}

/// A legacy file found during scan.
#[derive(Debug, Clone)]
pub struct LegacyFile {
    /// Original path (e.g., "C:\\Users\\John\\Documents\\report.docx")
    pub path: String,
    /// File size (bytes)
    pub size: u64,
    /// Content hash (SHA-256)
    pub hash: [u8; 32],
    /// MIME type (detected from extension/magic bytes)
    pub mime_type: String,
    /// Last modified timestamp
    pub modified: u64,
    /// Was this file deduplicated? (had a duplicate)
    pub deduplicated: bool,
    /// Assigned Q-Object ID
    pub qobject_id: Option<u64>,
}

/// A legacy registry key to import.
#[derive(Debug, Clone)]
pub struct LegacyRegKey {
    /// Registry path
    pub path: String,
    /// Value name
    pub name: String,
    /// Value (serialized as string)
    pub value: String,
    /// Imported to Qegistry?
    pub imported: bool,
}

/// Content category (for semantic tagging).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentCategory {
    Document,
    Image,
    Video,
    Audio,
    Code,
    Archive,
    Database,
    Executable,
    Config,
    Other,
}

impl ContentCategory {
    pub fn from_extension(ext: &str) -> Self {
        match ext.to_lowercase().as_str() {
            "doc" | "docx" | "pdf" | "txt" | "rtf" | "odt" | "xls" | "xlsx" | "pptx" => ContentCategory::Document,
            "jpg" | "jpeg" | "png" | "gif" | "bmp" | "svg" | "webp" | "ico" => ContentCategory::Image,
            "mp4" | "avi" | "mkv" | "mov" | "wmv" | "flv" | "webm" => ContentCategory::Video,
            "mp3" | "wav" | "flac" | "ogg" | "aac" | "wma" => ContentCategory::Audio,
            "rs" | "py" | "js" | "ts" | "c" | "cpp" | "h" | "java" | "go" | "cs" => ContentCategory::Code,
            "zip" | "tar" | "gz" | "7z" | "rar" | "bz2" => ContentCategory::Archive,
            "db" | "sqlite" | "mdb" | "sql" => ContentCategory::Database,
            "exe" | "dll" | "msi" | "bat" | "cmd" => ContentCategory::Executable,
            "ini" | "cfg" | "conf" | "toml" | "yaml" | "json" | "xml" => ContentCategory::Config,
            _ => ContentCategory::Other,
        }
    }
}

/// Migration statistics.
#[derive(Debug, Clone, Default)]
pub struct MigrationStats {
    pub files_scanned: u64,
    pub files_migrated: u64,
    pub files_deduplicated: u64,
    pub bytes_original: u64,
    pub bytes_after_dedup: u64,
    pub registry_keys_imported: u64,
    pub categories: BTreeMap<u8, u64>, // category ordinal → count
}

/// The Q-Bridge Migration Engine.
pub struct QBridge {
    /// Current state
    pub state: MigrationState,
    /// Scanned files
    pub files: Vec<LegacyFile>,
    /// Registry keys to import
    pub reg_keys: Vec<LegacyRegKey>,
    /// Dedup index (hash → first file index)
    pub dedup_index: BTreeMap<[u8; 32], usize>,
    /// Next Q-Object ID
    next_oid: u64,
    /// Statistics
    pub stats: MigrationStats,
}

impl QBridge {
    pub fn new() -> Self {
        QBridge {
            state: MigrationState::Idle,
            files: Vec::new(),
            reg_keys: Vec::new(),
            dedup_index: BTreeMap::new(),
            next_oid: 1,
            stats: MigrationStats::default(),
        }
    }

    /// Start the migration process.
    pub fn start(&mut self) {
        self.state = MigrationState::Scanning;
    }

    /// Add a discovered file during scanning.
    pub fn add_file(&mut self, path: &str, size: u64, hash: [u8; 32], mime: &str, modified: u64) {
        self.files.push(LegacyFile {
            path: String::from(path),
            size,
            hash,
            mime_type: String::from(mime),
            modified,
            deduplicated: false,
            qobject_id: None,
        });
        self.stats.files_scanned += 1;
        self.stats.bytes_original += size;
    }

    /// Run deduplication pass.
    pub fn deduplicate(&mut self) {
        self.state = MigrationState::Deduplicating;

        for i in 0..self.files.len() {
            let hash = self.files[i].hash;

            if let Some(&first_idx) = self.dedup_index.get(&hash) {
                // Duplicate found — point to the same Q-Object
                self.files[i].deduplicated = true;
                self.files[i].qobject_id = self.files[first_idx].qobject_id;
                self.stats.files_deduplicated += 1;
            } else {
                // New unique file — assign Q-Object ID
                let oid = self.next_oid;
                self.next_oid += 1;
                self.files[i].qobject_id = Some(oid);
                self.dedup_index.insert(hash, i);
                self.stats.bytes_after_dedup += self.files[i].size;
            }
            self.stats.files_migrated += 1;
        }
    }

    /// Add a registry key for import.
    pub fn add_reg_key(&mut self, path: &str, name: &str, value: &str) {
        self.reg_keys.push(LegacyRegKey {
            path: String::from(path),
            name: String::from(name),
            value: String::from(value),
            imported: false,
        });
    }

    /// Import registry keys to Qegistry.
    pub fn import_settings(&mut self) {
        self.state = MigrationState::ImportingSettings;
        for key in &mut self.reg_keys {
            // In production: call Qegistry.set() with translated paths
            key.imported = true;
            self.stats.registry_keys_imported += 1;
        }
    }

    /// Build semantic index (categorize files).
    pub fn build_index(&mut self) {
        self.state = MigrationState::Indexing;
        for file in &self.files {
            let ext = file.path.rsplit('.').next().unwrap_or("");
            let cat = ContentCategory::from_extension(ext);
            *self.stats.categories.entry(cat as u8).or_insert(0) += 1;
        }
    }

    /// Run the full migration pipeline.
    pub fn run_full(&mut self) {
        self.deduplicate();
        self.import_settings();
        self.build_index();
        self.state = MigrationState::Complete;
    }

    /// Get space saved by deduplication.
    pub fn space_saved(&self) -> u64 {
        self.stats.bytes_original.saturating_sub(self.stats.bytes_after_dedup)
    }

    /// Get dedup ratio (e.g., 0.68 = 68% of original size).
    pub fn dedup_ratio(&self) -> f32 {
        if self.stats.bytes_original == 0 { return 1.0; }
        self.stats.bytes_after_dedup as f32 / self.stats.bytes_original as f32
    }
}
