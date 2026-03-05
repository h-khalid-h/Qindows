//! # Chimera COM Interop
//!
//! Component Object Model (COM) interop layer for Chimera.
//! Legacy Windows apps use COM extensively — OLE automation,
//! ActiveX, Shell extensions, DirectShow, WMI, etc.
//!
//! This module provides:
//! - IUnknown / IDispatch vtable emulation
//! - CLSID → Qindows service mapping
//! - Reference counting (AddRef/Release)
//! - QueryInterface dispatch
//! - Apartment threading model (STA/MTA)
//! - COM class factory stubs

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ─── COM GUIDs ──────────────────────────────────────────────────────────────

/// A COM GUID (128-bit identifier).
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

    /// The null GUID.
    pub const ZERO: Guid = Guid::new(0, 0, 0, [0; 8]);

    /// IUnknown IID.
    pub const IID_IUNKNOWN: Guid = Guid::new(
        0x00000000, 0x0000, 0x0000,
        [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    );

    /// IDispatch IID.
    pub const IID_IDISPATCH: Guid = Guid::new(
        0x00020400, 0x0000, 0x0000,
        [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    );

    /// IClassFactory IID.
    pub const IID_ICLASSFACTORY: Guid = Guid::new(
        0x00000001, 0x0000, 0x0000,
        [0xC0, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x46],
    );

    /// Format as registry string: {D1-D2-D3-D4}.
    pub fn to_string(&self) -> String {
        alloc::format!(
            "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            self.data1, self.data2, self.data3,
            self.data4[0], self.data4[1],
            self.data4[2], self.data4[3], self.data4[4],
            self.data4[5], self.data4[6], self.data4[7],
        )
    }
}

// ─── COM Interfaces ─────────────────────────────────────────────────────────

/// COM HRESULT codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum HResult {
    /// Success
    SOk = 0,
    /// Success, false return
    SFalse = 1,
    /// Interface not supported
    ENoInterface = -2147467262, // 0x80004002
    /// Not implemented
    ENotImpl = -2147467263, // 0x80004001
    /// Out of memory
    EOutOfMemory = -2147024882, // 0x8007000E
    /// Invalid argument
    EInvalidArg = -2147024809, // 0x80070057
    /// Class not registered
    EClassNotRegistered = -2147221164, // 0x80040154
    /// Unexpected failure
    EUnexpected = -2147418113, // 0x8000FFFF
}

/// COM threading model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApartmentModel {
    /// Single-Threaded Apartment
    STA,
    /// Multi-Threaded Apartment
    MTA,
    /// Both
    Both,
    /// Free (neutral)
    Free,
}

/// A registered COM class.
#[derive(Debug, Clone)]
pub struct ComClass {
    /// Class ID (CLSID)
    pub clsid: Guid,
    /// Human-readable name
    pub name: String,
    /// ProgID (e.g., "Excel.Application")
    pub prog_id: Option<String>,
    /// Threading model
    pub apartment: ApartmentModel,
    /// Supported interfaces (IIDs)
    pub interfaces: Vec<Guid>,
    /// Is this an in-process server?
    pub in_process: bool,
    /// Reference count of active instances
    pub active_instances: u32,
}

/// A live COM object instance.
#[derive(Debug, Clone)]
pub struct ComObject {
    /// Instance ID
    pub id: u64,
    /// Class ID
    pub clsid: Guid,
    /// Reference count
    pub ref_count: u32,
    /// Owning Silo
    pub silo_id: u64,
    /// Apartment this object lives in
    pub apartment: ApartmentModel,
    /// Is this object still alive?
    pub alive: bool,
}

impl ComObject {
    /// IUnknown::AddRef
    pub fn add_ref(&mut self) -> u32 {
        self.ref_count = self.ref_count.saturating_add(1);
        self.ref_count
    }

    /// IUnknown::Release
    pub fn release(&mut self) -> u32 {
        self.ref_count = self.ref_count.saturating_sub(1);
        if self.ref_count == 0 {
            self.alive = false;
        }
        self.ref_count
    }

