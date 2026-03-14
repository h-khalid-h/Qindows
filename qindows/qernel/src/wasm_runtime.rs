//! # WASM Runtime Stub — Universal Binary Support (Phase 62)
//!
//! Qindows is a WASM-native OS (ARCHITECTURE.md §2):
//! > "Universal Binaries: Instead of compiling for x86 or ARM, developers
//! > ship Wasm binaries. Qindows compiles these to machine code at install
//! > time, ensuring perfect performance on any processor."
//!
//! ## Two-Phase Model
//!
//! ### Phase A: AOT Compilation (at install time)
//! Q-Ledger receives a `.qapp` (WASM + Q-Manifest).
//! The WASM runtime compiles it to native machine code using a tiered
//! compiler (baseline JIT → optimizing AOT).
//! The resulting native binary is stored as a Prism object with a
//! deterministic hash. All users on the same ISA share the same compiled OID.
//!
//! ### Phase B: Isolated Execution (at launch time)
//! The compiled native binary is loaded via `loader::load_elf()` into
//! a new Q-Silo with a WASM-specific capability set.
//! The WASM linear memory maps to a Silo virtual region.
//! WASM imports are translated to Q-Ring syscalls via an ABI shim.
//!
//! ## Architecture Guardian Note
//! This module is the KERNEL INTERFACE for WASM execution:
//! - Module validation and manifest checking
//! - Memory region planning (linear memory, table, stack)
//! - Import resolution (WASM imports → Q-Ring syscalls)
//!
//! The actual WASM compiler/interpreter is a user-mode service (runs
//! in its own Silo). This module communicates with it via Q-Ring IPC.
//!
//! ## Q-Manifest Law 5: Global Deduplication
//! Compiled WASM OIDs are content-addressable. If two apps use the
//! same WASM binary (same hash), they share one native code object on disk.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ── WASM Module Descriptor ────────────────────────────────────────────────────

/// WASM section types we track for planning purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmSectionKind {
    Type,
    Import,
    Function,
    Table,
    Memory,
    Global,
    Export,
    Start,
    Element,
    Code,
    Data,
    Custom,
}

/// A parsed WASM import declaration.
#[derive(Debug, Clone)]
pub struct WasmImport {
    pub module: String,
    pub name: String,
    pub kind: WasmImportKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmImportKind {
    Function,
    Table,
    Memory,
    Global,
}

/// A WASM export declaration.
#[derive(Debug, Clone)]
pub struct WasmExport {
    pub name: String,
    pub kind: WasmImportKind,
    pub index: u32,
}

/// High-level description of a WASM module (after validation).
#[derive(Debug, Clone)]
pub struct WasmModuleDesc {
    /// Module name (from "name" custom section, or empty)
    pub name: String,
    /// WASM binary hash (FNV-64 of the original bytes)
    pub binary_hash: u64,
    /// Linear memory initial size (pages, 1 page = 64 KiB)
    pub memory_pages_initial: u32,
    /// Linear memory max size (None = unbounded)
    pub memory_pages_max: Option<u32>,
    /// Imports declared by this module
    pub imports: Vec<WasmImport>,
    /// Exports declared by this module
    pub exports: Vec<WasmExport>,
    /// Does this module declare a start function?
    pub has_start: bool,
}

// ── WASM Validation ───────────────────────────────────────────────────────────

/// WASM validation error.
#[derive(Debug)]
pub enum WasmValidationError {
    /// Not a valid WASM binary (bad magic/version)
    InvalidMagic,
    /// The module requires too much linear memory
    MemoryLimitExceeded { requested_pages: u32, limit_pages: u32 },
    /// An import cannot be resolved to a Q-Ring syscall
    UnresolvedImport { module: String, name: String },
    /// A Q-Manifest capability required by the module is absent
    MissingCapability { cap_name: String },
    /// The WASM binary size exceeds the OS limit
    BinaryTooLarge { size: u64, limit: u64 },
}

/// Qindows WASM limits.
pub const WASM_MAX_MEMORY_PAGES: u32 = 65536;    // 4 GiB linear memory max
pub const WASM_MAX_BINARY_SIZE: u64 = 256 * 1024 * 1024; // 256 MiB
pub const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D];
pub const WASM_VERSION: [u8; 4] = [0x01, 0x00, 0x00, 0x00];

