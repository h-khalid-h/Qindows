//! # Chimera DLL Import Resolver
//!
//! Resolves PE import tables by mapping Win32 DLL functions
//! to Chimera thunks. When a legacy app calls `CreateFileW`,
//! the thunk redirects to `chimera::win32_api::kernel32::create_file`.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A resolved import entry.
#[derive(Debug, Clone)]
pub struct ImportEntry {
    /// DLL name (e.g., "kernel32.dll")
    pub dll: String,
    /// Function name (e.g., "CreateFileW")
    pub function: String,
    /// Thunk address (Chimera handler)
    pub thunk_addr: u64,
    /// Ordinal (if imported by ordinal instead of name)
    pub ordinal: Option<u16>,
    /// Is this function implemented?
    pub implemented: bool,
}

/// DLL resolution result.
#[derive(Debug, Clone)]
pub enum ResolveResult {
    /// Resolved to a Chimera thunk
    Resolved(u64),
    /// Stubbed (logs a warning but doesn't crash)
    Stubbed,
    /// Not found (will cause app to fail)
    NotFound,
}

/// The DLL Import Resolver.
pub struct ImportResolver {
    /// Registered thunks: (dll_name, function_name) → thunk_addr
    thunks: BTreeMap<(String, String), u64>,
    /// Stubbed functions (return 0 / S_OK)
    stubs: Vec<(String, String)>,
    /// Unresolved imports (for error reporting)
    pub unresolved: Vec<ImportEntry>,
    /// Resolution statistics
    pub stats: ResolveStats,
}

/// Resolution statistics.
#[derive(Debug, Clone, Default)]
pub struct ResolveStats {
    pub total_imports: u64,
    pub resolved: u64,
    pub stubbed: u64,
    pub failed: u64,
}

impl ImportResolver {
    pub fn new() -> Self {
        let mut resolver = ImportResolver {
            thunks: BTreeMap::new(),
            stubs: Vec::new(),
            unresolved: Vec::new(),
            stats: ResolveStats::default(),
        };

        resolver.register_defaults();
        resolver
    }

    /// Register the default Win32 API thunks.
    fn register_defaults(&mut self) {
        // kernel32.dll
        let k32 = "kernel32.dll";
        self.register(k32, "CreateFileW", 0x7000_0001);
        self.register(k32, "ReadFile", 0x7000_0002);
        self.register(k32, "WriteFile", 0x7000_0003);
        self.register(k32, "CloseHandle", 0x7000_0004);
        self.register(k32, "GetLastError", 0x7000_0005);
        self.register(k32, "SetLastError", 0x7000_0006);
        self.register(k32, "CreateMutexW", 0x7000_0007);
        self.register(k32, "CreateEventW", 0x7000_0008);
        self.register(k32, "WaitForSingleObject", 0x7000_0009);
        self.register(k32, "GetCurrentProcess", 0x7000_000A);
        self.register(k32, "GetCurrentThread", 0x7000_000B);
        self.register(k32, "ExitProcess", 0x7000_000C);
        self.register(k32, "GetModuleHandleW", 0x7000_000D);
        self.register(k32, "GetProcAddress", 0x7000_000E);
        self.register(k32, "LoadLibraryW", 0x7000_000F);
        self.register(k32, "VirtualAlloc", 0x7000_0010);
        self.register(k32, "VirtualFree", 0x7000_0011);
        self.register(k32, "HeapAlloc", 0x7000_0012);
        self.register(k32, "HeapFree", 0x7000_0013);
        self.register(k32, "GetSystemInfo", 0x7000_0014);

        // user32.dll
        let u32_dll = "user32.dll";
        self.register(u32_dll, "CreateWindowExW", 0x7100_0001);
        self.register(u32_dll, "ShowWindow", 0x7100_0002);
        self.register(u32_dll, "DestroyWindow", 0x7100_0003);
        self.register(u32_dll, "SendMessageW", 0x7100_0004);
        self.register(u32_dll, "PostMessageW", 0x7100_0005);
        self.register(u32_dll, "GetMessageW", 0x7100_0006);
        self.register(u32_dll, "DispatchMessageW", 0x7100_0007);
        self.register(u32_dll, "DefWindowProcW", 0x7100_0008);
        self.register(u32_dll, "MessageBoxW", 0x7100_0009);
        self.register(u32_dll, "GetCursorPos", 0x7100_000A);
        self.register(u32_dll, "SetCursorPos", 0x7100_000B);
        self.register(u32_dll, "RegisterClassExW", 0x7100_000C);

        // gdi32.dll
        let gdi = "gdi32.dll";
        self.register(gdi, "CreateCompatibleDC", 0x7200_0001);
        self.register(gdi, "DeleteDC", 0x7200_0002);
        self.register(gdi, "BitBlt", 0x7200_0003);
        self.register(gdi, "SelectObject", 0x7200_0004);
        self.register(gdi, "CreateSolidBrush", 0x7200_0005);
        self.register(gdi, "CreateFontW", 0x7200_0006);
        self.register(gdi, "TextOutW", 0x7200_0007);
        self.register(gdi, "GetDeviceCaps", 0x7200_0008);

        // ntdll.dll (stubs — most NT internals are not needed)
        self.stub("ntdll.dll", "RtlInitUnicodeString");
        self.stub("ntdll.dll", "NtQueryInformationProcess");
        self.stub("ntdll.dll", "RtlGetVersion");

        // advapi32.dll
        self.register("advapi32.dll", "RegOpenKeyExW", 0x7300_0001);
        self.register("advapi32.dll", "RegQueryValueExW", 0x7300_0002);
        self.register("advapi32.dll", "RegCloseKey", 0x7300_0003);
        self.register("advapi32.dll", "RegSetValueExW", 0x7300_0004);
    }

