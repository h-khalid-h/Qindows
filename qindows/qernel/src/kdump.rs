//! # KDump — Crash Dump Collection & Analysis
//!
//! Captures kernel state on crash for post-mortem analysis
//! (Section 12.5). Writes crash dump to reserved memory
//! area, then flushes to persistent storage on reboot.
//!
//! Features:
//! - Full and mini dump modes
//! - Per-Silo crash isolation (crash in one Silo doesn't dump all)
//! - Stack trace capture
//! - Register snapshot
//! - Crash reason classification

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Dump type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpType {
    Mini,    // Registers + stack + crash thread only
    Kernel,  // Full kernel state
    Full,    // Kernel + all Silo memory
}

/// Crash reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashReason {
    PageFault,
    DoubleFault,
    StackOverflow,
    InvalidOpcode,
    DivideByZero,
    GeneralProtection,
    Watchdog,
    Assertion,
    Panic,
    Unknown,
}

/// A crash dump record.
#[derive(Debug, Clone)]
pub struct CrashDump {
    pub id: u64,
    pub silo_id: Option<u64>,
    pub dump_type: DumpType,
    pub reason: CrashReason,
    pub instruction_ptr: u64,
    pub stack_ptr: u64,
    pub registers: [u64; 16],
    pub stack_trace: Vec<u64>,
    pub message: String,
    pub timestamp: u64,
    pub dump_size: u64,
}

/// KDump statistics.
#[derive(Debug, Clone, Default)]
pub struct KDumpStats {
    pub dumps_collected: u64,
    pub mini_dumps: u64,
    pub full_dumps: u64,
    pub bytes_written: u64,
}

/// The KDump Manager.
pub struct KDump {
    pub dumps: BTreeMap<u64, CrashDump>,
    pub max_dumps: usize,
    pub default_type: DumpType,
    next_id: u64,
    pub stats: KDumpStats,
}

impl KDump {
    pub fn new() -> Self {
        KDump {
            dumps: BTreeMap::new(),
            max_dumps: 64,
            default_type: DumpType::Mini,
            next_id: 1,
            stats: KDumpStats::default(),
        }
    }

    /// Record a crash dump.
    pub fn capture(&mut self, silo_id: Option<u64>, reason: CrashReason,
                   ip: u64, sp: u64, regs: [u64; 16], stack: Vec<u64>,
                   msg: &str, now: u64) -> u64 {
        // Evict oldest if at capacity
        if self.dumps.len() >= self.max_dumps {
            if let Some(&oldest_id) = self.dumps.keys().next() {
                self.dumps.remove(&oldest_id);
            }
        }

        let id = self.next_id;
        self.next_id += 1;
        let dump_type = self.default_type;

        let size = match dump_type {
            DumpType::Mini => 4096 + (stack.len() as u64 * 8),
            DumpType::Kernel => 1024 * 1024,
            DumpType::Full => 256 * 1024 * 1024,
        };

        self.dumps.insert(id, CrashDump {
            id, silo_id, dump_type, reason,
            instruction_ptr: ip, stack_ptr: sp,
            registers: regs, stack_trace: stack,
            message: String::from(msg), timestamp: now,
            dump_size: size,
        });

        self.stats.dumps_collected += 1;
        self.stats.bytes_written += size;
        match dump_type {
            DumpType::Mini => self.stats.mini_dumps += 1,
            DumpType::Full | DumpType::Kernel => self.stats.full_dumps += 1,
        }

        id
    }

    /// Get dump by ID.
    pub fn get(&self, id: u64) -> Option<&CrashDump> {
        self.dumps.get(&id)
    }

    /// Get dumps for a Silo.
    pub fn dumps_for_silo(&self, silo_id: u64) -> Vec<&CrashDump> {
        self.dumps.values()
            .filter(|d| d.silo_id == Some(silo_id))
            .collect()
    }

    /// Clear all dumps.
    pub fn clear(&mut self) {
        self.dumps.clear();
    }
}
