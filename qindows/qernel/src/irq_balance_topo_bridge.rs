//! # IRQ Balance Topology Bridge (Phase 160)
//!
//! ## Architecture Guardian: The Gap
//! `irq_balance.rs` implements `IrqBalancer`:
//! - `new(num_cores: u32)` — creates balancer for N cores
//! - `register(device, irq_type, silo_id, policy)` → IRQ ID
//! - `handle_irq(irq)` — routes IRQ
//! - `rebalance()` → count of IRQs re-routed
//!
//! **Missing link**: IRQ balancer never knew which Silos were running on
//! which CPUs, and `register()` was never called at device discovery.
//!
//! This module provides `IrqBalanceTopologyBridge`:
//! 1. `register_device_irq()` — registers device IRQ at discovery time
//! 2. `on_irq()` — routes hardware IRQ through balancer
//! 3. `on_rebalance_tick()` — periodic rebalance

extern crate alloc;

use crate::irq_balance::{IrqBalancer, IrqType, BalancePolicy};

#[derive(Debug, Default, Clone)]
pub struct IrqBalanceBridgeStats {
    pub irqs_registered: u64,
    pub irqs_handled:    u64,
    pub rebalances:      u64,
}

pub struct IrqBalanceTopologyBridge {
    pub balancer: IrqBalancer,
    pub stats:    IrqBalanceBridgeStats,
}

impl IrqBalanceTopologyBridge {
    pub fn new(num_cores: u32) -> Self {
        IrqBalanceTopologyBridge {
            balancer: IrqBalancer::new(num_cores),
            stats:    IrqBalanceBridgeStats::default(),
        }
    }

    /// Register a device IRQ with the balancer at device discovery.
    pub fn register_device_irq(
        &mut self,
        device_name: &str,
        irq_type: IrqType,
        owner_silo: Option<u64>,
    ) -> u32 {
        self.stats.irqs_registered += 1;
        self.balancer.register(device_name, irq_type, owner_silo, BalancePolicy::LeastLoaded)
    }

    /// Route a hardware IRQ.
    pub fn on_irq(&mut self, irq: u32) {
        self.stats.irqs_handled += 1;
        self.balancer.handle_irq(irq);
    }

    /// Set Silo CPU affinity so balancer can route affinely.
    pub fn set_silo_affinity(&mut self, silo_id: u64, cores: alloc::vec::Vec<u32>) {
        self.balancer.set_silo_affinity(silo_id, cores);
    }

    /// Periodic rebalance (call from APIC timer at low frequency).
    pub fn on_rebalance_tick(&mut self) {
        self.stats.rebalances += 1;
        self.balancer.rebalance();
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  IrqBalanceBridge: registered={} handled={} rebalances={}",
            self.stats.irqs_registered, self.stats.irqs_handled, self.stats.rebalances
        );
    }
}