    /// Register a thunk for a DLL function.
    pub fn register(&mut self, dll: &str, function: &str, thunk_addr: u64) {
        self.thunks.insert(
            (dll.to_lowercase().into(), function.into()),
            thunk_addr,
        );
    }

    /// Register a stub (returns 0).
    pub fn stub(&mut self, dll: &str, function: &str) {
        self.stubs.push((dll.to_lowercase().into(), function.into()));
    }

    /// Resolve a single import.
    pub fn resolve(&mut self, dll: &str, function: &str) -> ResolveResult {
        self.stats.total_imports += 1;
        let key = (dll.to_lowercase(), String::from(function));

        if let Some(&addr) = self.thunks.get(&key) {
            self.stats.resolved += 1;
            ResolveResult::Resolved(addr)
        } else if self.stubs.contains(&key) {
            self.stats.stubbed += 1;
            ResolveResult::Stubbed
        } else {
            self.stats.failed += 1;
            self.unresolved.push(ImportEntry {
                dll: String::from(dll),
                function: String::from(function),
                thunk_addr: 0,
                ordinal: None,
                implemented: false,
            });
            ResolveResult::NotFound
        }
    }

    /// Resolve all imports from a PE import directory.
    pub fn resolve_all(&mut self, imports: &[(String, Vec<String>)]) -> Vec<ImportEntry> {
        let mut resolved = Vec::new();

        for (dll, functions) in imports {
            for func in functions {
                let result = self.resolve(dll, func);
                resolved.push(ImportEntry {
                    dll: dll.clone(),
                    function: func.clone(),
                    thunk_addr: match result {
                        ResolveResult::Resolved(addr) => addr,
                        _ => 0,
                    },
                    ordinal: None,
                    implemented: matches!(result, ResolveResult::Resolved(_)),
                });
            }
        }

        resolved
    }

    /// Get the number of registered thunks.
    pub fn thunk_count(&self) -> usize {
        self.thunks.len()
    }

    /// Get the resolution success rate.
    pub fn success_rate(&self) -> f64 {
        if self.stats.total_imports == 0 { return 100.0; }
        (self.stats.resolved + self.stats.stubbed) as f64
            / self.stats.total_imports as f64 * 100.0
    }
}