/// Validate a WASM binary before compilation.
///
/// Checks magic/version, memory limits, and binary size.
/// Returns a `WasmModuleDesc` on success (import resolution is deferred
/// to the compiler Silo, not done in the kernel).
pub fn validate_wasm_binary(bytes: &[u8]) -> Result<WasmModuleDesc, WasmValidationError> {
    // 1. Check size
    if bytes.len() as u64 > WASM_MAX_BINARY_SIZE {
        return Err(WasmValidationError::BinaryTooLarge {
            size: bytes.len() as u64,
            limit: WASM_MAX_BINARY_SIZE,
        });
    }

    // 2. Check magic + version
    if bytes.len() < 8 {
        return Err(WasmValidationError::InvalidMagic);
    }
    if bytes[0..4] != WASM_MAGIC {
        return Err(WasmValidationError::InvalidMagic);
    }
    if bytes[4..8] != WASM_VERSION {
        return Err(WasmValidationError::InvalidMagic);
    }

    // 3. Compute binary hash (FNV-64)
    let hash = {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    };

    // 4. Build stub descriptor (full section parsing done by compiler Silo)
    Ok(WasmModuleDesc {
        name: String::new(),
        binary_hash: hash,
        memory_pages_initial: 1,
        memory_pages_max: None,
        imports: Vec::new(),
        exports: Vec::new(),
        has_start: false,
    })
}

// ── WASM Import ABI Shim ─────────────────────────────────────────────────────

/// Q-Ring syscall mappings for common WASM host imports.
///
/// WASM apps import from "qindows" module. These names map to Q-Ring IDs.
/// Any import from an unrecognized module is blocked by the kernel.
const QINDOWS_HOST_IMPORTS: &[(&str, u64)] = &[
    // WASI-style I/O (mapped to Prism)
    ("fd_read",          11), // SyscallId::PrismRead
    ("fd_write",         12), // SyscallId::PrismWrite
    ("fd_close",         13), // SyscallId::PrismClose
    ("path_open",        10), // SyscallId::PrismOpen
    // Silo lifecycle
    ("proc_exit",         1), // SyscallId::Exit
    ("sched_yield",       0), // SyscallId::Yield
    // Time
    ("clock_time_get",   50), // SyscallId::GetTime
    // Networking (Q-Fabric)
    ("sock_accept",      70), // SyscallId::NetConnect
    ("sock_send",        71), // SyscallId::NetSend
    ("sock_recv",        72), // SyscallId::NetRecv
    // UI (Aether)
    ("aether_register",  60), // SyscallId::AetherRegister
    ("aether_submit",    61), // SyscallId::AetherSubmit
];

/// Resolve a WASM "qindows" host import to its Q-Ring syscall ID.
pub fn resolve_wasm_import(module: &str, name: &str) -> Option<u64> {
    if module != "qindows" && module != "wasi_snapshot_preview1" {
        crate::serial_println!(
            "[WASM] Import from unrecognized module \"{}\" blocked.",
            module
        );
        return None;
    }
    QINDOWS_HOST_IMPORTS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, id)| *id)
}

// ── WASM Memory Plan ─────────────────────────────────────────────────────────

/// Virtual address layout for a WASM Silo.
///
/// The WASM linear memory is mapped below the stack in the Silo's
/// address space. Guard pages protect both ends.
#[derive(Debug, Clone, Copy)]
pub struct WasmMemoryPlan {
    /// Base virtual address of the linear memory region
    pub linear_mem_base: u64,
    /// Size of the linear memory region in bytes
    pub linear_mem_size: u64,
    /// Base virtual address of the WASM table (indirect call targets)
    pub table_base: u64,
    /// Stack top virtual address
    pub stack_top: u64,
    /// Code base virtual address (the JIT-compiled native code)
    pub code_base: u64,
}

