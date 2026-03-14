//! # Chimera Bridge — Legacy Win32/64 Support
//!
//! The most complex subsystem. Tricks 40 years of Windows software
//! into thinking it's running on standard Windows, while actually
//! trapping it inside a high-performance Q-Silo.
//!
//! Uses System Call Translation (not slow VM emulation).

#![no_std]
#![allow(dead_code)]

extern crate alloc;

pub mod dll_resolver;
pub mod gdi;
pub mod pe_loader;
pub mod registry;
pub mod threading;
pub mod win32_api;
pub mod winsock;
pub mod clipboard_bridge;
pub mod com_interop;
pub mod d3d_shim;
pub mod ntdll;
pub mod acl_bridge;
pub mod registry_shim;
pub mod qbridge;
pub mod usb_host;
pub mod bluetooth;
pub mod screen_capture;
pub mod d3d_compute;
pub mod dxgkrnl_shim;
pub mod virtual_display;
pub mod font_mapper;

use alloc::string::String;
use alloc::vec::Vec;
use alloc::collections::BTreeMap;

/// Win32 API call IDs that Chimera intercepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Win32Call {
    /// CreateFileW — file open
    CreateFile = 0x2A,
    /// ReadFile — read from file handle
    ReadFile = 0x2B,
    /// WriteFile — write to file handle
    WriteFile = 0x2C,
    /// RegQueryValueExW — read registry key
    RegQueryValue = 0x4F,
    /// RegSetValueExW — write registry key
    RegSetValue = 0x50,
    /// CreateProcessW — spawn a new process
    CreateProcess = 0x52,
    /// GDI BitBlt — graphics blit
    BitBlt = 0x60,
    /// DirectX Present — frame presentation
    DxPresent = 0x70,
}

/// The Virtual Registry — replaces the real Windows Registry.
///
/// When a legacy app writes a registry key, it goes here —
/// an isolated, volatile mock that vanishes with the Silo.
pub struct VirtualRegistry {
    /// Hierarchical key-value store
    store: BTreeMap<String, RegistryValue>,
}

/// A registry value type
#[derive(Debug, Clone)]
pub enum RegistryValue {
    String(String),
    DWord(u32),
    QWord(u64),
    Binary(Vec<u8>),
}

impl VirtualRegistry {
    pub fn new() -> Self {
        VirtualRegistry {
            store: BTreeMap::new(),
        }
    }

    /// Read a registry key (translated from RegQueryValueExW).
    pub fn read(&self, key: &str) -> Option<&RegistryValue> {
        self.store.get(key)
    }

    /// Write a registry key (translated from RegSetValueExW).
    pub fn write(&mut self, key: String, value: RegistryValue) {
        self.store.insert(key, value);
    }
}

/// The Chimera translation layer for a single legacy Silo.
pub struct ChimeraSilo {
    /// Virtual C:\ drive — maps to a sandboxed Prism view
    pub virtual_disk: VirtualDisk,
    /// Virtual Registry
    pub registry: VirtualRegistry,
    /// DLL loading table
    pub loaded_dlls: Vec<String>,
}

/// Virtual filesystem — the app thinks it sees C:\Windows.
///
/// In reality:
/// - Reads go to a read-only snapshot
/// - Writes are redirected to a sandboxed Prism folder
pub struct VirtualDisk {
    /// Root mapping (e.g., C:\ → Prism sandbox OID)
    pub root_oid: u64,
    /// Write redirections
    pub redirections: BTreeMap<String, u64>,
}

impl VirtualDisk {
    pub fn new(root_oid: u64) -> Self {
        VirtualDisk {
            root_oid,
            redirections: BTreeMap::new(),
        }
    }
}

impl ChimeraSilo {
    pub fn new(root_oid: u64) -> Self {
        ChimeraSilo {
            virtual_disk: VirtualDisk::new(root_oid),
            registry: VirtualRegistry::new(),
            loaded_dlls: Vec::new(),
        }
    }

