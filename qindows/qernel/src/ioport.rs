//! # I/O Port Manager — Legacy Port I/O Virtualization
//!
//! Virtualizes x86 I/O port access for legacy device
//! emulation and per-Silo port isolation (Section 9.15).
//!
//! Features:
//! - Per-Silo I/O port bitmap
//! - Port range allocation
//! - Trap-and-emulate for legacy devices
//! - Port access logging
//! - Conflict detection

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// I/O port access type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortAccess {
    Read,
    Write,
    ReadWrite,
}

/// A port range allocation.
#[derive(Debug, Clone)]
pub struct PortAllocation {
    pub start: u16,
    pub end: u16,
    pub silo_id: u64,
    pub access: PortAccess,
    pub trapped: bool,
    pub accesses: u64,
}

/// I/O port statistics.
#[derive(Debug, Clone, Default)]
pub struct IoPortStats {
    pub allocations: u64,
    pub reads: u64,
    pub writes: u64,
    pub traps: u64,
    pub conflicts: u64,
}

/// The I/O Port Manager.
pub struct IoPortManager {
    pub allocations: Vec<PortAllocation>,
    /// Port → owning Silo lookup
    pub port_owners: BTreeMap<u16, u64>,
    pub stats: IoPortStats,
}

impl IoPortManager {
    pub fn new() -> Self {
        IoPortManager {
            allocations: Vec::new(),
            port_owners: BTreeMap::new(),
            stats: IoPortStats::default(),
        }
    }

    /// Allocate a port range to a Silo.
    pub fn allocate(&mut self, start: u16, end: u16, silo_id: u64, access: PortAccess, trapped: bool) -> Result<(), &'static str> {
        if start > end { return Err("Invalid range"); }

        // Check for conflicts
        for port in start..=end {
            if let Some(&owner) = self.port_owners.get(&port) {
                if owner != silo_id {
                    self.stats.conflicts += 1;
                    return Err("Port conflict");
                }
            }
        }

        // Register ownership
        for port in start..=end {
            self.port_owners.insert(port, silo_id);
        }

        self.allocations.push(PortAllocation {
            start, end, silo_id, access, trapped, accesses: 0,
        });
        self.stats.allocations += 1;
        Ok(())
    }

    /// Check if a Silo can read a port.
    pub fn can_read(&self, port: u16, silo_id: u64) -> bool {
        self.port_owners.get(&port).copied() == Some(silo_id) &&
        self.allocations.iter().any(|a| {
            a.silo_id == silo_id && port >= a.start && port <= a.end &&
            matches!(a.access, PortAccess::Read | PortAccess::ReadWrite)
        })
    }

    /// Check if a Silo can write a port.
    pub fn can_write(&self, port: u16, silo_id: u64) -> bool {
        self.port_owners.get(&port).copied() == Some(silo_id) &&
        self.allocations.iter().any(|a| {
            a.silo_id == silo_id && port >= a.start && port <= a.end &&
            matches!(a.access, PortAccess::Write | PortAccess::ReadWrite)
        })
    }

    /// Record a port I/O access.
    pub fn record_access(&mut self, port: u16, is_write: bool) {
        if is_write { self.stats.writes += 1; } else { self.stats.reads += 1; }

        for alloc in &mut self.allocations {
            if port >= alloc.start && port <= alloc.end {
                alloc.accesses += 1;
                if alloc.trapped { self.stats.traps += 1; }
                break;
            }
        }
    }

    /// Release all ports for a Silo.
    pub fn release_silo(&mut self, silo_id: u64) {
        self.allocations.retain(|a| a.silo_id != silo_id);
        self.port_owners.retain(|_, &mut owner| owner != silo_id);
    }
}