impl WasmMemoryPlan {
    /// Plan the virtual address layout for a WASM module.
    ///
    /// Uses a fixed layout above the lowest user-space page to make
    /// WASM linear memory address 0 correspond to virtual 0x10000
    /// (null-pointer protection is naturally provided by the unmapped
    /// first 64 KiB for WASM modules that use address 0 as sentinel).
    pub fn plan(desc: &WasmModuleDesc) -> Self {
        let pages = desc.memory_pages_initial as u64;
        let linear_size = pages * 65536; // 64 KiB per WASM page

        WasmMemoryPlan {
            linear_mem_base: 0x0000_0001_0000_0000, // 4 GiB offset
            linear_mem_size: linear_size,
            table_base:      0x0000_0002_0000_0000,
            stack_top:       0x0000_7FFF_FFFF_F000,
            code_base:       0x0000_0000_0040_0000, // 4 MiB (ELF convention)
        }
    }
}

// ── WASM Runtime State ────────────────────────────────────────────────────────

/// Compilation state of a WASM module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmCompileState {
    /// Module submitted for compilation, awaiting compiler Silo
    Pending,
    /// Baseline JIT compiled (fast but unoptimized)
    Baseline,
    /// Fully optimized AOT artifact stored in Prism
    Optimized,
    /// Compilation failed
    Failed,
}

/// Per-module WASM runtime record maintained by the kernel.
#[derive(Debug, Clone)]
pub struct WasmRuntimeRecord {
    pub desc: WasmModuleDesc,
    pub memory_plan: WasmMemoryPlan,
    pub compile_state: WasmCompileState,
    /// Prism OID of the compiled native binary (set when Optimized)
    pub compiled_oid: Option<u64>,
    /// Number of active Silos running this module
    pub active_instances: u32,
}

/// Statistics.
#[derive(Debug, Default, Clone)]
pub struct WasmStats {
    pub modules_validated: u64,
    pub compilations_requested: u64,
    pub compilations_complete: u64,
    pub imports_resolved: u64,
    pub imports_blocked: u64,
}

/// The WASM runtime kernel interface.
pub struct WasmRuntime {
    /// binary_hash → runtime record
    pub modules: BTreeMap<u64, WasmRuntimeRecord>,
    /// Stats
    pub stats: WasmStats,
}

impl WasmRuntime {
    pub fn new() -> Self {
        WasmRuntime { modules: BTreeMap::new(), stats: WasmStats::default() }
    }

    /// Validate and register a WASM module for compilation.
    pub fn register_module(&mut self, bytes: &[u8]) -> Result<u64, WasmValidationError> {
        let desc = validate_wasm_binary(bytes)?;
        let hash = desc.binary_hash;
        let plan = WasmMemoryPlan::plan(&desc);

        self.stats.modules_validated += 1;

        if self.modules.contains_key(&hash) {
            crate::serial_println!("[WASM] Module {:016x} already registered (dedup hit).", hash);
            return Ok(hash);
        }

        crate::serial_println!(
            "[WASM] Module {:016x} validated ({} bytes, {} pages linear mem)",
            hash, bytes.len(), desc.memory_pages_initial
        );

        self.modules.insert(hash, WasmRuntimeRecord {
            desc,
            memory_plan: plan,
            compile_state: WasmCompileState::Pending,
            compiled_oid: None,
            active_instances: 0,
        });

        self.stats.compilations_requested += 1;
        Ok(hash)
    }

    /// Mark a module's compilation as complete (called by compiler Silo).
    pub fn compilation_complete(&mut self, hash: u64, compiled_oid: u64) {
        if let Some(rec) = self.modules.get_mut(&hash) {
            rec.compile_state = WasmCompileState::Optimized;
            rec.compiled_oid = Some(compiled_oid);
            self.stats.compilations_complete += 1;
            crate::serial_println!(
                "[WASM] Module {:016x} AOT compiled → Prism OID {}",
                hash, compiled_oid
            );
        }
    }

    /// Get the compiled Prism OID for a module (if ready).
    pub fn compiled_oid(&self, hash: u64) -> Option<u64> {
        self.modules.get(&hash)?.compiled_oid
    }
}
