//! # Chimera — Win32 Compatibility Bridge (Phase 57)
//!
//! Chimera translates legacy Win32 API calls into Qindows-native
//! Q-Manifest-compliant equivalents without running any Windows code.
//!
//! ## Strategy: Translation, Not Emulation
//!
//! Chimera is NOT a Wine-style NT kernel emulator. It intercepts Win32 calls
//! at the **ABI boundary** and maps them to Qindows primitives:
//!
//! - `CreateFile` → `PrismOpen` (capabilities-gated object handle)
//! - `CreateThread` → `SpawnFiber` (typed Q-Silo fiber)
//! - `VirtualAlloc` → `AllocFrames` + `MapCapPage`
//! - `HeapAlloc` → Qindows slab allocator
//! - `CreateWindow` → `AetherRegister` (Q-Kit window)
//! - `MessageBox` → Aether notification toast
//! - `RegOpenKeyEx` → Prism query (no registry, Prism replaces it)
//! - `socket` / `connect` → Q-Fabric stream
//!
//! ## Architecture Guardian: Law II Compliance
//! Win32 binaries are loaded by the ELF/PE loader with ALL pages marked CoW.
//! Any self-modification attempt triggers a CoW fault → Sentinel enforcement.
//!
//! ## Q-Manifest Law 10: Graceful Degradation
//! Unsupported Win32 calls return `ERROR_NOT_SUPPORTED` and log a stub call
//! for the developer telemetry dashboard — they never crash the Silo.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ── Win32 Error Codes (minimal subset) ───────────────────────────────────────

pub const ERROR_SUCCESS: u32          = 0;
pub const ERROR_FILE_NOT_FOUND: u32   = 2;
pub const ERROR_ACCESS_DENIED: u32    = 5;
pub const ERROR_NOT_ENOUGH_MEMORY: u32 = 8;
pub const ERROR_NOT_SUPPORTED: u32    = 50;
pub const ERROR_INVALID_HANDLE: u32   = 6;
pub const INVALID_HANDLE_VALUE: u64   = u64::MAX;

// ── Win32 Handle Table ────────────────────────────────────────────────────────

/// Maps a Win32 HANDLE (u64) to a Qindows Prism handle (u64).
pub struct HandleTable {
    pub entries: BTreeMap<u64, HandleEntry>,
    next_handle: u64,
}

#[derive(Debug, Clone)]
pub struct HandleEntry {
    /// The Qindows Prism object ID behind this Win32 handle
    pub prism_oid: u64,
    /// Type of object this handle refers to
    pub kind: HandleKind,
    /// Win32 access flags used to open this handle
    pub access: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    File,
    Thread,
    Process,
    Event,
    Mutex,
    Semaphore,
    Socket,
    RegKey,
    Window,
}

impl HandleTable {
    pub fn new() -> Self {
        HandleTable { entries: BTreeMap::new(), next_handle: 4 } // 0-3 reserved (std handles)
    }

    pub fn alloc(&mut self, entry: HandleEntry) -> u64 {
        let h = self.next_handle;
        self.next_handle += 4; // Win32 handles are multiples of 4
        self.entries.insert(h, entry);
        h
    }

    pub fn get(&self, handle: u64) -> Option<&HandleEntry> {
        self.entries.get(&handle)
    }

    pub fn close(&mut self, handle: u64) -> bool {
        self.entries.remove(&handle).is_some()
    }
}

// ── Win32 File API ────────────────────────────────────────────────────────────

/// Chimera translation of `CreateFileA/W` → `PrismOpen`.
///
/// Converts a Win32 file path to a Prism object query.
/// Note: the legacy `C:\Windows\System32` path prefix is stripped —
/// Qindows has no drive letters. Paths are remapped to Prism namespace.
pub fn chimera_create_file(
    handle_table: &mut HandleTable,
    path: &str,
    desired_access: u32,    // Win32 GENERIC_READ | GENERIC_WRITE etc.
    _share_mode: u32,
    _creation_disposition: u32,
) -> u64 {
    // Strip Windows path prefix, convert to Prism-style path
    let prism_path = translate_win32_path(path);

    crate::serial_println!(
        "[CHIMERA] CreateFile: \"{}\" → Prism path \"{}\"",
        path, prism_path
    );

    // In production: call PrismOpen syscall and get a Prism OID back
    // For now: allocate a synthetic handle
    let prism_oid = alloc_synthetic_prism_oid(&prism_path);

    if prism_oid == 0 {
        return INVALID_HANDLE_VALUE;
    }

    handle_table.alloc(HandleEntry {
        prism_oid,
        kind: HandleKind::File,
        access: desired_access,
    })
}

/// Chimera translation of `CloseHandle`.
pub fn chimera_close_handle(handle_table: &mut HandleTable, handle: u64) -> u32 {
    if handle_table.close(handle) {
        ERROR_SUCCESS
    } else {
        ERROR_INVALID_HANDLE
    }
}

/// Chimera translation of `ReadFile` → `PrismRead`.
pub fn chimera_read_file(
    handle_table: &HandleTable,
    handle: u64,
    buf: &mut [u8],
) -> (u32, u32) { // (error_code, bytes_read)
    match handle_table.get(handle) {
        Some(entry) if entry.kind == HandleKind::File => {
            crate::serial_println!("[CHIMERA] ReadFile: handle {} (OID {})", handle, entry.prism_oid);
            // Production: invoke PrismRead syscall
            (ERROR_SUCCESS, 0)
        }
        _ => (ERROR_INVALID_HANDLE, 0),
    }
}

// ── Win32 Thread API ─────────────────────────────────────────────────────────