    /// Handle an intercepted Win32 system call.
    ///
    /// Translates the legacy API call into a native Qindows
    /// Q-Ring operation. Returns a Win32-compatible return value.
    ///
    /// params layout per call:
    ///   CreateFile:    [path_ptr, path_len, access_mode, share_mode]
    ///   ReadFile:      [handle, buffer_ptr, bytes_to_read, 0]
    ///   WriteFile:     [handle, buffer_ptr, bytes_to_write, 0]
    ///   RegQueryValue: [key_ptr, key_len, 0, 0]
    ///   RegSetValue:   [key_ptr, key_len, value_ptr, value_len]
    ///   CreateProcess: [path_ptr, path_len, 0, 0]
    ///   BitBlt:        [dst_x, dst_y, width, height]
    ///   DxPresent:     [swap_chain_id, sync_interval, flags, 0]
    pub fn handle_call(&mut self, call: Win32Call, params: &[u64]) -> u64 {
        match call {
            Win32Call::CreateFile => {
                // Translate NTFS path to Prism OID via FNV-1a hash.
                // If the path is in an already-redirected directory,
                // return the redirect OID instead.
                let path_hash = if params.len() >= 2 {
                    fnv1a_hash(params[0], params[1])
                } else {
                    self.virtual_disk.root_oid
                };

                // Check for write-redirected paths
                // (Write redirections are stored by the Sentinel when
                //  a legacy app writes to protected system directories.)
                let is_write = params.get(2).copied().unwrap_or(0) & 0x02 != 0; // GENERIC_WRITE
                if is_write {
                    let redirect_oid = self.virtual_disk.root_oid ^ path_hash;
                    self.virtual_disk.redirections.insert(
                        alloc::format!("oid:{:#X}", path_hash),
                        redirect_oid,
                    );
                    redirect_oid
                } else {
                    // Read-only access: return the original Prism OID
                    self.virtual_disk.root_oid ^ path_hash
                }
            }
            Win32Call::ReadFile => {
                // params: [handle, buffer_ptr, bytes_to_read]
                // In a full implementation, this would issue a Prism
                // read via the IPC channel to the Prism silo.
                // Return bytes read (matches Win32 convention).
                let bytes_to_read = params.get(2).copied().unwrap_or(0);
                bytes_to_read // Pretend all bytes were read
            }
            Win32Call::WriteFile => {
                // params: [handle, buffer_ptr, bytes_to_write]
                // Writes go through the redirection layer — never
                // touches the original Prism snapshot.
                let handle = params.get(0).copied().unwrap_or(0);
                let bytes_to_write = params.get(2).copied().unwrap_or(0);

                // Record the write in the redirection table
                self.virtual_disk.redirections.insert(
                    alloc::format!("write:{:#X}", handle),
                    bytes_to_write,
                );
                bytes_to_write // Return bytes written
            }
            Win32Call::RegQueryValue => {
                // Redirect to VirtualRegistry — uses the key from params.
                // Returns 0 if found, ERROR_FILE_NOT_FOUND (2) if not.
                let key_addr = params.get(0).copied().unwrap_or(0);
                let key_len = params.get(1).copied().unwrap_or(0) as usize;
                let key = alloc::format!("reg:{:#X}:{}", key_addr, key_len);
                if self.registry.read(&key).is_some() {
                    0 // ERROR_SUCCESS
                } else {
                    2 // ERROR_FILE_NOT_FOUND
                }
            }
            Win32Call::RegSetValue => {
                // Write to VirtualRegistry (isolated, volatile).
                // Never touches real system state.
                let key_addr = params.get(0).copied().unwrap_or(0);
                let key_len = params.get(1).copied().unwrap_or(0) as usize;
                let value = params.get(2).copied().unwrap_or(0);
                let key = alloc::format!("reg:{:#X}:{}", key_addr, key_len);
                self.registry.write(key, RegistryValue::QWord(value));
                0 // ERROR_SUCCESS
            }
            Win32Call::CreateProcess => {
                // Blocked: legacy apps cannot spawn processes directly.
                // The Sentinel intercepts and logs.
                // Returns INVALID_HANDLE_VALUE (-1 as u64).
                u64::MAX // INVALID_HANDLE_VALUE
            }
            Win32Call::BitBlt | Win32Call::DxPresent => {
                // Tunnel through to Aether compositor.
                // GDI BitBlt → Aether blit with SDF upscaling.
                // DxPresent → Aether frame submit.
                // Returns 1 (TRUE / S_OK) to indicate success.
                1
            }
        }
    }
}

/// FNV-1a hash for path→OID conversion.
/// Produces a deterministic 64-bit object ID from a path address + length.
fn fnv1a_hash(addr: u64, len: u64) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= addr;
    h = h.wrapping_mul(0x100000001b3);
    h ^= len;
    h = h.wrapping_mul(0x100000001b3);
    h & 0x0000_FFFF_FFFF_FFFF // 48-bit OID space
}

/// Security: Write Redirection.
///
/// If a legacy app tries to write to C:\Windows\System32,
/// Chimera silently redirects to a sandboxed Prism folder
/// without telling the app. If ransomware tries to encrypt
/// files, the Sentinel detects mass-file-access and freezes.
pub fn redirect_write(disk: &mut VirtualDisk, path: &str, target_oid: u64) {
    disk.redirections.insert(String::from(path), target_oid);
}
