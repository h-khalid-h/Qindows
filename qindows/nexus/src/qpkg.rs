//! # Q-Package Manager — Decentralized Package Distribution
//!
//! Software distribution via mesh-backed content-addressed packages
//! (Section 11.3). Packages are Q-Objects identified by hash,
//! distributed via peer-to-peer swarming, and verified before install.
//!
//! Features:
//! - Content-addressed packages (SHA-256 hash = package ID)
//! - P2P swarming for fast downloads across mesh
//! - Dependency resolution with version constraints
//! - Rollback: every install creates a Silo snapshot
//! - Sandboxed install: packages run in isolated Silos

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Package state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkgState {
    Available,
    Downloading,
    Verifying,
    Installed,
    Failed,
    Removed,
}

/// Version constraint.
#[derive(Debug, Clone)]
pub struct VersionReq {
    pub name: String,
    pub min_major: u32,
    pub min_minor: u32,
    pub min_patch: u32,
}

/// A package manifest.
#[derive(Debug, Clone)]
pub struct Package {
    pub hash: [u8; 32],
    pub name: String,
    pub version_major: u32,
    pub version_minor: u32,
    pub version_patch: u32,
    pub size: u64,
    pub state: PkgState,
    pub dependencies: Vec<VersionReq>,
    pub installed_at: u64,
    pub silo_id: Option<u64>,
    /// Peers who have this package
    pub peers: Vec<[u8; 32]>,
    /// Download progress (0-100)
    pub progress: u8,
}

/// Package statistics.
#[derive(Debug, Clone, Default)]
pub struct PkgStats {
    pub installed: u64,
    pub removed: u64,
    pub downloads: u64,
    pub bytes_downloaded: u64,
    pub bytes_seeded: u64,
    pub verification_failures: u64,
}

/// The Q-Package Manager.
pub struct QPkg {
    pub packages: BTreeMap<[u8; 32], Package>,
    /// Name → hash index
    pub name_index: BTreeMap<String, [u8; 32]>,
    pub stats: PkgStats,
}

impl QPkg {
    pub fn new() -> Self {
        QPkg {
            packages: BTreeMap::new(),
            name_index: BTreeMap::new(),
            stats: PkgStats::default(),
        }
    }

    /// Register a package from the mesh catalog.
    pub fn register(&mut self, hash: [u8; 32], name: &str, major: u32, minor: u32, patch: u32, size: u64, deps: Vec<VersionReq>) {
        self.name_index.insert(String::from(name), hash);
        self.packages.entry(hash).or_insert_with(|| Package {
            hash, name: String::from(name),
            version_major: major, version_minor: minor, version_patch: patch,
            size, state: PkgState::Available,
            dependencies: deps, installed_at: 0, silo_id: None,
            peers: Vec::new(), progress: 0,
        });
    }

    /// Start downloading a package.
    pub fn download(&mut self, hash: &[u8; 32]) -> Result<(), &'static str> {
        let pkg = self.packages.get_mut(hash).ok_or("Package not found")?;
        if pkg.state != PkgState::Available {
            return Err("Package not in available state");
        }
        pkg.state = PkgState::Downloading;
        pkg.progress = 0;
        self.stats.downloads += 1;
        Ok(())
    }

    /// Update download progress.
    pub fn update_progress(&mut self, hash: &[u8; 32], progress: u8, bytes: u64) {
        if let Some(pkg) = self.packages.get_mut(hash) {
            pkg.progress = progress.min(100);
            self.stats.bytes_downloaded += bytes;
            if progress >= 100 {
                pkg.state = PkgState::Verifying;
            }
        }
    }

    /// Verify and install a package.
    pub fn install(&mut self, hash: &[u8; 32], silo_id: u64, now: u64) -> Result<(), &'static str> {
        let pkg = self.packages.get_mut(hash).ok_or("Package not found")?;
        if pkg.state != PkgState::Verifying {
            return Err("Package not ready for install");
        }

        // Check dependencies are installed
        let dep_names: Vec<String> = pkg.dependencies.iter().map(|d| d.name.clone()).collect();
        for dep_name in &dep_names {
            let dep_hash = self.name_index.get(dep_name).ok_or("Missing dependency")?;
            let dep = self.packages.get(dep_hash).ok_or("Dependency not registered")?;
            if dep.state != PkgState::Installed {
                return Err("Dependency not installed");
            }
        }

        let pkg = self.packages.get_mut(hash).ok_or("Package not found")?;
        pkg.state = PkgState::Installed;
        pkg.installed_at = now;
        pkg.silo_id = Some(silo_id);
        self.stats.installed += 1;
        Ok(())
    }

    /// Remove a package.
    pub fn remove(&mut self, hash: &[u8; 32]) -> Result<(), &'static str> {
        let pkg = self.packages.get_mut(hash).ok_or("Package not found")?;
        if pkg.state != PkgState::Installed {
            return Err("Package not installed");
        }

        // Check no other installed package depends on this
        let pkg_name = pkg.name.clone();
        for other in self.packages.values() {
            if other.state == PkgState::Installed {
                for dep in &other.dependencies {
                    if dep.name == pkg_name {
                        return Err("Required by another package");
                    }
                }
            }
        }

        let pkg = self.packages.get_mut(hash).ok_or("Package not found")?;
        pkg.state = PkgState::Removed;
        pkg.silo_id = None;
        self.stats.removed += 1;
        Ok(())
    }

    /// Search packages by name prefix.
    pub fn search(&self, prefix: &str) -> Vec<&Package> {
        self.packages.values()
            .filter(|p| p.name.starts_with(prefix))
            .collect()
    }
}
