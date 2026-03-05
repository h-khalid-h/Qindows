//! # Qernel SMBIOS Parser
//!
//! Parses SMBIOS/DMI tables for hardware inventory — BIOS version,
//! CPU info, memory DIMMs, system manufacturer, serial numbers,
//! and chassis type. Used by the Settings crate and diagnostics.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// SMBIOS entry point signature: "_SM3_" (SMBIOS 3.0+).
pub const SMBIOS3_ANCHOR: [u8; 5] = *b"_SM3_";
/// Legacy SMBIOS entry point signature: "_SM_".
pub const SMBIOS2_ANCHOR: [u8; 4] = *b"_SM_";

/// SMBIOS structure types we care about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmbiosType {
    BiosInfo       = 0,
    SystemInfo     = 1,
    BaseboardInfo  = 2,
    ChassisInfo    = 3,
    ProcessorInfo  = 4,
    CacheInfo      = 7,
    MemoryDevice   = 17,
    BootInfo       = 32,
}

impl SmbiosType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0  => Some(SmbiosType::BiosInfo),
            1  => Some(SmbiosType::SystemInfo),
            2  => Some(SmbiosType::BaseboardInfo),
            3  => Some(SmbiosType::ChassisInfo),
            4  => Some(SmbiosType::ProcessorInfo),
            7  => Some(SmbiosType::CacheInfo),
            17 => Some(SmbiosType::MemoryDevice),
            32 => Some(SmbiosType::BootInfo),
            _  => None,
        }
    }
}

/// SMBIOS structure header (common to all types).
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct SmbiosHeader {
    pub struct_type: u8,
    pub length: u8,
    pub handle: u16,
}

/// Parsed BIOS information (Type 0).
#[derive(Debug, Clone)]
pub struct BiosInfo {
    pub vendor: String,
    pub version: String,
    pub release_date: String,
    pub rom_size_kb: u32,
    pub major_release: u8,
    pub minor_release: u8,
}

/// Parsed system information (Type 1).
#[derive(Debug, Clone)]
pub struct SystemInfo {
    pub manufacturer: String,
    pub product_name: String,
    pub version: String,
    pub serial_number: String,
    pub uuid: [u8; 16],
    pub sku: String,
    pub family: String,
}

/// Chassis type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChassisType {
    Desktop,
    Laptop,
    Notebook,
    Tablet,
    Server,
    Tower,
    MiniTower,
    AllInOne,
    Convertible,
    Other(u8),
}

impl ChassisType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            3  => ChassisType::Desktop,
            9  => ChassisType::Laptop,
            10 => ChassisType::Notebook,
            11 => ChassisType::Tablet, // Docking station, reused
            17 => ChassisType::Server,
            6  => ChassisType::MiniTower,
            7  => ChassisType::Tower,
            13 => ChassisType::AllInOne,
            31 => ChassisType::Convertible,
            _  => ChassisType::Other(v),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ChassisType::Desktop     => "Desktop",
            ChassisType::Laptop      => "Laptop",
            ChassisType::Notebook    => "Notebook",
            ChassisType::Tablet      => "Tablet",
            ChassisType::Server      => "Server",
            ChassisType::Tower       => "Tower",
            ChassisType::MiniTower   => "Mini Tower",
            ChassisType::AllInOne    => "All-in-One",
            ChassisType::Convertible => "Convertible",
            ChassisType::Other(_)    => "Other",
        }
    }
}

/// Parsed processor information (Type 4).
#[derive(Debug, Clone)]
pub struct ProcessorInfo {
    pub socket: String,
    pub manufacturer: String,
    pub version: String,
    pub max_speed_mhz: u16,
    pub current_speed_mhz: u16,
    pub core_count: u16,
    pub thread_count: u16,
    pub voltage_mv: u16,
    pub status: ProcessorStatus,
}

/// Processor status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessorStatus {
    Unknown,
    Enabled,
    DisabledByUser,
    DisabledByBios,
    Idle,
    Other(u8),
}

