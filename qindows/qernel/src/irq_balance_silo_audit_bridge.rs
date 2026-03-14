//! # IRQ Balance Silo Audit Bridge (Phase 224)
//!
//! ## Architecture Guardian: The Gap
//! `irq_balance.rs` implements `IrqBalancer`:
//! - `set_silo_affinity(silo_id, cores: Vec<u32>)` — pin IRQs to cores
//! - `BalancePolicy` — RoundRobin, SiloAffinity, NumaAware, PowerSave
//!
//! **Missing link**: `set_silo_affinity()` allowed a Silo to pin ALL
//! IRQs to a single core, starving other Silos of interrupt service.
//!
//! This module provides `IrqBalanceSiloAuditBridge`:
//! Admin:EXEC cap gate on IRQ affinity changes.

extern crate alloc;
use alloc::vec::Vec;

use crate::irq_balance::IrqBalancer;
use crate::cap_tokens::{CapTokenForge, CapType, CAP_EXEC};

#[derive(Debug, Default, Clone)]
pub struct IrqBalanceAuditStats {
    pub allowed: u64,
    pub denied:  u64,
}

pub struct IrqBalanceSiloAuditBridge {
    pub balancer: IrqBalancer,
    pub stats:    IrqBalanceAuditStats,
}

impl IrqBalanceSiloAuditBridge {
    pub fn new(num_cores: u32) -> Self {
        IrqBalanceSiloAuditBridge { balancer: IrqBalancer::new(num_cores), stats: IrqBalanceAuditStats::default() }
    }

    /// Set IRQ core affinity for a Silo — requires Admin:EXEC cap.
    pub fn set_silo_affinity(
        &mut self,
        silo_id: u64,
        cores: Vec<u32>,
        forge: &mut CapTokenForge,
        tick: u64,
    ) -> bool {
        if !forge.check(silo_id, CapType::Admin, CAP_EXEC, 0, tick) {
            self.stats.denied += 1;
            return false;
        }
        self.stats.allowed += 1;
        self.balancer.set_silo_affinity(silo_id, cores);
        true
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IrqBalanceBridge: allowed={} denied={}", self.stats.allowed, self.stats.denied
        );
    }
}
