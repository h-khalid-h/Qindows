//! # Prism Encryption Key Rotation
//!
//! Manages key lifecycle: generation, rotation, re-encryption,
//! and retirement. Tracks key versions so old data can still
//! be decrypted during the migration window.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Key state in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// Key is active for new encryptions
    Active,
    /// Key is being phased out — decrypt only, no new encryptions
    Retiring,
    /// Key is retired — kept for emergency decryption only
    Retired,
    /// Key has been destroyed (metadata only)
    Destroyed,
}

/// A versioned encryption key.
#[derive(Clone)]
pub struct VersionedKey {
    /// Key generation number
    pub generation: u32,
    /// Raw key bytes (256-bit)
    bytes: [u8; 32],
    /// Key state
    pub state: KeyState,
    /// Creation timestamp
    pub created_at: u64,
    /// Retirement timestamp (if retired)
    pub retired_at: Option<u64>,
    /// Number of objects encrypted with this key
    pub objects_encrypted: u64,
    /// Number of objects re-encrypted to next key
    pub objects_migrated: u64,
}

impl VersionedKey {
    pub fn new(bytes: [u8; 32], generation: u32, now: u64) -> Self {
        VersionedKey {
            generation,
            bytes,
            state: KeyState::Active,
            created_at: now,
            retired_at: None,
            objects_encrypted: 0,
            objects_migrated: 0,
        }
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Securely zeroize key material.
    pub fn zeroize(&mut self) {
        for byte in &mut self.bytes {
            unsafe { core::ptr::write_volatile(byte, 0); }
        }
        self.state = KeyState::Destroyed;
    }
}

impl Drop for VersionedKey {
    fn drop(&mut self) {
        self.zeroize();
    }
}

/// A re-encryption job (one object that needs migration).
#[derive(Debug, Clone)]
pub struct ReEncryptionJob {
    /// Object ID
    pub oid: u64,
    /// Current key generation
    pub from_generation: u32,
    /// Target key generation
    pub to_generation: u32,
    /// Object size in bytes
    pub size: u64,
    /// Status
    pub status: JobStatus,
}

/// Re-encryption job status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    InProgress,
    Complete,
    Failed,
}

/// Rotation policy.
#[derive(Debug, Clone)]
pub struct RotationPolicy {
    /// Auto-rotate after this many seconds
    pub max_age_seconds: u64,
    /// Auto-rotate after encrypting this many objects
    pub max_objects: u64,
    /// Maximum number of retired keys to keep
    pub max_retired_keys: usize,
    /// Re-encryption batch size
    pub batch_size: usize,
}

impl Default for RotationPolicy {
    fn default() -> Self {
        RotationPolicy {
            max_age_seconds: 30 * 24 * 3600, // 30 days
            max_objects: 1_000_000,
            max_retired_keys: 5,
            batch_size: 100,
        }
    }
}

/// The Key Rotation Manager.
pub struct KeyRotationManager {
    /// All key versions (generation → key)
    pub keys: BTreeMap<u32, VersionedKey>,
    /// Current active generation
    pub active_generation: u32,
    /// Re-encryption queue
    pub queue: Vec<ReEncryptionJob>,
    /// Rotation policy
    pub policy: RotationPolicy,
    /// Stats
    pub stats: RotationStats,
}

/// Rotation statistics.
#[derive(Debug, Clone, Default)]
pub struct RotationStats {
    pub rotations_performed: u64,
    pub objects_re_encrypted: u64,
    pub bytes_re_encrypted: u64,
    pub keys_destroyed: u64,
    pub failed_re_encryptions: u64,
}

impl KeyRotationManager {
    pub fn new(initial_key: [u8; 32], now: u64) -> Self {
        let mut keys = BTreeMap::new();
        keys.insert(1, VersionedKey::new(initial_key, 1, now));

        KeyRotationManager {
            keys,
            active_generation: 1,
            queue: Vec::new(),
            policy: RotationPolicy::default(),
            stats: RotationStats::default(),
        }
    }

    /// Get the active encryption key.
    pub fn active_key(&self) -> Option<&VersionedKey> {
        self.keys.get(&self.active_generation)
    }

    /// Get a key by generation (for decryption of old data).
    pub fn key_for_generation(&self, generation: u32) -> Option<&VersionedKey> {
        self.keys.get(&generation).filter(|k| k.state != KeyState::Destroyed)
    }

