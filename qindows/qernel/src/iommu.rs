//! # Qernel IOMMU / VT-d Driver
//!
//! Intel VT-d (Virtualization Technology for Directed I/O) driver.
//! Provides DMA remapping so that devices can only access memory
//! explicitly granted to them — critical for Silo isolation.
//!
//! Without IOMMU, a rogue device driver could DMA into any physical
//! address, bypassing all memory protection. VT-d fixes this by
//! interposing page tables on every DMA transaction.

extern crate alloc;

use alloc::vec::Vec;

/// IOMMU capability flags.
pub mod caps {
    pub const SAGAW_39BIT: u8  = 0x02; // 3-level page tables
    pub const SAGAW_48BIT: u8  = 0x04; // 4-level page tables
    pub const CACHING_MODE: u8 = 0x01;
    pub const PASS_THROUGH: u8 = 0x02;
}

/// IOMMU register offsets (relative to MMIO base).
pub mod regs {
    pub const VER: u64         = 0x000;  // Version
    pub const CAP: u64         = 0x008;  // Capability
    pub const ECAP: u64        = 0x010;  // Extended capability
    pub const GCMD: u64        = 0x018;  // Global command
    pub const GSTS: u64        = 0x01C;  // Global status
    pub const RTADDR: u64      = 0x020;  // Root table address
    pub const CCMD: u64        = 0x028;  // Context command
    pub const FSTS: u64        = 0x034;  // Fault status
    pub const FECTL: u64       = 0x038;  // Fault event control
    pub const FEDATA: u64      = 0x03C;  // Fault event data
    pub const FEADDR: u64      = 0x040;  // Fault event address
    pub const IQH: u64         = 0x080;  // Invalidation queue head
    pub const IQT: u64         = 0x088;  // Invalidation queue tail
    pub const IQA: u64         = 0x090;  // Invalidation queue address
}

/// GCMD register bits.
pub mod gcmd {
    pub const TRANSLATION_ENABLE: u32    = 1 << 31;
    pub const SET_ROOT_TABLE: u32        = 1 << 30;
    pub const INTERRUPT_REMAP_ENABLE: u32 = 1 << 25;
    pub const QUEUED_INVAL_ENABLE: u32   = 1 << 26;
}

/// GSTS register bits.
pub mod gsts {
    pub const TRANSLATION_ENABLED: u32   = 1 << 31;
    pub const ROOT_TABLE_SET: u32        = 1 << 30;
}

/// A root table entry (16 bytes, one per PCI bus).
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct RootEntry {
    /// Low 64 bits: present bit + context table pointer
    pub lo: u64,
    /// High 64 bits: reserved
    pub hi: u64,
}

impl RootEntry {
    pub const EMPTY: RootEntry = RootEntry { lo: 0, hi: 0 };

    pub fn is_present(&self) -> bool {
        self.lo & 1 != 0
    }

    pub fn context_table_addr(&self) -> u64 {
        self.lo & 0xFFFF_FFFF_FFFF_F000
    }

    pub fn set_context_table(&mut self, phys: u64) {
        self.lo = (phys & 0xFFFF_FFFF_FFFF_F000) | 1; // Present
    }
}

/// A context table entry (16 bytes, one per device:function on a bus).
#[repr(C, align(16))]
#[derive(Debug, Clone, Copy)]
pub struct ContextEntry {
    /// Low: present + fault processing disable + translation type + address width + second-level PT
    pub lo: u64,
    /// High: domain ID + reserved
    pub hi: u64,
}

impl ContextEntry {
    pub const EMPTY: ContextEntry = ContextEntry { lo: 0, hi: 0 };

    pub fn is_present(&self) -> bool {
        self.lo & 1 != 0
    }

    /// Set to point at a second-level page table for DMA remapping.
    pub fn set_translation(&mut self, page_table_phys: u64, domain_id: u16, address_width: u8) {
        // Bits 0: present
        // Bits 2-3: translation type (00 = second-level only)
        // Bits 4-6: address width (010 = 39-bit, 011 = 48-bit)
        let aw = match address_width {
            39 => 0b010u64,
            48 => 0b011u64,
            _  => 0b010u64,
        };
        self.lo = (page_table_phys & 0xFFFF_FFFF_FFFF_F000) | (aw << 2) | 1;
        self.hi = domain_id as u64;
    }

    pub fn domain_id(&self) -> u16 {
        (self.hi & 0xFFFF) as u16
    }
}

/// Second-level page table entry (used by IOMMU, same format as CPU).
#[derive(Debug, Clone, Copy)]
pub struct SlPte(pub u64);

impl SlPte {
    pub const EMPTY: SlPte = SlPte(0);

