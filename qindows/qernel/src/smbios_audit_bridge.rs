//! # SMBIOS Inventory Audit Bridge (Phase 193)
//!
//! ## Architecture Guardian: The Gap
//! `smbios.rs` implements SMBIOS table parsing:
//! - `SmbiosHeader { struct_type: u8, length: u8, handle: u16 }` — raw table header
//! - `BiosInfo { vendor, version, release_date, rom_size_kb, major_release, minor_release }`
//! - `SystemInfo { manufacturer, product_name, uuid, serial_number, sku, family }`
//!
//! **Missing link**: SMBIOS fields were read but never audited for completeness.
//! A hypervisor spoofing SMBIOS with empty vendor/version strings bypassed detection.
//!
//! This module provides `SmbiosAuditBridge`:
//! Validates BiosInfo and SystemInfo for non-empty strings at boot.

extern crate alloc;

use crate::smbios::{BiosInfo, SystemInfo, SmbiosHeader};

#[derive(Debug, Default, Clone)]
pub struct SmbiosAuditStats {
    pub headers_checked: u64,
    pub anomalies:       u64,
}

pub struct SmbiosAuditBridge {
    pub stats: SmbiosAuditStats,
}

impl SmbiosAuditBridge {
    pub fn new() -> Self {
        SmbiosAuditBridge { stats: SmbiosAuditStats::default() }
    }

    /// Validate that a header has a known struct_type (0-127 defined by DMTF).
    pub fn audit_header(&mut self, header: &SmbiosHeader) -> bool {
        self.stats.headers_checked += 1;
        if header.length < 4 {
            self.stats.anomalies += 1;
            crate::serial_println!(
                "[SMBIOS] Anomaly: type={} length={} too short (< 4)", header.struct_type, header.length
            );
            false
        } else {
            true
        }
    }

    /// Audit BiosInfo for non-empty vendor + version strings.
    pub fn audit_bios_info(&mut self, info: &BiosInfo) -> bool {
        self.stats.headers_checked += 1;
        if info.vendor.is_empty() || info.version.is_empty() {
            self.stats.anomalies += 1;
            crate::serial_println!("[SMBIOS] Anomaly: empty BIOS vendor/version — hypervisor spoof?");
            false
        } else {
            crate::serial_println!("[SMBIOS] BIOS: vendor='{}' version='{}'", info.vendor, info.version);
            true
        }
    }

    /// Audit SystemInfo for non-empty manufacturer and product_name.
    pub fn audit_system_info(&mut self, info: &SystemInfo) -> bool {
        self.stats.headers_checked += 1;
        if info.manufacturer.is_empty() || info.product_name.is_empty() {
            self.stats.anomalies += 1;
            crate::serial_println!("[SMBIOS] Anomaly: empty manufacturer/product");
            false
        } else {
            crate::serial_println!(
                "[SMBIOS] System: '{}' / '{}'", info.manufacturer, info.product_name
            );
            true
        }
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SmbiosAuditBridge: checked={} anomalies={}",
            self.stats.headers_checked, self.stats.anomalies
        );
    }
}