impl ProcessorStatus {
    pub fn from_u8(v: u8) -> Self {
        match v & 0x07 {
            0 => ProcessorStatus::Unknown,
            1 => ProcessorStatus::Enabled,
            2 => ProcessorStatus::DisabledByUser,
            3 => ProcessorStatus::DisabledByBios,
            4 => ProcessorStatus::Idle,
            x => ProcessorStatus::Other(x),
        }
    }
}

/// Parsed memory device (Type 17).
#[derive(Debug, Clone)]
pub struct MemoryDevice {
    pub locator: String,
    pub bank: String,
    pub manufacturer: String,
    pub serial: String,
    pub part_number: String,
    pub size_mb: u32,
    pub speed_mhz: u16,
    pub form_factor: MemoryFormFactor,
    pub mem_type: MemoryType,
}

/// Memory form factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFormFactor {
    DIMM,
    SODIMM,
    RDIMM,
    LRDIMM,
    Other(u8),
}

impl MemoryFormFactor {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x09 => MemoryFormFactor::DIMM,
            0x0D => MemoryFormFactor::SODIMM,
            0x0C => MemoryFormFactor::RDIMM,
            0x0F => MemoryFormFactor::LRDIMM,
            _    => MemoryFormFactor::Other(v),
        }
    }
}

/// Memory type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryType {
    DDR3,
    DDR4,
    DDR5,
    LPDDR4,
    LPDDR5,
    Other(u8),
}

impl MemoryType {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0x18 => MemoryType::DDR3,
            0x1A => MemoryType::DDR4,
            0x22 => MemoryType::DDR5,
            0x1C => MemoryType::LPDDR4,
            0x23 => MemoryType::LPDDR5,
            _    => MemoryType::Other(v),
        }
    }
}

/// Complete SMBIOS hardware inventory.
#[derive(Debug, Clone)]
pub struct HardwareInventory {
    pub bios: Option<BiosInfo>,
    pub system: Option<SystemInfo>,
    pub chassis: Option<ChassisType>,
    pub processors: Vec<ProcessorInfo>,
    pub memory_devices: Vec<MemoryDevice>,
    pub total_memory_mb: u32,
    pub smbios_version: (u8, u8),
}

/// The SMBIOS Parser.
pub struct SmbiosParser {
    pub inventory: HardwareInventory,
    pub tables_parsed: u32,
    pub parse_errors: u32,
}

impl SmbiosParser {
    pub fn new() -> Self {
        SmbiosParser {
            inventory: HardwareInventory {
                bios: None,
                system: None,
                chassis: None,
                processors: Vec::new(),
                memory_devices: Vec::new(),
                total_memory_mb: 0,
                smbios_version: (0, 0),
            },
            tables_parsed: 0,
            parse_errors: 0,
        }
    }

    /// Parse all SMBIOS structures starting from the table base address.
    ///
    /// # Safety
    /// `table_addr` must point to valid SMBIOS table data.
    /// `length` is the total byte length of the table area.
    pub unsafe fn parse_tables(&mut self, table_addr: u64, length: usize, version: (u8, u8)) {
        self.inventory.smbios_version = version;
        let base = table_addr as *const u8;
        let mut offset = 0usize;

        while offset + core::mem::size_of::<SmbiosHeader>() < length {
            let header_ptr = base.add(offset) as *const SmbiosHeader;
            let header = core::ptr::read_unaligned(header_ptr);

            // Validate header length
            if (header.length as usize) < core::mem::size_of::<SmbiosHeader>() {
                self.parse_errors += 1;
                break;
            }

            // End-of-table marker (type 127)
            if header.struct_type == 127 { break; }

            // Read string table (null-terminated strings after the formatted area)
            let strings = self.read_string_table(base, offset, header.length as usize, length);

            // Parse based on type
            match SmbiosType::from_u8(header.struct_type) {
                Some(SmbiosType::BiosInfo) => {
                    self.parse_bios(base.add(offset), header.length as usize, &strings);
                }
                Some(SmbiosType::SystemInfo) => {
                    self.parse_system(base.add(offset), header.length as usize, &strings);
                }
                Some(SmbiosType::ChassisInfo) => {
                    if offset + 5 < length {
                        let chassis_byte = *base.add(offset + 5) & 0x7F;
                        self.inventory.chassis = Some(ChassisType::from_u8(chassis_byte));
                    }
                }
                Some(SmbiosType::ProcessorInfo) => {
                    self.parse_processor(base.add(offset), header.length as usize, &strings);
                }
                Some(SmbiosType::MemoryDevice) => {
                    self.parse_memory_device(base.add(offset), header.length as usize, &strings);
                }
                _ => {}
            }

            self.tables_parsed += 1;

            // Advance past formatted area + string table (double null-terminated)
            let mut str_offset = offset + header.length as usize;
            while str_offset + 1 < length {
                if *base.add(str_offset) == 0 && *base.add(str_offset + 1) == 0 {
                    str_offset += 2;
                    break;
                }
                str_offset += 1;
            }
            offset = str_offset;
        }

        // Compute total memory
        self.inventory.total_memory_mb = self.inventory.memory_devices
            .iter()
            .map(|d| d.size_mb)
            .sum();
    }

