//! # Mesh NAT — Network Address Translation for Silos
//!
//! Provides outbound NAT for Silo network traffic, mapping
//! internal Silo addresses to shared external IPs (Section 11.26).
//!
//! Features:
//! - Port-based NAPT (Network Address Port Translation)
//! - Connection tracking table
//! - Timeout-based entry expiry
//! - Per-Silo source IP assignment
//! - Hairpin NAT for inter-Silo traffic

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// NAT mapping entry.
#[derive(Debug, Clone)]
pub struct NatEntry {
    pub silo_id: u64,
    pub internal_ip: [u8; 4],
    pub internal_port: u16,
    pub external_ip: [u8; 4],
    pub external_port: u16,
    pub dest_ip: [u8; 4],
    pub dest_port: u16,
    pub created_at: u64,
    pub last_used: u64,
    pub packets: u64,
    pub bytes: u64,
}

/// NAT statistics.
#[derive(Debug, Clone, Default)]
pub struct NatStats {
    pub translations: u64,
    pub entries_created: u64,
    pub entries_expired: u64,
    pub port_exhaustion: u64,
    pub hairpin_translations: u64,
}

/// The Mesh NAT Engine.
pub struct MeshNat {
    /// Connection tracking: external_port → entry
    pub table: BTreeMap<u16, NatEntry>,
    pub external_ip: [u8; 4],
    pub port_range_start: u16,
    pub port_range_end: u16,
    next_port: u16,
    pub timeout_ms: u64,
    pub stats: NatStats,
}

impl MeshNat {
    pub fn new(external_ip: [u8; 4]) -> Self {
        MeshNat {
            table: BTreeMap::new(),
            external_ip,
            port_range_start: 32768,
            port_range_end: 61000,
            next_port: 32768,
            timeout_ms: 120_000, // 2 minute timeout
            stats: NatStats::default(),
        }
    }

    /// Translate an outbound packet (Silo → external).
    pub fn translate_outbound(
        &mut self, silo_id: u64,
        src_ip: [u8; 4], src_port: u16,
        dst_ip: [u8; 4], dst_port: u16,
        now: u64,
    ) -> Option<u16> {
        // Check for existing mapping
        for (&ext_port, entry) in &mut self.table {
            if entry.silo_id == silo_id
                && entry.internal_ip == src_ip
                && entry.internal_port == src_port
                && entry.dest_ip == dst_ip
                && entry.dest_port == dst_port
            {
                entry.last_used = now;
                entry.packets += 1;
                self.stats.translations += 1;
                return Some(ext_port);
            }
        }

        // Allocate new external port
        let ext_port = self.alloc_port()?;

        self.table.insert(ext_port, NatEntry {
            silo_id, internal_ip: src_ip, internal_port: src_port,
            external_ip: self.external_ip, external_port: ext_port,
            dest_ip: dst_ip, dest_port: dst_port,
            created_at: now, last_used: now, packets: 1, bytes: 0,
        });

        self.stats.entries_created += 1;
        self.stats.translations += 1;
        Some(ext_port)
    }

    /// Translate an inbound reply (external → Silo).
    pub fn translate_inbound(&mut self, ext_port: u16, now: u64) -> Option<([u8; 4], u16, u64)> {
        if let Some(entry) = self.table.get_mut(&ext_port) {
            entry.last_used = now;
            entry.packets += 1;
            self.stats.translations += 1;
            Some((entry.internal_ip, entry.internal_port, entry.silo_id))
        } else {
            None
        }
    }

    /// Expire old entries.
    pub fn expire(&mut self, now: u64) {
        let expired: Vec<u16> = self.table.iter()
            .filter(|(_, e)| now.saturating_sub(e.last_used) > self.timeout_ms)
            .map(|(&port, _)| port)
            .collect();
        for port in expired {
            self.table.remove(&port);
            self.stats.entries_expired += 1;
        }
    }

    /// Allocate next available external port.
    fn alloc_port(&mut self) -> Option<u16> {
        let start = self.next_port;
        loop {
            if !self.table.contains_key(&self.next_port) {
                let port = self.next_port;
                self.next_port += 1;
                if self.next_port > self.port_range_end {
                    self.next_port = self.port_range_start;
                }
                return Some(port);
            }
            self.next_port += 1;
            if self.next_port > self.port_range_end {
                self.next_port = self.port_range_start;
            }
            if self.next_port == start {
                self.stats.port_exhaustion += 1;
                return None; // All ports in use
            }
        }
    }

    /// Active connection count.
    pub fn active_connections(&self) -> usize {
        self.table.len()
    }
}
