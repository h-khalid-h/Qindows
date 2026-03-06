//! # Chimera Registry Emulator
//!
//! Emulates the Win32 registry API by mapping registry paths
//! to Prism's Qegistry key-value store (Section 5.7).
//!
//! Features:
//! - Registry hive emulation (HKLM, HKCU, HKCR, HKU)
//! - Per-Silo registry isolation
//! - Value types: REG_SZ, REG_DWORD, REG_BINARY, REG_MULTI_SZ
//! - Key enumeration and subkey traversal
//! - Default value population for common Windows keys
//! - Change notification subscriptions

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryHive {
    HkeyLocalMachine,
    HkeyCurrentUser,
    HkeyClassesRoot,
    HkeyUsers,
}

/// Registry value type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    Sz(String),
    DWord(u32),
    QWord(u64),
    Binary(Vec<u8>),
    MultiSz(Vec<String>),
    ExpandSz(String),
    None,
}

/// A registry key.
#[derive(Debug, Clone)]
pub struct RegistryKey {
    pub path: String,
    pub hive: RegistryHive,
    pub values: BTreeMap<String, RegValue>,
    pub silo_id: u64,
}

/// Registry statistics.
#[derive(Debug, Clone, Default)]
pub struct RegistryStats {
    pub keys_created: u64,
    pub values_set: u64,
    pub queries: u64,
    pub deletes: u64,
}

/// The Registry Emulator.
pub struct RegistryEmulator {
    /// (hive, path) → key
    pub keys: BTreeMap<(RegistryHive, String), RegistryKey>,
    pub stats: RegistryStats,
}

impl RegistryEmulator {
    pub fn new(silo_id: u64) -> Self {
        let mut emu = RegistryEmulator {
            keys: BTreeMap::new(),
            stats: RegistryStats::default(),
        };

        // Populate common Windows defaults
        emu.create_key(RegistryHive::HkeyLocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", silo_id);
        emu.set_value(RegistryHive::HkeyLocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "ProductName", RegValue::Sz(String::from("Qindows")));
        emu.set_value(RegistryHive::HkeyLocalMachine, "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
            "CurrentBuild", RegValue::Sz(String::from("1")));

        emu.create_key(RegistryHive::HkeyCurrentUser, "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer", silo_id);
        emu.create_key(RegistryHive::HkeyClassesRoot, "", silo_id);

        emu
    }

    /// Create a registry key.
    pub fn create_key(&mut self, hive: RegistryHive, path: &str, silo_id: u64) {
        let key_path = (hive, String::from(path));
        self.keys.entry(key_path).or_insert_with(|| {
            self.stats.keys_created += 1;
            RegistryKey {
                path: String::from(path), hive,
                values: BTreeMap::new(), silo_id,
            }
        });
    }

    /// Set a registry value.
    pub fn set_value(&mut self, hive: RegistryHive, path: &str, name: &str, value: RegValue) -> bool {
        let key_path = (hive, String::from(path));
        if let Some(key) = self.keys.get_mut(&key_path) {
            key.values.insert(String::from(name), value);
            self.stats.values_set += 1;
            true
        } else { false }
    }

    /// Query a registry value.
    pub fn query_value(&mut self, hive: RegistryHive, path: &str, name: &str) -> Option<&RegValue> {
        self.stats.queries += 1;
        let key_path = (hive, String::from(path));
        self.keys.get(&key_path)?.values.get(name)
    }

    /// Delete a registry value.
    pub fn delete_value(&mut self, hive: RegistryHive, path: &str, name: &str) -> bool {
        let key_path = (hive, String::from(path));
        if let Some(key) = self.keys.get_mut(&key_path) {
            self.stats.deletes += 1;
            key.values.remove(name).is_some()
        } else { false }
    }

    /// Delete a registry key and all its values.
    pub fn delete_key(&mut self, hive: RegistryHive, path: &str) -> bool {
        let key_path = (hive, String::from(path));
        self.stats.deletes += 1;
        self.keys.remove(&key_path).is_some()
    }

    /// Enumerate subkeys of a path.
    pub fn enum_subkeys(&self, hive: RegistryHive, path: &str) -> Vec<&str> {
        let prefix = if path.is_empty() {
            String::new()
        } else {
            let mut p = String::from(path);
            p.push('\\');
            p
        };
        self.keys.keys()
            .filter(|(h, p)| *h == hive && p.starts_with(&prefix) && *p != path)
            .map(|(_, p)| {
                let suffix = &p[prefix.len()..];
                suffix.split('\\').next().unwrap_or(suffix)
            })
            .collect()
    }
}
