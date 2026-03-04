//! # Chimera Virtual Registry
//!
//! Emulates the Windows Registry for legacy apps.
//! Registry reads/writes are intercepted and stored as Prism
//! objects within the Silo's namespace — completely isolated.
//!
//! Key mappings:
//!   HKEY_LOCAL_MACHINE → /chimera/registry/HKLM/
//!   HKEY_CURRENT_USER  → /chimera/registry/HKCU/
//!   HKEY_CLASSES_ROOT  → /chimera/registry/HKCR/

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Registry value types (matching Windows REG_* constants).
#[derive(Debug, Clone)]
pub enum RegValue {
    /// REG_SZ — String value
    String(String),
    /// REG_DWORD — 32-bit integer
    Dword(u32),
    /// REG_QWORD — 64-bit integer
    Qword(u64),
    /// REG_BINARY — Raw bytes
    Binary(Vec<u8>),
    /// REG_MULTI_SZ — Array of strings
    MultiString(Vec<String>),
    /// REG_EXPAND_SZ — String with environment variable references
    ExpandString(String),
    /// REG_NONE — No value
    None,
}

/// A registry key (like a directory).
#[derive(Debug, Clone)]
pub struct RegKey {
    /// Subkeys
    pub subkeys: BTreeMap<String, RegKey>,
    /// Values
    pub values: BTreeMap<String, RegValue>,
}

impl RegKey {
    pub fn new() -> Self {
        RegKey {
            subkeys: BTreeMap::new(),
            values: BTreeMap::new(),
        }
    }
}

/// Registry hive roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hive {
    LocalMachine,    // HKLM
    CurrentUser,     // HKCU
    ClassesRoot,     // HKCR
    Users,           // HKU
    CurrentConfig,   // HKCC
}

impl Hive {
    pub fn from_str(s: &str) -> Option<Hive> {
        match s.to_uppercase().as_str() {
            "HKEY_LOCAL_MACHINE" | "HKLM" => Some(Hive::LocalMachine),
            "HKEY_CURRENT_USER" | "HKCU" => Some(Hive::CurrentUser),
            "HKEY_CLASSES_ROOT" | "HKCR" => Some(Hive::ClassesRoot),
            "HKEY_USERS" | "HKU" => Some(Hive::Users),
            "HKEY_CURRENT_CONFIG" | "HKCC" => Some(Hive::CurrentConfig),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Hive::LocalMachine => "HKEY_LOCAL_MACHINE",
            Hive::CurrentUser => "HKEY_CURRENT_USER",
            Hive::ClassesRoot => "HKEY_CLASSES_ROOT",
            Hive::Users => "HKEY_USERS",
            Hive::CurrentConfig => "HKEY_CURRENT_CONFIG",
        }
    }
}

/// Registry errors.
#[derive(Debug, Clone)]
pub enum RegError {
    KeyNotFound,
    ValueNotFound,
    InvalidPath,
    AccessDenied,
    InvalidType,
}

/// The Virtual Registry.
pub struct VirtualRegistry {
    /// Hive roots
    hives: BTreeMap<Hive, RegKey>,
    /// Silo ID this registry belongs to
    pub silo_id: u64,
    /// Access statistics
    pub stats: RegStats,
}

/// Registry access statistics.
#[derive(Debug, Clone, Default)]
pub struct RegStats {
    pub reads: u64,
    pub writes: u64,
    pub key_creates: u64,
    pub key_deletes: u64,
    pub not_found: u64,
}

