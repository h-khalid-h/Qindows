//! # Chimera Registry Shim — Win32 Registry → Qegistry
//!
//! Legacy Win32 apps call `RegOpenKeyEx`, `RegSetValueEx`, etc.
//! This shim intercepts those calls and transparently redirects
//! them to the Prism Qegistry (versioned config store).
//!
//! Mapping:
//! - `HKEY_LOCAL_MACHINE\SOFTWARE\...` → `/system/chimera/hklm/...`
//! - `HKEY_CURRENT_USER\...` → `/silo/{id}/chimera/hkcu/...`
//! - `HKEY_CLASSES_ROOT\...` → `/system/chimera/hkcr/...`
//! - REG_DWORD → QValue::Int
//! - REG_SZ → QValue::Str
//! - REG_BINARY → QValue::Bytes

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Win32 registry root keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HKey {
    LocalMachine,    // HKEY_LOCAL_MACHINE
    CurrentUser,     // HKEY_CURRENT_USER
    ClassesRoot,     // HKEY_CLASSES_ROOT
    Users,           // HKEY_USERS
    CurrentConfig,   // HKEY_CURRENT_CONFIG
}

impl HKey {
    /// Map to Qegistry path prefix.
    pub fn to_qegistry_prefix(&self, silo_id: u64) -> String {
        match self {
            HKey::LocalMachine => String::from("/system/chimera/hklm"),
            HKey::CurrentUser => alloc::format!("/silo/{}/chimera/hkcu", silo_id),
            HKey::ClassesRoot => String::from("/system/chimera/hkcr"),
            HKey::Users => String::from("/system/chimera/hku"),
            HKey::CurrentConfig => String::from("/system/chimera/hkcc"),
        }
    }
}

/// Win32 registry value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegType {
    None,      // REG_NONE
    Sz,        // REG_SZ (string)
    ExpandSz,  // REG_EXPAND_SZ
    Binary,    // REG_BINARY
    Dword,     // REG_DWORD (u32)
    Qword,     // REG_QWORD (u64)
    MultiSz,   // REG_MULTI_SZ
}

/// A registry value.
#[derive(Debug, Clone)]
pub struct RegValue {
    /// Value name
    pub name: String,
    /// Value type
    pub reg_type: RegType,
    /// Raw data
    pub data: Vec<u8>,
}

impl RegValue {
    /// Create a DWORD value.
    pub fn dword(name: &str, value: u32) -> Self {
        RegValue {
            name: String::from(name),
            reg_type: RegType::Dword,
            data: value.to_le_bytes().to_vec(),
        }
    }

    /// Create a string value.
    pub fn sz(name: &str, value: &str) -> Self {
        // Win32 strings are UTF-16LE null-terminated
        let mut data: Vec<u8> = Vec::new();
        for ch in value.encode_utf16() {
            data.extend_from_slice(&ch.to_le_bytes());
        }
        data.extend_from_slice(&[0, 0]); // Null terminator
        RegValue { name: String::from(name), reg_type: RegType::Sz, data }
    }

    /// Create a binary value.
    pub fn binary(name: &str, data: Vec<u8>) -> Self {
        RegValue { name: String::from(name), reg_type: RegType::Binary, data }
    }

