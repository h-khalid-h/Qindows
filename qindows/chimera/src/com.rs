//! # Chimera COM (Component Object Model) Emulation
//!
//! Emulates COM interfaces for legacy Win32 apps.
//! Provides IUnknown, class factory, and interface
//! query machinery with reference counting.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

/// A COM GUID (128-bit unique identifier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

impl Guid {
    pub const fn new(d1: u32, d2: u16, d3: u16, d4: [u8; 8]) -> Self {
        Guid { data1: d1, data2: d2, data3: d3, data4: d4 }
    }

    pub const ZERO: Guid = Guid::new(0, 0, 0, [0; 8]);
}

/// Well-known GUIDs.
pub mod guids {
    use super::Guid;
    pub const IID_IUNKNOWN: Guid = Guid::new(
        0x00000000, 0x0000, 0x0000, [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46]
    );
    pub const IID_ICLASS_FACTORY: Guid = Guid::new(
        0x00000001, 0x0000, 0x0000, [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46]
    );
    pub const IID_IDISPATCH: Guid = Guid::new(
        0x00020400, 0x0000, 0x0000, [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46]
    );
    pub const IID_IPERSIST: Guid = Guid::new(
        0x0000010C, 0x0000, 0x0000, [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46]
    );
}

/// COM HRESULT values.
pub mod hresult {
    pub const S_OK: i32 = 0;
    pub const S_FALSE: i32 = 1;
    pub const E_NOINTERFACE: i32 = -2147467262; // 0x80004002
    pub const E_POINTER: i32 = -2147467261;     // 0x80004003
    pub const E_FAIL: i32 = -2147467259;        // 0x80004005
    pub const E_OUTOFMEMORY: i32 = -2147024882; // 0x8007000E
    pub const E_INVALIDARG: i32 = -2147024809;  // 0x80070057
    pub const CLASS_E_NOAGGREGATION: i32 = -2147221232; // 0x80040110
    pub const REGDB_E_CLASSNOTREG: i32 = -2147221164;   // 0x80040154
}

/// A COM interface entry (vtable slot).
#[derive(Debug, Clone)]
pub struct InterfaceEntry {
    /// Interface GUID
    pub iid: Guid,
    /// Interface name (for debugging)
    pub name: String,
}

/// A COM object instance.
pub struct ComObject {
    /// Class GUID
    pub clsid: Guid,
    /// Supported interfaces
    pub interfaces: Vec<InterfaceEntry>,
    /// Reference count
    pub ref_count: AtomicU32,
    /// Properties (key-value store for object state)
    pub properties: BTreeMap<String, String>,
}

impl ComObject {
    pub fn new(clsid: Guid) -> Self {
        ComObject {
            clsid,
            interfaces: alloc::vec![
                InterfaceEntry { iid: guids::IID_IUNKNOWN, name: String::from("IUnknown") },
            ],
            ref_count: AtomicU32::new(1),
            properties: BTreeMap::new(),
        }
    }

    /// IUnknown::AddRef
    pub fn add_ref(&self) -> u32 {
        self.ref_count.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// IUnknown::Release
    pub fn release(&self) -> u32 {
        let prev = self.ref_count.fetch_sub(1, Ordering::Release);
        if prev == 1 {
            core::sync::atomic::fence(Ordering::Acquire);
            // Object should be destroyed — ref count hit 0
        }
        prev - 1
    }

    /// IUnknown::QueryInterface
    pub fn query_interface(&self, iid: &Guid) -> i32 {
        if self.interfaces.iter().any(|i| i.iid == *iid) {
            self.add_ref();
            hresult::S_OK
        } else {
            hresult::E_NOINTERFACE
        }
    }

    /// Add a supported interface.
    pub fn add_interface(&mut self, iid: Guid, name: &str) {
        self.interfaces.push(InterfaceEntry {
            iid,
            name: String::from(name),
        });
    }
}

/// A registered COM class.
#[derive(Debug, Clone)]
pub struct ComClass {
    /// Class GUID
    pub clsid: Guid,
    /// ProgID (e.g. "Excel.Application")
    pub prog_id: String,
    /// Description
    pub description: String,
    /// Threading model
    pub threading: ThreadingModel,
}

/// COM threading model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadingModel {
    Apartment,
    Free,
    Both,
    Neutral,
}

/// The COM Runtime.
pub struct ComRuntime {
    /// Registered classes: CLSID → class info
    pub classes: BTreeMap<Guid, ComClass>,
    /// ProgID → CLSID mapping
    pub prog_ids: BTreeMap<String, Guid>,
    /// Live objects
    pub objects: Vec<u64>, // Object handles
    /// Next object handle
    next_handle: u64,
    /// Stats
    pub stats: ComStats,
}

/// COM statistics.
#[derive(Debug, Clone, Default)]
pub struct ComStats {
    pub objects_created: u64,
    pub objects_destroyed: u64,
    pub query_interface_calls: u64,
    pub class_not_found: u64,
}

impl ComRuntime {
    pub fn new() -> Self {
        ComRuntime {
            classes: BTreeMap::new(),
            prog_ids: BTreeMap::new(),
            objects: Vec::new(),
            next_handle: 1,
            stats: ComStats::default(),
        }
    }

    /// Register a COM class (CoRegisterClassObject).
    pub fn register_class(&mut self, clsid: Guid, prog_id: &str, desc: &str, threading: ThreadingModel) {
        let class = ComClass {
            clsid,
            prog_id: String::from(prog_id),
            description: String::from(desc),
            threading,
        };
        self.prog_ids.insert(String::from(prog_id), clsid);
        self.classes.insert(clsid, class);
    }

    /// Create an instance (CoCreateInstance).
    pub fn create_instance(&mut self, clsid: &Guid) -> Result<ComObject, i32> {
        if !self.classes.contains_key(clsid) {
            self.stats.class_not_found += 1;
            return Err(hresult::REGDB_E_CLASSNOTREG);
        }

        let obj = ComObject::new(*clsid);
        self.stats.objects_created += 1;
        Ok(obj)
    }

    /// Look up a CLSID by ProgID (CLSIDFromProgID).
    pub fn clsid_from_prog_id(&self, prog_id: &str) -> Option<Guid> {
        self.prog_ids.get(prog_id).copied()
    }
}