impl VirtualRegistry {
    /// Create a new virtual registry for a Silo.
    pub fn new(silo_id: u64) -> Self {
        let mut hives = BTreeMap::new();

        // Pre-populate common keys
        let mut hklm = RegKey::new();
        let mut software = RegKey::new();
        let mut microsoft = RegKey::new();
        let mut windows = RegKey::new();
        let mut cv = RegKey::new();

        cv.values.insert(String::from("ProductName"), RegValue::String(
            String::from("Qindows (Chimera Compatibility)")
        ));
        cv.values.insert(String::from("CurrentMajorVersionNumber"), RegValue::Dword(10));
        cv.values.insert(String::from("CurrentMinorVersionNumber"), RegValue::Dword(0));
        cv.values.insert(String::from("CurrentBuildNumber"), RegValue::String(
            String::from("26000")
        ));
        cv.values.insert(String::from("EditionID"), RegValue::String(
            String::from("Professional")
        ));

        windows.subkeys.insert(String::from("CurrentVersion"), cv);
        let mut nt = RegKey::new();
        nt.subkeys.insert(String::from("Windows NT"), windows);
        microsoft.subkeys.insert(String::from("Microsoft"), nt.clone());
        software.subkeys.insert(String::from("Software"), microsoft.clone());

        // System key
        let mut system = RegKey::new();
        let mut current_control = RegKey::new();
        let mut control = RegKey::new();
        control.values.insert(String::from("SystemStartOptions"), RegValue::String(
            String::from("QINDOWS_CHIMERA")
        ));
        current_control.subkeys.insert(String::from("Control"), control);
        system.subkeys.insert(String::from("CurrentControlSet"), current_control);
        software.subkeys.insert(String::from("SYSTEM"), system);

        hklm.subkeys = software.subkeys;
        hives.insert(Hive::LocalMachine, hklm);

        // HKCU
        let mut hkcu = RegKey::new();
        let mut user_software = RegKey::new();
        user_software.subkeys.insert(String::from("Microsoft"), microsoft);
        hkcu.subkeys.insert(String::from("Software"), user_software);
        hives.insert(Hive::CurrentUser, hkcu);

        // Empty roots for others
        hives.insert(Hive::ClassesRoot, RegKey::new());
        hives.insert(Hive::Users, RegKey::new());
        hives.insert(Hive::CurrentConfig, RegKey::new());

        VirtualRegistry {
            hives,
            silo_id,
            stats: RegStats::default(),
        }
    }

    /// Navigate to a key by path.
    fn navigate(&self, hive: Hive, path: &str) -> Result<&RegKey, RegError> {
        let root = self.hives.get(&hive).ok_or(RegError::KeyNotFound)?;

        if path.is_empty() {
            return Ok(root);
        }

        let parts: Vec<&str> = path.split('\\')
            .filter(|p| !p.is_empty())
            .collect();

        let mut current = root;
        for part in parts {
            current = current.subkeys.get(part).ok_or(RegError::KeyNotFound)?;
        }

        Ok(current)
    }

    /// Navigate to a key mutably.
    fn navigate_mut(&mut self, hive: Hive, path: &str) -> Result<&mut RegKey, RegError> {
        let root = self.hives.get_mut(&hive).ok_or(RegError::KeyNotFound)?;

        if path.is_empty() {
            return Ok(root);
        }

        let parts: Vec<&str> = path.split('\\')
            .filter(|p| !p.is_empty())
            .collect();

        let mut current = root;
        for part in parts {
            current = current.subkeys.get_mut(part).ok_or(RegError::KeyNotFound)?;
        }

        Ok(current)
    }

    /// Read a registry value.
    pub fn read(&mut self, hive: Hive, path: &str, name: &str) -> Result<&RegValue, RegError> {
        self.stats.reads += 1;
        let key = self.navigate(hive, path)?;
        key.values.get(name).ok_or_else(|| {
            self.stats.not_found += 1;
            RegError::ValueNotFound
        })
    }

    /// Write a registry value.
    pub fn write(&mut self, hive: Hive, path: &str, name: &str, value: RegValue) -> Result<(), RegError> {
        self.stats.writes += 1;
        let key = self.navigate_mut(hive, path)?;
        key.values.insert(String::from(name), value);
        Ok(())
    }

    /// Create a registry key (and any missing parents).
    pub fn create_key(&mut self, hive: Hive, path: &str) -> Result<(), RegError> {
        self.stats.key_creates += 1;

        let parts: Vec<&str> = path.split('\\')
            .filter(|p| !p.is_empty())
            .collect();

        let root = self.hives.get_mut(&hive).ok_or(RegError::KeyNotFound)?;
        let mut current = root;

        for part in parts {
            if !current.subkeys.contains_key(part) {
                current.subkeys.insert(String::from(part), RegKey::new());
            }
            current = current.subkeys.get_mut(part).unwrap();
        }

        Ok(())
    }

    /// Enumerate subkeys of a key.
    pub fn enum_keys(&self, hive: Hive, path: &str) -> Result<Vec<String>, RegError> {
        let key = self.navigate(hive, path)?;
        Ok(key.subkeys.keys().cloned().collect())
    }

    /// Enumerate values of a key.
    pub fn enum_values(&self, hive: Hive, path: &str) -> Result<Vec<String>, RegError> {
        let key = self.navigate(hive, path)?;
        Ok(key.values.keys().cloned().collect())
    }

    /// Parse a full registry path like "HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft".
    pub fn parse_path(full_path: &str) -> Option<(Hive, String)> {
        let parts: Vec<&str> = full_path.splitn(2, '\\').collect();
        if parts.is_empty() { return None; }

        let hive = Hive::from_str(parts[0])?;
        let path = if parts.len() > 1 { String::from(parts[1]) } else { String::new() };
        Some((hive, path))
    }
}
