//! # ACPI Parser
//!
//! Parses the ACPI tables provided by firmware to discover:
//! - CPU cores and their APIC IDs (MADT table)
//! - PCI configuration space (MCFG table)  
//! - Power management capabilities (FADT table)
//! - NUMA domains (SRAT table)
//!
//! Essential for SMP boot and hardware enumeration.

/// RSDP (Root System Description Pointer) — the starting point
/// for finding all ACPI tables. Located in firmware memory.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Rsdp {
    pub signature: [u8; 8],    // "RSD PTR "
    pub checksum: u8,
    pub oem_id: [u8; 6],
    pub revision: u8,
    pub rsdt_address: u32,     // ACPI 1.0 (32-bit)
    // ACPI 2.0+ extended fields
    pub length: u32,
    pub xsdt_address: u64,     // ACPI 2.0 (64-bit)
    pub ext_checksum: u8,
    pub _reserved: [u8; 3],
}

/// Standard ACPI table header (present in all tables).
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct AcpiTableHeader {
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

/// MADT (Multiple APIC Description Table) — lists all CPU cores.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct Madt {
    pub header: AcpiTableHeader,
    pub local_apic_addr: u32,
    pub flags: u32,
    // Followed by variable-length MADT entries
}

/// MADT entry types
#[derive(Debug, Clone, Copy)]
pub enum MadtEntry {
    /// Processor Local APIC (type 0)
    LocalApic {
        processor_id: u8,
        apic_id: u8,
        flags: u32,
    },
    /// IO APIC (type 1)
    IoApic {
        id: u8,
        address: u32,
        global_irq_base: u32,
    },
    /// Interrupt Source Override (type 2)
    IntSourceOverride {
        bus: u8,
        source: u8,
        global_irq: u32,
        flags: u16,
    },
    /// Local APIC NMI (type 4)
    LocalApicNmi {
        processor_id: u8,
        flags: u16,
        lint: u8,
    },
}

/// Parsed ACPI information.
#[derive(Debug)]
pub struct AcpiInfo {
    /// CPU core count
    pub cpu_count: usize,
    /// APIC IDs for each core
    pub apic_ids: [u8; 256],
    /// IO APIC address
    pub ioapic_addr: u32,
    /// Local APIC address
    pub lapic_addr: u32,
    /// IRQ source overrides
    pub irq_overrides: [(u8, u32); 16],
    /// Number of IRQ overrides
    pub num_overrides: usize,
}

impl AcpiInfo {
    pub const fn new() -> Self {
        AcpiInfo {
            cpu_count: 0,
            apic_ids: [0; 256],
            ioapic_addr: 0,
            lapic_addr: 0xFEE0_0000,
            irq_overrides: [(0, 0); 16],
            num_overrides: 0,
        }
    }
}

/// Parse the RSDP to find ACPI table addresses.
///
/// # Safety
/// Requires firmware memory to be identity-mapped.
pub unsafe fn parse_rsdp(rsdp_addr: u64) -> Option<u64> {
    let rsdp = &*(rsdp_addr as *const Rsdp);

    // Verify signature "RSD PTR "
    if &rsdp.signature != b"RSD PTR " {
        return None;
    }

    // Use XSDT (64-bit) if available (ACPI 2.0+)
    if rsdp.revision >= 2 && rsdp.xsdt_address != 0 {
        Some(rsdp.xsdt_address)
    } else {
        Some(rsdp.rsdt_address as u64)
    }
}

/// Parse the MADT to discover CPU cores and IO APICs.
///
/// # Safety
/// The madt_addr must point to a valid MADT table in memory.
pub unsafe fn parse_madt(madt_addr: u64) -> AcpiInfo {
    let madt = &*(madt_addr as *const Madt);
    let mut info = AcpiInfo::new();
    info.lapic_addr = madt.local_apic_addr;

    let total_len = madt.header.length as usize;
    let entries_start = madt_addr as usize + core::mem::size_of::<Madt>();
    let entries_end = madt_addr as usize + total_len;

    let mut offset = entries_start;
    while offset + 2 <= entries_end {
        let entry_type = *(offset as *const u8);
        let entry_len = *((offset + 1) as *const u8) as usize;

        if entry_len < 2 || offset + entry_len > entries_end {
            break;
        }

        match entry_type {
            0 => {
                // Processor Local APIC
                let processor_id = *((offset + 2) as *const u8);
                let apic_id = *((offset + 3) as *const u8);
                let flags = *((offset + 4) as *const u32);

                // Only count enabled processors
                if flags & 1 != 0 {
                    if info.cpu_count < 256 {
                        info.apic_ids[info.cpu_count] = apic_id;
                        info.cpu_count += 1;
                    }
                }
                let _ = processor_id;
            }
            1 => {
                // IO APIC
                let _id = *((offset + 2) as *const u8);
                let address = *((offset + 4) as *const u32);
                info.ioapic_addr = address;
            }
            2 => {
                // Interrupt Source Override
                let source = *((offset + 3) as *const u8);
                let global_irq = *((offset + 4) as *const u32);
                if info.num_overrides < 16 {
                    info.irq_overrides[info.num_overrides] = (source, global_irq);
                    info.num_overrides += 1;
                }
            }
            _ => {} // Skip unknown entry types
        }

        offset += entry_len;
    }

    info
}

/// Search for an ACPI table by its 4-byte signature.
///
/// Scans the XSDT/RSDT entries to find a specific table
/// (e.g., "APIC" for MADT, "FACP" for FADT, "MCFG" for PCI config).
pub unsafe fn find_table(xsdt_addr: u64, signature: &[u8; 4]) -> Option<u64> {
    let header = &*(xsdt_addr as *const AcpiTableHeader);
    let total_len = header.length as usize;
    let entries_start = xsdt_addr as usize + core::mem::size_of::<AcpiTableHeader>();
    let entry_size = 8; // 64-bit pointers in XSDT
    let num_entries = (total_len - core::mem::size_of::<AcpiTableHeader>()) / entry_size;

    for i in 0..num_entries {
        let entry_addr = *((entries_start + i * entry_size) as *const u64);
        let table_header = &*(entry_addr as *const AcpiTableHeader);

        if &table_header.signature == signature {
            return Some(entry_addr);
        }
    }

    None
}