    /// Get as u32 (DWORD).
    pub fn as_dword(&self) -> Option<u32> {
        if self.data.len() >= 4 {
            Some(u32::from_le_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else { None }
    }

    /// Get as string (SZ).
    pub fn as_string(&self) -> Option<String> {
        if self.reg_type != RegType::Sz && self.reg_type != RegType::ExpandSz {
            return None;
        }
        // Decode UTF-16LE
        let u16s: Vec<u16> = self.data.chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        Some(String::from_utf16_lossy(&u16s))
    }
}

/// An open registry key handle.
#[derive(Debug, Clone)]
pub struct RegKeyHandle {
    /// Handle value
    pub handle: u64,
    /// Root key
    pub root: HKey,
    /// Subkey path (e.g., "SOFTWARE\\Microsoft\\Windows")
    pub subkey: String,
    /// Full Qegistry path
    pub qegistry_path: String,
    /// Owning Silo
    pub silo_id: u64,
}

/// Registry shim statistics.
#[derive(Debug, Clone, Default)]
pub struct RegistryStats {
    pub keys_opened: u64,
    pub keys_created: u64,
    pub values_read: u64,
    pub values_written: u64,
    pub keys_deleted: u64,
    pub keys_enumerated: u64,
}

/// The Registry Shim.
pub struct RegistryShim {
    /// Open key handles
    pub handles: BTreeMap<u64, RegKeyHandle>,
    /// Stored values (qegistry_path/name → value)
    pub values: BTreeMap<String, RegValue>,
    /// Subkeys (qegistry_path → list of child names)
    pub subkeys: BTreeMap<String, Vec<String>>,
    /// Next handle
    next_handle: u64,
    /// Statistics
    pub stats: RegistryStats,
}

impl RegistryShim {
    pub fn new() -> Self {
        RegistryShim {
            handles: BTreeMap::new(),
            values: BTreeMap::new(),
            subkeys: BTreeMap::new(),
            next_handle: 0x80000000,
            stats: RegistryStats::default(),
        }
    }

    /// Convert a Win32 registry path to a Qegistry path.
    fn to_qpath(&self, root: HKey, subkey: &str, silo_id: u64) -> String {
        let prefix = root.to_qegistry_prefix(silo_id);
        let clean = subkey.replace('\\', "/").to_lowercase();
        if clean.is_empty() {
            prefix
        } else {
            alloc::format!("{}/{}", prefix, clean)
        }
    }

    /// RegOpenKeyEx — open a registry key.
    pub fn open_key(&mut self, root: HKey, subkey: &str, silo_id: u64) -> Option<u64> {
        let qpath = self.to_qpath(root, subkey, silo_id);

        let handle = self.next_handle;
        self.next_handle += 1;

        self.handles.insert(handle, RegKeyHandle {
            handle,
            root,
            subkey: String::from(subkey),
            qegistry_path: qpath,
            silo_id,
        });

        self.stats.keys_opened += 1;
        Some(handle)
    }

    /// RegCreateKeyEx — create or open a registry key.
    pub fn create_key(&mut self, root: HKey, subkey: &str, silo_id: u64) -> u64 {
        let qpath = self.to_qpath(root, subkey, silo_id);

        // Ensure parent subkey entries exist
        let parts: Vec<&str> = subkey.split('\\').collect();
        if parts.len() > 1 {
            let parent = parts[..parts.len() - 1].join("\\");
            let parent_qpath = self.to_qpath(root, &parent, silo_id);
            let child = String::from(parts[parts.len() - 1]);
            self.subkeys.entry(parent_qpath).or_insert_with(Vec::new)
                .push(child);
        }

        self.stats.keys_created += 1;
        self.open_key(root, subkey, silo_id).unwrap_or(0)
    }

    /// RegSetValueEx — set a value under an open key.
    pub fn set_value(&mut self, handle: u64, value: RegValue) -> bool {
        let key = match self.handles.get(&handle) {
            Some(k) => k.qegistry_path.clone(),
            None => return false,
        };
        let full_path = alloc::format!("{}/{}", key, value.name);
        self.values.insert(full_path, value);
        self.stats.values_written += 1;
        true
    }

    /// RegQueryValueEx — read a value.
    pub fn get_value(&mut self, handle: u64, name: &str) -> Option<&RegValue> {
        let key = self.handles.get(&handle)?;
        let full_path = alloc::format!("{}/{}", key.qegistry_path, name);
        self.stats.values_read += 1;
        self.values.get(&full_path)
    }

    /// RegDeleteValue — delete a value.
    pub fn delete_value(&mut self, handle: u64, name: &str) -> bool {
        let key = match self.handles.get(&handle) {
            Some(k) => k.qegistry_path.clone(),
            None => return false,
        };
        let full_path = alloc::format!("{}/{}", key, name);
        self.values.remove(&full_path).is_some()
    }

    /// RegEnumKeyEx — enumerate subkeys.
    pub fn enum_keys(&mut self, handle: u64) -> Vec<String> {
        self.stats.keys_enumerated += 1;
        let key = match self.handles.get(&handle) {
            Some(k) => k.qegistry_path.clone(),
            None => return Vec::new(),
        };
        self.subkeys.get(&key).cloned().unwrap_or_default()
    }

    /// RegCloseKey — close a key handle.
    pub fn close_key(&mut self, handle: u64) {
        self.handles.remove(&handle);
    }

    /// Clean up all handles for a Silo (on Silo termination).
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        let to_remove: Vec<u64> = self.handles.iter()
            .filter(|(_, h)| h.silo_id == silo_id)
            .map(|(&id, _)| id)
            .collect();
        for id in to_remove {
            self.handles.remove(&id);
        }
    }
}
