//! # Qernel ACPI Table Parser
//!
//! Parses ACPI tables (RSDP, RSDT/XSDT, MADT, FADT, HPET)
//! for hardware discovery and power management configuration.

extern crate alloc;

/// RSDP (Root System Description Pointer) — the entry point for ACPI.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct Rsdp {
    pub signature: [u8; 8],    // "RSD PTR "
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,          // 0 = ACPI 1.0, 2 = ACPI 2.0+
    pub rsdt_address: u32,
}

/// RSDP v2 extension (ACPI 2.0+).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct RsdpExtended {
    pub base: Rsdp,
    pub length: u32,
    pub xsdt_address: u64,
    pub ext_checksum: u8,
    pub reserved: [u8; 3],
}

/// Standard ACPI table header (all tables start with this).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct AcpiHeader {
    pub signature: [u8; 4],
    pub length: u32,
    pub revision: u8,
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub oem_table_id: [u8; 8],
    pub oem_revision: u32,
    pub creator_id: u32,
    pub creator_revision: u32,
}

impl AcpiHeader {
    /// Verify the table checksum.
    pub fn verify_checksum(&self, table_ptr: *const u8) -> bool {
        let length = self.length as usize;
        let mut sum: u8 = 0;
        for i in 0..length {
            sum = sum.wrapping_add(unsafe { *table_ptr.add(i) });
        }
        sum == 0
    }

    /// Get the 4-char signature as a string.
    pub fn signature_str(&self) -> &str {
        core::str::from_utf8(&self.signature).unwrap_or("????")
    }
}

/// MADT (Multiple APIC Description Table) entry types.
#[derive(Debug, Clone)]
pub enum MadtEntry {
    /// Type 0: Processor Local APIC
    LocalApic {
        processor_id: u8,
        apic_id: u8,
        flags: u32, // bit 0 = enabled, bit 1 = online capable
    },
    /// Type 1: I/O APIC
    IoApic {
        id: u8,
        address: u32,
        gsi_base: u32,
    },
    /// Type 2: Interrupt Source Override
    IntSourceOverride {
        bus: u8,
        source: u8,
        gsi: u32,
        flags: u16,
    },
    /// Type 4: Non-Maskable Interrupt
    Nmi {
        processor_id: u8,
        flags: u16,
        lint: u8,
    },
    /// Type 5: Local APIC Address Override
    LocalApicOverride {
        address: u64,
    },
}

/// Parsed MADT table.
#[derive(Debug, Clone)]
pub struct Madt {
    pub local_apic_address: u32,
    pub flags: u32,
    pub entries: alloc::vec::Vec<MadtEntry>,
}

/// FADT power management profile.
#[derive(Debug, Clone, Copy)]
pub enum PowerProfile {
    Unspecified,
    Desktop,
    Mobile,
    Workstation,
    EnterpriseServer,
    SohoServer,
    AppliancePc,
    PerformanceServer,
    Tablet,
}

/// Parsed FADT (Fixed ACPI Description Table).
#[derive(Debug, Clone)]
pub struct Fadt {
    /// FACS physical address
    pub facs_address: u64,
    /// DSDT physical address
    pub dsdt_address: u64,
    /// Power management profile
    pub power_profile: PowerProfile,
    /// SCI interrupt number
    pub sci_interrupt: u16,
    /// SMI command port
    pub smi_cmd_port: u32,
    /// PM1a event block
    pub pm1a_evt_blk: u32,
    /// PM1a control block
    pub pm1a_cnt_blk: u32,
    /// PM timer block
    pub pm_timer_blk: u32,
    /// PM timer is 32-bit? (else 24-bit)
    pub pm_timer_32bit: bool,
    /// Century CMOS register
    pub century_register: u8,
    /// Boot architecture flags
    pub boot_flags: u16,
    /// Feature flags
    pub flags: u32,
}

/// Parsed HPET (High Precision Event Timer) table.
#[derive(Debug, Clone)]
pub struct Hpet {
    /// Hardware revision ID
    pub hw_rev_id: u8,
    /// Number of comparators
    pub comparator_count: u8,
    /// Counter size (true = 64-bit, false = 32-bit)
    pub counter_64bit: bool,
    /// Legacy replacement capable
    pub legacy_capable: bool,
    /// PCI vendor ID
    pub vendor_id: u16,
    /// Base address
    pub base_address: u64,
    /// HPET number
    pub hpet_number: u8,
    /// Minimum clock tick (fs)
    pub min_tick: u16,
}

/// The ACPI table parser.
pub struct AcpiParser {
    /// All discovered tables (signature → physical address)
    pub tables: alloc::collections::BTreeMap<[u8; 4], u64>,
    /// Parsed MADT
    pub madt: Option<Madt>,
    /// Parsed FADT
    pub fadt: Option<Fadt>,
    /// Parsed HPET
    pub hpet: Option<Hpet>,
    /// RSDP revision
    pub revision: u8,
    /// Number of CPU cores found
    pub cpu_count: u8,
    /// IOAPIC count
    pub ioapic_count: u8,
}

impl AcpiParser {
    pub fn new() -> Self {
        AcpiParser {
            tables: alloc::collections::BTreeMap::new(),
            madt: None,
            fadt: None,
            hpet: None,
            revision: 0,
            cpu_count: 0,
            ioapic_count: 0,
        }
    }

