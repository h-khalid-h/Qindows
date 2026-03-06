//! # IRQ Balancer — Interrupt Affinity Across Cores + Silos
//!
//! Distributes hardware interrupts across CPU cores with
//! per-Silo isolation (Section 9.5).
//!
//! Features:
//! - Per-Silo IRQ affinity masks
//! - Load-based balancing (spread IRQs evenly)
//! - Priority IRQ pinning (NVMe, GPU locked to specific cores)
//! - Rebalance on CPU hotplug events
//! - IRQ coalescing hints

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// IRQ type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqType {
    Msi,
    MsiX,
    Legacy,
    Ipi,
}

/// IRQ balance policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalancePolicy {
    RoundRobin,
    LeastLoaded,
    Pinned,
    SiloAffine,
}

/// An IRQ descriptor.
#[derive(Debug, Clone)]
pub struct IrqDesc {
    pub irq: u32,
    pub irq_type: IrqType,
    pub device_name: alloc::string::String,
    pub silo_id: Option<u64>,
    pub affinity_core: u32,
    pub policy: BalancePolicy,
    pub count: u64,
    pub rate_per_sec: u32,
}

/// Per-core IRQ load.
#[derive(Debug, Clone)]
pub struct CoreLoad {
    pub core_id: u32,
    pub irq_count: u32,
    pub total_interrupts: u64,
    pub online: bool,
}

/// IRQ balancer statistics.
#[derive(Debug, Clone, Default)]
pub struct IrqStats {
    pub irqs_registered: u64,
    pub rebalances: u64,
    pub migrations: u64,
    pub total_interrupts: u64,
}

/// The IRQ Balancer.
pub struct IrqBalancer {
    pub irqs: BTreeMap<u32, IrqDesc>,
    pub cores: BTreeMap<u32, CoreLoad>,
    pub silo_masks: BTreeMap<u64, Vec<u32>>, // Silo → allowed cores
    next_irq: u32,
    pub stats: IrqStats,
}

impl IrqBalancer {
    pub fn new(num_cores: u32) -> Self {
        let mut cores = BTreeMap::new();
        for i in 0..num_cores {
            cores.insert(i, CoreLoad {
                core_id: i, irq_count: 0, total_interrupts: 0, online: true,
            });
        }
        IrqBalancer {
            irqs: BTreeMap::new(),
            cores,
            silo_masks: BTreeMap::new(),
            next_irq: 32, // Start after CPU exceptions
            stats: IrqStats::default(),
        }
    }

    /// Set allowed cores for a Silo.
    pub fn set_silo_affinity(&mut self, silo_id: u64, cores: Vec<u32>) {
        self.silo_masks.insert(silo_id, cores);
    }

    /// Register an IRQ.
    pub fn register(&mut self, device: &str, irq_type: IrqType, silo_id: Option<u64>, policy: BalancePolicy) -> u32 {
        let irq = self.next_irq;
        self.next_irq += 1;

        // Find best core
        let core = self.find_core(silo_id, policy);

        self.irqs.insert(irq, IrqDesc {
            irq, irq_type, device_name: alloc::string::String::from(device),
            silo_id, affinity_core: core, policy, count: 0, rate_per_sec: 0,
        });

        if let Some(c) = self.cores.get_mut(&core) {
            c.irq_count += 1;
        }

        self.stats.irqs_registered += 1;
        irq
    }

    /// Record an interrupt.
    pub fn handle_irq(&mut self, irq: u32) {
        if let Some(desc) = self.irqs.get_mut(&irq) {
            desc.count += 1;
        }
        self.stats.total_interrupts += 1;
    }

    /// Rebalance all non-pinned IRQs.
    pub fn rebalance(&mut self) -> u32 {
        self.stats.rebalances += 1;
        let mut migrated = 0u32;

        // Reset core counts
        for core in self.cores.values_mut() {
            core.irq_count = 0;
        }

        // Collect IRQ info
        let irq_info: Vec<(u32, Option<u64>, BalancePolicy)> = self.irqs.values()
            .map(|d| (d.irq, d.silo_id, d.policy))
            .collect();

        for (irq, silo_id, policy) in irq_info {
            if policy == BalancePolicy::Pinned {
                // Keep pinned IRQs where they are
                if let Some(desc) = self.irqs.get(&irq) {
                    if let Some(c) = self.cores.get_mut(&desc.affinity_core) {
                        c.irq_count += 1;
                    }
                }
                continue;
            }

            let new_core = self.find_core(silo_id, policy);
            if let Some(desc) = self.irqs.get_mut(&irq) {
                if desc.affinity_core != new_core {
                    desc.affinity_core = new_core;
                    migrated += 1;
                    self.stats.migrations += 1;
                }
            }
            if let Some(c) = self.cores.get_mut(&new_core) {
                c.irq_count += 1;
            }
        }

        migrated
    }

    fn find_core(&self, silo_id: Option<u64>, _policy: BalancePolicy) -> u32 {
        // Get allowed cores for this Silo
        let allowed: Option<&Vec<u32>> = silo_id
            .and_then(|sid| self.silo_masks.get(&sid));

        // Find least-loaded core
        self.cores.values()
            .filter(|c| c.online)
            .filter(|c| match allowed {
                Some(mask) => mask.contains(&c.core_id),
                None => true,
            })
            .min_by_key(|c| c.irq_count)
            .map(|c| c.core_id)
            .unwrap_or(0)
    }
}