    /// Rotate to a new key.
    pub fn rotate(&mut self, new_key: [u8; 32], now: u64) -> u32 {
        // Retire current active key
        if let Some(current) = self.keys.get_mut(&self.active_generation) {
            current.state = KeyState::Retiring;
            current.retired_at = Some(now);
        }

        // Create new generation
        let new_gen = self.active_generation + 1;
        self.keys.insert(new_gen, VersionedKey::new(new_key, new_gen, now));
        self.active_generation = new_gen;
        self.stats.rotations_performed += 1;

        // Prune old retired keys beyond policy
        self.prune_old_keys();

        new_gen
    }

    /// Check if rotation is needed.
    pub fn needs_rotation(&self, now: u64) -> bool {
        if let Some(active) = self.active_key() {
            let age = now.saturating_sub(active.created_at);
            if age > self.policy.max_age_seconds * 1_000_000_000 {
                return true;
            }
            if active.objects_encrypted >= self.policy.max_objects {
                return true;
            }
        }
        false
    }

    /// Enqueue objects for re-encryption after a key rotation.
    pub fn enqueue_re_encryption(&mut self, oids: &[u64], from_gen: u32, sizes: &[u64]) {
        let to_gen = self.active_generation;
        for (i, &oid) in oids.iter().enumerate() {
            self.queue.push(ReEncryptionJob {
                oid,
                from_generation: from_gen,
                to_generation: to_gen,
                size: sizes.get(i).copied().unwrap_or(0),
                status: JobStatus::Pending,
            });
        }
    }

    /// Process the next batch of re-encryption jobs.
    /// Returns (completed_count, failed_count).
    pub fn process_batch(&mut self) -> (usize, usize) {
        let batch_size = self.policy.batch_size;
        let mut completed = 0;
        let mut failed = 0;

        for job in self.queue.iter_mut().take(batch_size) {
            if job.status != JobStatus::Pending { continue; }

            job.status = JobStatus::InProgress;

            // Verify we have both keys
            let has_source = self.keys.contains_key(&job.from_generation);
            let has_target = self.keys.contains_key(&job.to_generation);

            if has_source && has_target {
                // In production: decrypt with old key, encrypt with new key
                job.status = JobStatus::Complete;
                completed += 1;
                self.stats.objects_re_encrypted += 1;
                self.stats.bytes_re_encrypted += job.size;

                // Track migration on source key
                if let Some(old_key) = self.keys.get_mut(&job.from_generation) {
                    old_key.objects_migrated += 1;
                }
            } else {
                job.status = JobStatus::Failed;
                failed += 1;
                self.stats.failed_re_encryptions += 1;
            }
        }

        // Remove completed/failed jobs
        self.queue.retain(|j| j.status == JobStatus::Pending);

        (completed, failed)
    }

    /// Finalize a retiring key once all its objects are re-encrypted.
    pub fn finalize_retirement(&mut self, generation: u32) {
        if let Some(key) = self.keys.get_mut(&generation) {
            if key.state == KeyState::Retiring && key.objects_migrated >= key.objects_encrypted {
                key.state = KeyState::Retired;
            }
        }
    }

    /// Prune old retired keys beyond the policy limit.
    fn prune_old_keys(&mut self) {
        let mut retired: Vec<u32> = self.keys.iter()
            .filter(|(_, k)| k.state == KeyState::Retired)
            .map(|(gen, _)| *gen)
            .collect();

        // Sort oldest first
        retired.sort();

        while retired.len() > self.policy.max_retired_keys {
            if let Some(oldest) = retired.first().copied() {
                if let Some(key) = self.keys.get_mut(&oldest) {
                    key.zeroize();
                }
                self.keys.remove(&oldest);
                self.stats.keys_destroyed += 1;
                retired.remove(0);
            }
        }
    }

    /// Get the number of pending re-encryption jobs.
    pub fn pending_jobs(&self) -> usize {
        self.queue.len()
    }

    /// Get rotation progress (0.0 - 1.0).
    pub fn rotation_progress(&self) -> f32 {
        if let Some(retiring) = self.keys.values().find(|k| k.state == KeyState::Retiring) {
            if retiring.objects_encrypted == 0 { return 1.0; }
            retiring.objects_migrated as f32 / retiring.objects_encrypted as f32
        } else {
            1.0 // No rotation in progress
        }
    }
}
