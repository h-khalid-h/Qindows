#![no_std]
extern crate alloc;

use alloc::vec::Vec;
use crate::efi_stub::{EfiStub, EfiMemoryDescriptor};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

/// Bridge for Phase 299: EFI Memory Map Parse Admin Cap Bridge
/// Gates `parse_memory_map` requiring `Admin:EXEC` capability to prevent physical memory topology spoofing.
pub struct EfiMemoryMapParseCapBridge<'a> {
    target: &'a mut EfiStub,
}

impl<'a> EfiMemoryMapParseCapBridge<'a> {
    pub fn new(target: &'a mut EfiStub) -> Self {
        Self { target }
    }

    pub fn parse_memory_map(
        &mut self,
        silo_id: u64,
        entries: Vec<EfiMemoryDescriptor>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            crate::serial_println!(
                "[EFI STUB] Silo {} memory map parsing denied — Admin:EXEC required", silo_id
            );
            return false;
        }

        self.target.parse_memory_map(entries);
        true
    }
}