    /// Read the string table following a structure's formatted area.
    unsafe fn read_string_table(
        &self, base: *const u8, struct_offset: usize,
        formatted_len: usize, total_len: usize,
    ) -> Vec<String> {
        let mut strings = Vec::new();
        let mut pos = struct_offset + formatted_len;

        // String table is a sequence of null-terminated strings,
        // ended by a double-null
        while pos < total_len {
            if *base.add(pos) == 0 { break; } // Double-null end

            let start = pos;
            while pos < total_len && *base.add(pos) != 0 { pos += 1; }

            let bytes = core::slice::from_raw_parts(base.add(start), pos - start);
            let s = core::str::from_utf8(bytes)
                .unwrap_or("?");
            strings.push(String::from(s));

            if pos < total_len { pos += 1; } // Skip null terminator
        }

        strings
    }

    /// Get string by 1-based index from the string table.
    fn get_string(strings: &[String], index: u8) -> String {
        if index == 0 { return String::new(); }
        strings.get((index as usize).saturating_sub(1))
            .cloned()
            .unwrap_or_default()
    }

    unsafe fn parse_bios(&mut self, ptr: *const u8, len: usize, strings: &[String]) {
        if len < 18 { return; }

        let vendor_idx = *ptr.add(4);
        let version_idx = *ptr.add(5);
        let date_idx = *ptr.add(8);
        let rom_size_raw = *ptr.add(9);
        let rom_size_kb = ((rom_size_raw as u32) + 1) * 64;

        let (major, minor) = if len >= 24 {
            (*ptr.add(20), *ptr.add(21))
        } else {
            (0, 0)
        };

        self.inventory.bios = Some(BiosInfo {
            vendor: Self::get_string(strings, vendor_idx),
            version: Self::get_string(strings, version_idx),
            release_date: Self::get_string(strings, date_idx),
            rom_size_kb,
            major_release: major,
            minor_release: minor,
        });
    }

    unsafe fn parse_system(&mut self, ptr: *const u8, len: usize, strings: &[String]) {
        if len < 8 { return; }

        let manufacturer_idx = *ptr.add(4);
        let product_idx = *ptr.add(5);
        let version_idx = *ptr.add(6);
        let serial_idx = *ptr.add(7);

        let mut uuid = [0u8; 16];
        if len >= 24 {
            core::ptr::copy_nonoverlapping(ptr.add(8), uuid.as_mut_ptr(), 16);
        }

        let (sku_idx, family_idx) = if len >= 27 {
            (*ptr.add(25), *ptr.add(26))
        } else {
            (0, 0)
        };

        self.inventory.system = Some(SystemInfo {
            manufacturer: Self::get_string(strings, manufacturer_idx),
            product_name: Self::get_string(strings, product_idx),
            version: Self::get_string(strings, version_idx),
            serial_number: Self::get_string(strings, serial_idx),
            uuid,
            sku: Self::get_string(strings, sku_idx),
            family: Self::get_string(strings, family_idx),
        });
    }