    pub fn is_present(&self) -> bool { self.0 & 1 != 0 }
    pub fn is_writable(&self) -> bool { self.0 & 2 != 0 }
    pub fn is_readable(&self) -> bool { self.0 & 1 != 0 } // Read = Present in VT-d

    pub fn phys_addr(&self) -> u64 {
        self.0 & 0x000F_FFFF_FFFF_F000
    }

    /// Create a leaf mapping (read+write).
    pub fn map_rw(phys: u64) -> Self {
        SlPte((phys & 0x000F_FFFF_FFFF_F000) | 0x03) // read + write
    }

    /// Create a leaf mapping (read-only).
    pub fn map_ro(phys: u64) -> Self {
        SlPte((phys & 0x000F_FFFF_FFFF_F000) | 0x01) // read only
    }

    /// Create a non-leaf entry pointing to next page table level.
    pub fn table(phys: u64) -> Self {
        SlPte((phys & 0x000F_FFFF_FFFF_F000) | 0x03)
    }
}

/// A DMA domain — an isolated address space for one or more devices.
#[derive(Debug, Clone)]
pub struct DmaDomain {
    /// Domain ID (unique)
    pub domain_id: u16,
    /// Silo ID this domain belongs to
    pub silo_id: u64,
    /// Second-level page table root physical address
    pub page_table_root: u64,
    /// Devices (BDF) assigned to this domain
    pub devices: Vec<(u8, u8, u8)>, // (bus, device, function)
    /// Total mapped bytes
    pub mapped_bytes: u64,
    /// Fault count
    pub faults: u64,
}

/// IOMMU fault record.
#[derive(Debug, Clone)]
pub struct IommuFault {
    /// Faulting device (bus:device.function)
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    /// Faulting DMA address
    pub address: u64,
    /// Fault reason
    pub reason: FaultReason,
    /// Domain ID
    pub domain_id: u16,
    /// Was this a write? (false = read)
    pub is_write: bool,
}

/// IOMMU fault reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultReason {
    /// Page not present
    NotPresent,
    /// Write to read-only page
    WriteProtect,
    /// Address width exceeded
    AddressWidth,
    /// Context entry not present
    NoContext,
    /// Root entry not present
    NoRoot,
    /// Reserved field set
    Reserved,
    /// Other hardware fault
    Other(u8),
}

impl FaultReason {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x01 => FaultReason::NoRoot,
            0x02 => FaultReason::NoContext,
            0x05 => FaultReason::NotPresent,
            0x06 => FaultReason::WriteProtect,
            0x08 => FaultReason::AddressWidth,
            0x0B => FaultReason::Reserved,
            x    => FaultReason::Other(x),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FaultReason::NotPresent   => "page not present",
            FaultReason::WriteProtect => "write to read-only",
            FaultReason::AddressWidth => "address width exceeded",
            FaultReason::NoContext    => "context entry missing",
            FaultReason::NoRoot       => "root entry missing",
            FaultReason::Reserved     => "reserved field set",
            FaultReason::Other(_)     => "unknown",
        }
    }
}

/// IOMMU statistics.
#[derive(Debug, Clone, Default)]
pub struct IommuStats {
    pub domains_created: u64,
    pub mappings_created: u64,
    pub mappings_destroyed: u64,
    pub iotlb_flushes: u64,
    pub faults_total: u64,
    pub faults_recovered: u64,
}

/// The IOMMU Controller.
pub struct IommuController {
    /// MMIO base address (from ACPI DMAR table)
    pub mmio_base: u64,
    /// Root table physical address (4 KiB aligned, 256 entries)
    pub root_table_phys: u64,
    /// All DMA domains
    pub domains: Vec<DmaDomain>,
    /// Fault log
    pub fault_log: Vec<IommuFault>,
    /// Next domain ID
    next_domain_id: u16,
    /// Is IOMMU translation enabled?
    pub enabled: bool,
    /// Address width (39 or 48)
    pub address_width: u8,
    /// Stats
    pub stats: IommuStats,
}