    /// Parse the RSDP and discover all ACPI tables.
    ///
    /// # Safety
    /// `rsdp_addr` must point to a valid RSDP structure.
    pub unsafe fn parse_rsdp(&mut self, rsdp_addr: u64) {
        let rsdp = &*(rsdp_addr as *const Rsdp);

        // Verify signature
        if &rsdp.signature != b"RSD PTR " {
            crate::serial_println!("ACPI: Invalid RSDP signature");
            return;
        }

        // Verify checksum
        let mut sum: u8 = 0;
        for i in 0..20 {
            sum = sum.wrapping_add(*((rsdp_addr as *const u8).add(i)));
        }
        if sum != 0 {
            crate::serial_println!("ACPI: RSDP checksum failed");
            return;
        }

        self.revision = rsdp.revision;

        if rsdp.revision >= 2 {
            // ACPI 2.0+ — use XSDT (64-bit pointers)
            let ext = &*(rsdp_addr as *const RsdpExtended);
            self.parse_xsdt(ext.xsdt_address);
        } else {
            // ACPI 1.0 — use RSDT (32-bit pointers)
            self.parse_rsdt(rsdp.rsdt_address as u64);
        }
    }

    /// Parse the RSDT (32-bit table pointers).
    unsafe fn parse_rsdt(&mut self, rsdt_addr: u64) {
        let header = &*(rsdt_addr as *const AcpiHeader);
        let entry_count = (header.length as usize - core::mem::size_of::<AcpiHeader>()) / 4;
        let entries = (rsdt_addr as usize + core::mem::size_of::<AcpiHeader>()) as *const u32;

        for i in 0..entry_count {
            let table_addr = *entries.add(i) as u64;
            let table_header = &*(table_addr as *const AcpiHeader);
            self.tables.insert(table_header.signature, table_addr);
        }

        self.parse_discovered_tables();
    }

    /// Parse the XSDT (64-bit table pointers).
    unsafe fn parse_xsdt(&mut self, xsdt_addr: u64) {
        let header = &*(xsdt_addr as *const AcpiHeader);
        let entry_count = (header.length as usize - core::mem::size_of::<AcpiHeader>()) / 8;
        let entries = (xsdt_addr as usize + core::mem::size_of::<AcpiHeader>()) as *const u64;

        for i in 0..entry_count {
            let table_addr = *entries.add(i);
            let table_header = &*(table_addr as *const AcpiHeader);
            self.tables.insert(table_header.signature, table_addr);
        }

        self.parse_discovered_tables();
    }

    /// Parse the specific tables we care about.
    unsafe fn parse_discovered_tables(&mut self) {
        if let Some(&madt_addr) = self.tables.get(b"APIC") {
            self.parse_madt(madt_addr);
        }
        // FADT and HPET would be parsed similarly
    }

    /// Parse the MADT (APIC table).
    unsafe fn parse_madt(&mut self, madt_addr: u64) {
        let header = &*(madt_addr as *const AcpiHeader);
        let base = madt_addr as usize;

        // Read local APIC address and flags (offsets 36 and 40)
        let local_apic_addr = *(base as *const u8).add(36) as u32
            | (*(base as *const u8).add(37) as u32) << 8
            | (*(base as *const u8).add(38) as u32) << 16
            | (*(base as *const u8).add(39) as u32) << 24;

        let flags = *(base as *const u8).add(40) as u32
            | (*(base as *const u8).add(41) as u32) << 8
            | (*(base as *const u8).add(42) as u32) << 16
            | (*(base as *const u8).add(43) as u32) << 24;

        let mut entries = alloc::vec::Vec::new();
        let mut offset = 44; // Start of MADT entries

        while offset < header.length as usize {
            let entry_type = *(base as *const u8).add(offset);
            let entry_len = *(base as *const u8).add(offset + 1) as usize;

            if entry_len == 0 { break; } // Safety: prevent infinite loop

            match entry_type {
                0 => {
                    // Local APIC
                    let proc_id = *(base as *const u8).add(offset + 2);
                    let apic_id = *(base as *const u8).add(offset + 3);
                    let eflags = *((base + offset + 4) as *const u32);
                    if eflags & 1 != 0 { self.cpu_count += 1; }
                    entries.push(MadtEntry::LocalApic {
                        processor_id: proc_id,
                        apic_id,
                        flags: eflags,
                    });
                }
                1 => {
                    // I/O APIC
                    let id = *(base as *const u8).add(offset + 2);
                    let addr = *((base + offset + 4) as *const u32);
                    let gsi = *((base + offset + 8) as *const u32);
                    self.ioapic_count += 1;
                    entries.push(MadtEntry::IoApic { id, address: addr, gsi_base: gsi });
                }
                2 => {
                    // Int Source Override
                    let bus = *(base as *const u8).add(offset + 2);
                    let source = *(base as *const u8).add(offset + 3);
                    let gsi = *((base + offset + 4) as *const u32);
                    let oflags = *((base + offset + 8) as *const u16);
                    entries.push(MadtEntry::IntSourceOverride { bus, source, gsi, flags: oflags });
                }
                4 => {
                    // NMI
                    let proc_id = *(base as *const u8).add(offset + 2);
                    let nflags = *((base + offset + 3) as *const u16);
                    let lint = *(base as *const u8).add(offset + 5);
                    entries.push(MadtEntry::Nmi { processor_id: proc_id, flags: nflags, lint });
                }
                5 => {
                    // Local APIC Address Override
                    let addr = *((base + offset + 4) as *const u64);
                    entries.push(MadtEntry::LocalApicOverride { address: addr });
                }
                _ => {} // Skip unknown entries
            }

            offset += entry_len;
        }

        self.madt = Some(Madt {
            local_apic_address: local_apic_addr,
            flags,
            entries,
        });
    }
}
