#![no_std]

use crate::efi_stub::{EfiStub, BootInfo};
use crate::qaudit_kernel::QAuditKernel;

/// Bridge for Phase 294: EFI Boot Services Exit Audit Bridge
/// Wraps `exit_boot_services`. Enforces Law 2 (System Integrity) by firing a mandatory audit event.
pub struct EfiBootServicesAuditBridge<'a> {
    target: &'a mut EfiStub,
}

impl<'a> EfiBootServicesAuditBridge<'a> {
    pub fn new(target: &'a mut EfiStub) -> Self {
        Self { target }
    }

    pub fn exit_boot_services(
        &mut self,
        audit: &mut QAuditKernel,
        tick: u64,
    ) -> &BootInfo {
        audit.log_law_violation(
            2, // Law 2: System Integrity / Unauthorized Execution
            0, // Kernel context / Silo 0
            tick,
        );
        crate::serial_println!("[EFI STUB] Boot services exited. Audit logged. Generating final BootInfo map.");

        self.target.exit_boot_services()
    }
}