impl IommuController {
    /// Initialize the IOMMU.
    ///
    /// # Safety
    /// `mmio_base` must point to valid VT-d register space.
    /// `root_table_phys` must be a 4 KiB-aligned zeroed page.
    pub unsafe fn init(mmio_base: u64, root_table_phys: u64) -> Self {
        let mut ctrl = IommuController {
            mmio_base,
            root_table_phys,
            domains: Vec::new(),
            fault_log: Vec::new(),
            next_domain_id: 1,
            enabled: false,
            address_width: 48,
            stats: IommuStats::default(),
        };

        // Read version
        let ver = ctrl.read_reg32(regs::VER);
        let major = (ver >> 4) & 0xF;
        let minor = ver & 0xF;

        // Read capabilities
        let cap = ctrl.read_reg64(regs::CAP);
        let sagaw = ((cap >> 8) & 0x1F) as u8;
        ctrl.address_width = if sagaw & caps::SAGAW_48BIT != 0 { 48 } else { 39 };

        // Set root table address
        ctrl.write_reg64(regs::RTADDR, root_table_phys);
        ctrl.write_reg32(regs::GCMD, gcmd::SET_ROOT_TABLE);

        // Wait for root table to be set
        while ctrl.read_reg32(regs::GSTS) & gsts::ROOT_TABLE_SET == 0 {
            core::hint::spin_loop();
        }

        // Enable translation
        ctrl.write_reg32(regs::GCMD, gcmd::TRANSLATION_ENABLE);
        while ctrl.read_reg32(regs::GSTS) & gsts::TRANSLATION_ENABLED == 0 {
            core::hint::spin_loop();
        }

        ctrl.enabled = true;
        crate::serial_println!(
            "[OK] IOMMU VT-d v{}.{} enabled ({}-bit, root=0x{:X})",
            major, minor, ctrl.address_width, root_table_phys
        );

        ctrl
    }

    /// Create a new DMA domain for a Silo.
    pub fn create_domain(&mut self, silo_id: u64, page_table_root: u64) -> u16 {
        let domain_id = self.next_domain_id;
        self.next_domain_id += 1;

        self.domains.push(DmaDomain {
            domain_id,
            silo_id,
            page_table_root,
            devices: Vec::new(),
            mapped_bytes: 0,
            faults: 0,
        });

        self.stats.domains_created += 1;
        domain_id
    }

    /// Assign a PCI device to a DMA domain.
    ///
    /// This programs the root/context tables so the device's
    /// DMA transactions go through the domain's page table.
    pub unsafe fn assign_device(
        &mut self,
        domain_id: u16,
        bus: u8,
        device: u8,
        function: u8,
    ) -> Result<(), &'static str> {
        let domain = self.domains.iter_mut()
            .find(|d| d.domain_id == domain_id)
            .ok_or("Domain not found")?;

        // Get root entry for this bus
        let root_ptr = self.root_table_phys as *mut RootEntry;
        let root_entry = &mut *root_ptr.add(bus as usize);

        // Ensure context table exists
        if !root_entry.is_present() {
            // Allocate a context table (256 entries, 4 KiB)
            let ctx_frame = crate::memory::page_alloc::alloc_frame()
                .ok_or("OOM: cannot allocate context table")?;
            core::ptr::write_bytes(ctx_frame.0 as *mut u8, 0, 4096);
            root_entry.set_context_table(ctx_frame.0);
        }

        let ctx_table = root_entry.context_table_addr() as *mut ContextEntry;
        let devfn = ((device as usize) << 3) | (function as usize);
        let ctx_entry = &mut *ctx_table.add(devfn);

        ctx_entry.set_translation(domain.page_table_root, domain_id, self.address_width);
        domain.devices.push((bus, device, function));

        // Flush context cache
        self.flush_context_cache();