/// Chimera translation of `CreateThread` → `SpawnFiber`.
///
/// Maps the Win32 thread entry point into a new Qindows fiber within the
/// current Silo. Stack size request is honored if ≤ 64 MiB.
pub fn chimera_create_thread(
    handle_table: &mut HandleTable,
    start_address: u64,
    parameter: u64,
    stack_size: u64,
) -> u64 {
    let effective_stack = stack_size.clamp(64 * 1024, 64 * 1024 * 1024);

    crate::serial_println!(
        "[CHIMERA] CreateThread: entry=0x{:x} param=0x{:x} stack={}KiB",
        start_address, parameter, effective_stack / 1024
    );

    // Production: call SpawnFiber syscall, get fiber_id back
    let fiber_id = synthetic_fiber_id(start_address);

    handle_table.alloc(HandleEntry {
        prism_oid: fiber_id,
        kind: HandleKind::Thread,
        access: 0x1FFFFF, // THREAD_ALL_ACCESS
    })
}

// ── Win32 Memory API ─────────────────────────────────────────────────────────

/// Chimera translation of `VirtualAlloc` → `AllocFrames` + `MapCapPage`.
pub fn chimera_virtual_alloc(
    base_hint: u64,
    size: u64,
    alloc_type: u32,
    protect: u32,
) -> u64 {
    crate::serial_println!(
        "[CHIMERA] VirtualAlloc: hint=0x{:x} size={} type=0x{:x} prot=0x{:x}",
        base_hint, size, alloc_type, protect
    );
    // Production: call AllocFrames + MapCapPage with appropriate permissions
    // Stub: return a plausible user-space address
    if size == 0 { return 0; }
    base_hint.max(0x10000) // Never return NULL-page
}

/// Chimera translation of `VirtualFree` → `FreeFrames`.
pub fn chimera_virtual_free(base: u64, size: u64) -> u32 {
    crate::serial_println!("[CHIMERA] VirtualFree: base=0x{:x} size={}", base, size);
    ERROR_SUCCESS
}

// ── Win32 Registry API (redirected to Prism) ─────────────────────────────────

/// Win32 registry roots mapped to Prism namespaces.
pub const HKEY_LOCAL_MACHINE: u64 = 0x8000_0002;
pub const HKEY_CURRENT_USER: u64  = 0x8000_0001;
pub const HKEY_CLASSES_ROOT: u64  = 0x8000_0000;

/// Chimera translation of `RegOpenKeyEx` → Prism object query.
///
/// Registry keys are mapped to synthetic Prism objects by path.
/// The data lives in Prism — no actual registry hive exists.
pub fn chimera_reg_open_key(
    handle_table: &mut HandleTable,
    root_key: u64,
    sub_key: &str,
) -> (u32, u64) { // (error, hkey)
    let prism_path = translate_reg_path(root_key, sub_key);
    crate::serial_println!("[CHIMERA] RegOpenKey: \"{}\" → Prism \"{}\"", sub_key, prism_path);

    let oid = alloc_synthetic_prism_oid(&prism_path);
    let handle = handle_table.alloc(HandleEntry {
        prism_oid: oid,
        kind: HandleKind::RegKey,
        access: 0x20019, // KEY_READ
    });
    (ERROR_SUCCESS, handle)
}

// ── Win32 Window API (redirected to Aether) ───────────────────────────────────

/// Chimera translation of `CreateWindowEx` → Aether window registration.
pub fn chimera_create_window(
    handle_table: &mut HandleTable,
    class_name: &str,
    window_name: &str,
    width: u32,
    height: u32,
    x: i32,
    y: i32,
) -> u64 {
    crate::serial_println!(
        "[CHIMERA] CreateWindow: \"{}\" class={} {}x{} at ({},{})",
        window_name, class_name, width, height, x, y
    );
    // Production: call AetherRegister syscall
    let window_id = synthetic_window_id(window_name);
    handle_table.alloc(HandleEntry {
        prism_oid: window_id,
        kind: HandleKind::Window,
        access: 0,
    })
}

// ── Stub call tracker (Law X: Graceful Degradation) ──────────────────────────

/// Record of an unimplemented Win32 call (for developer telemetry).
#[derive(Debug, Clone)]
pub struct StubCall {
    pub api_name: &'static str,
    pub call_count: u64,
}

/// Translation metrics for the Sentinel and developer dashboard.
#[derive(Debug, Default, Clone)]
pub struct ChimeraStats {
    pub calls_translated: u64,
    pub calls_stubbed: u64,
    pub handles_allocated: u64,
    pub handles_closed: u64,
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn translate_win32_path(path: &str) -> String {
    // Strip common Windows prefixes
    let stripped = path
        .trim_start_matches("\\\\?\\")
        .trim_start_matches("C:\\Windows\\System32\\")
        .trim_start_matches("C:\\")
        .replace('\\', "/");
    alloc::format!("prism://{}", stripped)
}

fn translate_reg_path(root: u64, sub_key: &str) -> String {
    let prefix = match root {
        HKEY_LOCAL_MACHINE => "prism://registry/HKLM",
        HKEY_CURRENT_USER  => "prism://registry/HKCU",
        _                  => "prism://registry/UNKNOWN",
    };
    alloc::format!("{}/{}", prefix, sub_key.replace('\\', "/"))
}

fn alloc_synthetic_prism_oid(path: &str) -> u64 {
    // Simple hash of path string → synthetic OID (production uses Prism lookup)
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in path.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h & 0x7FFF_FFFF_FFFF_FFFF
}

fn synthetic_fiber_id(entry: u64) -> u64 { entry ^ 0xDEAD_BEEF_0000_0000 }
fn synthetic_window_id(name: &str) -> u64 { alloc_synthetic_prism_oid(name) }