    unsafe fn parse_processor(&mut self, ptr: *const u8, len: usize, strings: &[String]) {
        if len < 26 { return; }

        let socket_idx = *ptr.add(4);
        let manufacturer_idx = *ptr.add(7);
        let version_idx = *ptr.add(16);

        let max_speed = u16::from_le(core::ptr::read_unaligned(ptr.add(20) as *const u16));
        let current_speed = u16::from_le(core::ptr::read_unaligned(ptr.add(22) as *const u16));
        let voltage_raw = *ptr.add(17);
        let voltage_mv = if voltage_raw & 0x80 != 0 {
            ((voltage_raw & 0x7F) as u16) * 100  // In 100mV units
        } else {
            // Legacy: bits 0-3 indicate preset voltages
            match voltage_raw & 0x0F {
                0x01 => 5000,   // 5V
                0x02 => 3300,   // 3.3V
                0x04 => 2900,   // 2.9V
                _    => 0,
            }
        };

        let status = ProcessorStatus::from_u8(*ptr.add(24));

        let (core_count, thread_count) = if len >= 42 {
            // SMBIOS 3.0+: 16-bit core/thread counts
            let cores = u16::from_le(core::ptr::read_unaligned(ptr.add(38) as *const u16));
            let threads = u16::from_le(core::ptr::read_unaligned(ptr.add(40) as *const u16));
            (cores, threads)
        } else if len >= 36 {
            // SMBIOS 2.5+: 8-bit counts
            (*ptr.add(35) as u16, *ptr.add(37) as u16)
        } else {
            (1, 1)
        };

        self.inventory.processors.push(ProcessorInfo {
            socket: Self::get_string(strings, socket_idx),
            manufacturer: Self::get_string(strings, manufacturer_idx),
            version: Self::get_string(strings, version_idx),
            max_speed_mhz: max_speed,
            current_speed_mhz: current_speed,
            core_count,
            thread_count,
            voltage_mv,
            status,
        });
    }

    unsafe fn parse_memory_device(&mut self, ptr: *const u8, len: usize, strings: &[String]) {
        if len < 21 { return; }

        let locator_idx = *ptr.add(16);
        let bank_idx = *ptr.add(17);

        // Size field (offset 12, 2 bytes)
        let size_raw = u16::from_le(core::ptr::read_unaligned(ptr.add(12) as *const u16));
        let size_mb = if size_raw == 0x7FFF && len >= 32 {
            // Extended size at offset 28 (32-bit, in MiB)
            u32::from_le(core::ptr::read_unaligned(ptr.add(28) as *const u32))
        } else if size_raw & 0x8000 != 0 {
            (size_raw & 0x7FFF) as u32 / 1024 // Size in KB, convert to MB
        } else {
            size_raw as u32 // Size in MB
        };

        let form_factor = MemoryFormFactor::from_u8(*ptr.add(14));
        let mem_type = if len >= 19 {
            MemoryType::from_u8(*ptr.add(18))
        } else {
            MemoryType::Other(0)
        };

        let speed = if len >= 22 {
            u16::from_le(core::ptr::read_unaligned(ptr.add(20) as *const u16))
        } else { 0 };

        let manufacturer_idx = if len >= 24 { *ptr.add(23) } else { 0 };
        let serial_idx = if len >= 25 { *ptr.add(24) } else { 0 };
        let part_idx = if len >= 27 { *ptr.add(26) } else { 0 };

        self.inventory.memory_devices.push(MemoryDevice {
            locator: Self::get_string(strings, locator_idx),
            bank: Self::get_string(strings, bank_idx),
            manufacturer: Self::get_string(strings, manufacturer_idx),
            serial: Self::get_string(strings, serial_idx),
            part_number: Self::get_string(strings, part_idx),
            size_mb,
            speed_mhz: speed,
            form_factor,
            mem_type,
        });
    }
}