        Ok(())
    }

    /// Map a physical range into a DMA domain's address space.
    ///
    /// The device will see `iova` → `phys`, allowing it to DMA
    /// only to explicitly granted memory regions.
    pub unsafe fn map_iova(
        &mut self,
        domain_id: u16,
        iova: u64,
        phys: u64,
        size: u64,
        writable: bool,
    ) -> Result<(), &'static str> {
        let domain = self.domains.iter_mut()
            .find(|d| d.domain_id == domain_id)
            .ok_or("Domain not found")?;

        let pages = (size + 0xFFF) / 0x1000;
        for i in 0..pages {
            let va = iova + i * 0x1000;
            let pa = phys + i * 0x1000;

            // Walk the 4-level second-level page table
            let pml4 = domain.page_table_root as *mut SlPte;
            let pml4_idx = ((va >> 39) & 0x1FF) as usize;
            let pdpt = self.ensure_sl_table(&mut *pml4.add(pml4_idx));
            let pdpt_idx = ((va >> 30) & 0x1FF) as usize;
            let pd = self.ensure_sl_table(&mut *pdpt.add(pdpt_idx));
            let pd_idx = ((va >> 21) & 0x1FF) as usize;
            let pt = self.ensure_sl_table(&mut *pd.add(pd_idx));
            let pt_idx = ((va >> 12) & 0x1FF) as usize;

            *pt.add(pt_idx) = if writable { SlPte::map_rw(pa) } else { SlPte::map_ro(pa) };
        }

        domain.mapped_bytes += pages * 0x1000;
        self.stats.mappings_created += pages;

        // Flush IOTLB
        self.flush_iotlb();

        Ok(())
    }

    /// Unmap an IOVA range from a domain.
    pub unsafe fn unmap_iova(
        &mut self,
        domain_id: u16,
        iova: u64,
        size: u64,
    ) -> Result<(), &'static str> {
        let domain = self.domains.iter_mut()
            .find(|d| d.domain_id == domain_id)
            .ok_or("Domain not found")?;

        let pages = (size + 0xFFF) / 0x1000;
        for i in 0..pages {
            let va = iova + i * 0x1000;
            let pml4 = domain.page_table_root as *mut SlPte;
            let pml4_idx = ((va >> 39) & 0x1FF) as usize;
            if !(*pml4.add(pml4_idx)).is_present() { continue; }

            let pdpt = (*pml4.add(pml4_idx)).phys_addr() as *mut SlPte;
            let pdpt_idx = ((va >> 30) & 0x1FF) as usize;
            if !(*pdpt.add(pdpt_idx)).is_present() { continue; }

            let pd = (*pdpt.add(pdpt_idx)).phys_addr() as *mut SlPte;
            let pd_idx = ((va >> 21) & 0x1FF) as usize;
            if !(*pd.add(pd_idx)).is_present() { continue; }

            let pt = (*pd.add(pd_idx)).phys_addr() as *mut SlPte;
            let pt_idx = ((va >> 12) & 0x1FF) as usize;
            *pt.add(pt_idx) = SlPte::EMPTY;
        }

        domain.mapped_bytes = domain.mapped_bytes.saturating_sub(pages * 0x1000);
        self.stats.mappings_destroyed += pages;
        self.flush_iotlb();

        Ok(())
    }

    /// Handle IOMMU fault interrupt — read and log fault records.
    pub unsafe fn handle_fault(&mut self) {
        let fsts = self.read_reg32(regs::FSTS);
        if fsts & 1 == 0 { return; } // No primary fault

        // Read the fault recording register (simplified — real hardware
        // has a fault recording register bank at offset from ECAP)
        let fault = IommuFault {
            bus: 0,
            device: 0,
            function: 0,
            address: 0,
            reason: FaultReason::NotPresent,
            domain_id: 0,
            is_write: false,
        };

        self.fault_log.push(fault);
        self.stats.faults_total += 1;

        // Clear fault status
        self.write_reg32(regs::FSTS, 1);
    }

    /// Flush the context cache (after modifying root/context tables).
    unsafe fn flush_context_cache(&mut self) {
        // Write context command: global invalidate
        self.write_reg64(regs::CCMD, 1u64 << 63 | 0x01u64 << 61);
        // Wait for completion
        while self.read_reg64(regs::CCMD) & (1u64 << 63) != 0 {
            core::hint::spin_loop();
        }
    }

    /// Flush the IOTLB (after modifying second-level page tables).
    unsafe fn flush_iotlb(&mut self) {
        // Read ECAP to find IOTLB register offset
        let ecap = self.read_reg64(regs::ECAP);
        let iro = ((ecap >> 8) & 0x3FF) as u64 * 16;

        // Write IOTLB invalidation: global
        let iotlb_reg = self.mmio_base + iro + 8;
        core::ptr::write_volatile(iotlb_reg as *mut u64, 1u64 << 63 | 0x01u64 << 60);
        // Wait for completion
        while core::ptr::read_volatile(iotlb_reg as *const u64) & (1u64 << 63) != 0 {
            core::hint::spin_loop();
        }

        self.stats.iotlb_flushes += 1;
    }

    /// Ensure a second-level page table exists at a PTE.
    unsafe fn ensure_sl_table(&self, entry: &mut SlPte) -> *mut SlPte {
        if entry.is_present() {
            entry.phys_addr() as *mut SlPte
        } else {
            let frame = crate::memory::page_alloc::alloc_frame()
                .expect("IOMMU: OOM for page table");
            core::ptr::write_bytes(frame.0 as *mut u8, 0, 4096);
            *entry = SlPte::table(frame.0);
            frame.0 as *mut SlPte
        }
    }

    /// Register MMIO helpers.
    unsafe fn read_reg32(&self, offset: u64) -> u32 {
        core::ptr::read_volatile((self.mmio_base + offset) as *const u32)
    }

    unsafe fn write_reg32(&self, offset: u64, val: u32) {
        core::ptr::write_volatile((self.mmio_base + offset) as *mut u32, val);
    }

    unsafe fn read_reg64(&self, offset: u64) -> u64 {
        core::ptr::read_volatile((self.mmio_base + offset) as *const u64)
    }

    unsafe fn write_reg64(&self, offset: u64, val: u64) {
        core::ptr::write_volatile((self.mmio_base + offset) as *mut u64, val);
    }
}
