#![no_std]

use crate::cgroup::{CGroupManager, Resource, Enforcement};
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

/// Bridge for Phase 296: CGroup Limit Admin Cap Bridge
/// Enforces that only Silos with `Admin:EXEC` capability can modify hard/soft limits 
/// or enforcement policies for resource containers (cgroups).
pub struct CGroupLimitAdminCapBridge<'a> {
    target: &'a mut CGroupManager,
}

impl<'a> CGroupLimitAdminCapBridge<'a> {
    pub fn new(target: &'a mut CGroupManager) -> Self {
        Self { target }
    }

    pub fn set_limit(
        &mut self,
        silo_id: u64,
        group_id: u64,
        resource: Resource,
        hard: u64,
        soft: u64,
        enforcement: Enforcement,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            crate::serial_println!(
                "[CGROUP] Silo {} set_limit on CGroup {} denied — Admin:EXEC required", silo_id, group_id
            );
            return false;
        }

        self.target.set_limit(group_id, resource, hard, soft, enforcement);
        true
    }
}