    /// IUnknown::QueryInterface
    pub fn query_interface(&self, iid: &Guid, class: &ComClass) -> HResult {
        if *iid == Guid::IID_IUNKNOWN {
            return HResult::SOk;
        }
        if *iid == Guid::IID_IDISPATCH {
            // Only if the class supports IDispatch
            if class.interfaces.contains(&Guid::IID_IDISPATCH) {
                return HResult::SOk;
            }
        }
        if class.interfaces.contains(iid) {
            return HResult::SOk;
        }
        HResult::ENoInterface
    }
}

// ─── COM Manager ────────────────────────────────────────────────────────────

/// COM manager statistics.
#[derive(Debug, Clone, Default)]
pub struct ComStats {
    pub classes_registered: u64,
    pub objects_created: u64,
    pub objects_released: u64,
    pub query_interface_calls: u64,
    pub co_create_calls: u64,
    pub class_not_found: u64,
}

/// The COM Interop Manager.
pub struct ComManager {
    /// Registered classes (CLSID → class info)
    pub classes: BTreeMap<Guid, ComClass>,
    /// ProgID → CLSID mapping
    pub prog_id_map: BTreeMap<String, Guid>,
    /// Active COM objects
    pub objects: Vec<ComObject>,
    /// Next object ID
    next_obj_id: u64,
    /// Statistics
    pub stats: ComStats,
}

impl ComManager {
    pub fn new() -> Self {
        ComManager {
            classes: BTreeMap::new(),
            prog_id_map: BTreeMap::new(),
            objects: Vec::new(),
            next_obj_id: 1,
            stats: ComStats::default(),
        }
    }

    /// Register a COM class.
    pub fn register_class(&mut self, class: ComClass) {
        if let Some(ref prog_id) = class.prog_id {
            self.prog_id_map.insert(prog_id.clone(), class.clsid);
        }
        self.classes.insert(class.clsid, class);
        self.stats.classes_registered += 1;
    }

    /// CoCreateInstance — create a COM object.
    pub fn co_create_instance(
        &mut self,
        clsid: &Guid,
        silo_id: u64,
    ) -> Result<u64, HResult> {
        self.stats.co_create_calls += 1;

        let apartment = match self.classes.get(clsid) {
            Some(class) => {
                // Increment active instances
                class.apartment
            }
            None => {
                self.stats.class_not_found += 1;
                return Err(HResult::EClassNotRegistered);
            }
        };

        // Increment active instances
        if let Some(class) = self.classes.get_mut(clsid) {
            class.active_instances += 1;
        }

        let id = self.next_obj_id;
        self.next_obj_id += 1;

        self.objects.push(ComObject {
            id,
            clsid: *clsid,
            ref_count: 1,
            silo_id,
            apartment,
            alive: true,
        });

        self.stats.objects_created += 1;
        Ok(id)
    }

    /// CLSIDFromProgID — look up CLSID from ProgID.
    pub fn clsid_from_prog_id(&self, prog_id: &str) -> Option<Guid> {
        self.prog_id_map.get(prog_id).copied()
    }

    /// Release an object (decrements ref count).
    pub fn release_object(&mut self, obj_id: u64) -> Option<u32> {
        if let Some(obj) = self.objects.iter_mut().find(|o| o.id == obj_id) {
            let remaining = obj.release();
            if remaining == 0 {
                // Decrement active instances on class
                if let Some(class) = self.classes.get_mut(&obj.clsid) {
                    class.active_instances = class.active_instances.saturating_sub(1);
                }
                self.stats.objects_released += 1;
            }
            Some(remaining)
        } else {
            None
        }
    }

    /// Garbage-collect dead objects.
    pub fn gc(&mut self) -> usize {
        let before = self.objects.len();
        self.objects.retain(|o| o.alive);
        before - self.objects.len()
    }

    /// Release all objects for a Silo (cleanup on Silo death).
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        for obj in &mut self.objects {
            if obj.silo_id == silo_id && obj.alive {
                obj.alive = false;
                obj.ref_count = 0;
                if let Some(class) = self.classes.get_mut(&obj.clsid) {
                    class.active_instances = class.active_instances.saturating_sub(1);
                }
                self.stats.objects_released += 1;
            }
        }
    }

    /// Total live objects.
    pub fn live_objects(&self) -> usize {
        self.objects.iter().filter(|o| o.alive).count()
    }
}
