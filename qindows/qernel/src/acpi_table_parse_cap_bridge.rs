#![no_std]

use crate::acpi::{AcpiParser, AcpiHeader};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

/// Bridge for Phase 291: ACPI Table Parse Admin Cap Bridge
/// Enforces that only Silos with `Admin:EXEC` capability can initiate ACPI table parsing and checksum verification.
pub struct AcpiTableParseCapBridge<'a> {
    target: &'a mut AcpiParser,
}

impl<'a> AcpiTableParseCapBridge<'a> {
    pub fn new(target: &'a mut AcpiParser) -> Self {
        Self { target }
    }

    pub fn verify_checksum(
        &self,
        silo_id: u64,
        table_ptr: *const u8,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            crate::serial_println!(
                "[ACPI PARSE] Silo {} ACPI checksum verification denied — Admin:EXEC required", silo_id
            );
            return false;
        }

        let header = unsafe { &*(table_ptr as *const AcpiHeader) };
        header.verify_checksum(table_ptr)
    }
}
