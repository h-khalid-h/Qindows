//! # Qernel Core Dump Generator
//!
//! Post-mortem debugging: captures CPU state, memory regions,
//! kernel logs, and Silo metadata when a panic or fault occurs.
//! Dumps are written to Prism as immutable objects for later
//! analysis. Supports both mini-dumps (registers + stack) and
//! full dumps (complete memory snapshot).

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── Dump Types ─────────────────────────────────────────────────────────────

/// Type of core dump.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpType {
    /// Registers + stack only (~64KB)
    Mini,
    /// Registers + stack + heap of faulting Silo (~1-64MB)
    Standard,
    /// Complete memory snapshot (can be very large)
    Full,
    /// Kernel-only: registers + kernel stack + log ring
    Kernel,
}

/// Reason the dump was triggered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DumpReason {
    /// Kernel panic
    KernelPanic,
    /// Page fault (unmapped address)
    PageFault,
    /// General protection fault
    GeneralProtectionFault,
    /// Double fault
    DoubleFault,
    /// Stack overflow
    StackOverflow,
    /// Watchdog timeout
    WatchdogTimeout,
    /// User-requested (debug command)
    UserRequested,
    /// Assertion failure
    AssertionFailed,
    /// Out of memory
    OutOfMemory,
    /// Invalid opcode
    InvalidOpcode,
}

// ─── CPU State ──────────────────────────────────────────────────────────────

/// Captured CPU register state (x86-64).
#[derive(Debug, Clone, Copy, Default)]
pub struct CpuRegisters {
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    pub rip: u64,
    pub rflags: u64,
    pub cs: u64,
    pub ss: u64,
    pub cr2: u64,  // Page fault address
    pub cr3: u64,  // Page table root
}

/// A memory region in the dump.
#[derive(Debug, Clone)]
pub struct MemoryRegion {
    /// Virtual address start
    pub vaddr: u64,
    /// Region size
    pub size: u64,
    /// Permissions (rwx as bits)
    pub permissions: u8,
    /// Label (e.g., "kernel_stack", "heap", "code")
    pub label: String,
    /// Raw memory contents
    pub data: Vec<u8>,
}

impl MemoryRegion {
    pub fn is_readable(&self) -> bool { self.permissions & 0x4 != 0 }
    pub fn is_writable(&self) -> bool { self.permissions & 0x2 != 0 }
    pub fn is_executable(&self) -> bool { self.permissions & 0x1 != 0 }
}

/// A stack frame in the backtrace.
#[derive(Debug, Clone)]
pub struct StackFrame {
    /// Frame index (0 = top of stack)
    pub index: u32,
    /// Instruction pointer
    pub rip: u64,
    /// Frame pointer
    pub rbp: u64,
    /// Symbol name (if resolved)
    pub symbol: Option<String>,
    /// Source file (if debug info available)
    pub file: Option<String>,
    /// Line number
    pub line: Option<u32>,
}

// ─── Core Dump ──────────────────────────────────────────────────────────────

/// A complete core dump.
#[derive(Debug, Clone)]
pub struct CoreDump {
    /// Dump identifier
    pub id: u64,
    /// Dump type
    pub dump_type: DumpType,
    /// Why the dump was created
    pub reason: DumpReason,
    /// Timestamp (ns since boot)
    pub timestamp: u64,
    /// CPU registers at time of fault
    pub registers: CpuRegisters,
    /// Stack backtrace
    pub backtrace: Vec<StackFrame>,
    /// Memory regions included in dump
    pub memory_regions: Vec<MemoryRegion>,
    /// Faulting Silo ID (None if kernel context)
    pub silo_id: Option<u64>,
    /// CPU core that faulted
    pub cpu_core: u32,
    /// Panic message (if kernel panic)
    pub panic_message: Option<String>,
    /// Kernel log ring (last N entries)
    pub log_ring: Vec<String>,
    /// Total dump size in bytes
    pub total_size: u64,
    /// Has this dump been saved to Prism?
    pub persisted: bool,
}

// ─── Dump Manager ───────────────────────────────────────────────────────────

/// Core dump configuration.
#[derive(Debug, Clone)]
pub struct DumpConfig {
    /// Default dump type
    pub default_type: DumpType,
    /// Maximum dump size in bytes (0 = unlimited)
    pub max_size: u64,
    /// Maximum number of dumps to retain
    pub max_retained: usize,
    /// Number of log ring entries to capture
    pub log_ring_size: usize,
    /// Maximum stack depth for backtrace
    pub max_backtrace_depth: u32,
    /// Auto-dump on kernel panic?
    pub auto_dump_on_panic: bool,
    /// Auto-dump on watchdog timeout?
    pub auto_dump_on_watchdog: bool,
}

impl Default for DumpConfig {
    fn default() -> Self {
        DumpConfig {
            default_type: DumpType::Standard,
            max_size: 64 * 1024 * 1024, // 64 MB
            max_retained: 16,
            log_ring_size: 256,
            max_backtrace_depth: 64,
            auto_dump_on_panic: true,
            auto_dump_on_watchdog: true,
        }
    }
}

/// Dump manager statistics.
#[derive(Debug, Clone, Default)]
pub struct DumpStats {
    pub dumps_created: u64,
    pub dumps_persisted: u64,
    pub dumps_pruned: u64,
    pub total_bytes_captured: u64,
    pub panics_caught: u64,
    pub faults_caught: u64,
}

/// The Core Dump Manager.
pub struct DumpManager {
    /// Retained dumps
    pub dumps: Vec<CoreDump>,
    /// Configuration
    pub config: DumpConfig,
    /// Next dump ID
    next_id: u64,
    /// Statistics
    pub stats: DumpStats,
}

impl DumpManager {
    pub fn new(config: DumpConfig) -> Self {
        DumpManager {
            dumps: Vec::new(),
            config,
            next_id: 1,
            stats: DumpStats::default(),
        }
    }

    /// Capture a core dump.
    pub fn capture(
        &mut self,
        reason: DumpReason,
        registers: CpuRegisters,
        silo_id: Option<u64>,
        cpu_core: u32,
        panic_msg: Option<&str>,
        now: u64,
    ) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        // Build backtrace by walking frame pointers
        let backtrace = self.walk_stack(&registers);

        // Update stats
        self.stats.dumps_created += 1;
        match reason {
            DumpReason::KernelPanic => self.stats.panics_caught += 1,
            DumpReason::PageFault
            | DumpReason::GeneralProtectionFault
            | DumpReason::DoubleFault => self.stats.faults_caught += 1,
            _ => {}
        }

        let dump = CoreDump {
            id,
            dump_type: self.config.default_type,
            reason,
            timestamp: now,
            registers,
            backtrace,
            memory_regions: Vec::new(), // Populated by capture_memory()
            silo_id,
            cpu_core,
            panic_message: panic_msg.map(String::from),
            log_ring: Vec::new(), // Would be filled from kernel log
            total_size: 0,
            persisted: false,
        };

        crate::serial_println!(
            "[COREDUMP] #{} {:?} on core {} (Silo: {:?})",
            id, reason, cpu_core, silo_id
        );

        self.dumps.push(dump);
        self.prune_old();

        id
    }

    /// Walk the stack to build a backtrace.
    fn walk_stack(&self, regs: &CpuRegisters) -> Vec<StackFrame> {
        let mut frames = Vec::new();
        let mut rbp = regs.rbp;
        let mut rip = regs.rip;

        for i in 0..self.config.max_backtrace_depth {
            frames.push(StackFrame {
                index: i,
                rip,
                rbp,
                symbol: None, // Would resolve from symbol table
                file: None,
                line: None,
            });

            // In a real kernel: read [rbp] for next frame, [rbp+8] for return addr
            // For safety, we stop if rbp is 0 or looks invalid
            if rbp == 0 || rbp < 0x1000 {
                break;
            }

            // Simulate frame walk (actual impl reads from memory)
            rip = 0; // Would be read from [rbp + 8]
            rbp = 0; // Would be read from [rbp]
            if rip == 0 { break; }
        }

        frames
    }

    /// Add a memory region to an existing dump.
    pub fn add_memory_region(
        &mut self,
        dump_id: u64,
        vaddr: u64,
        data: Vec<u8>,
        permissions: u8,
        label: &str,
    ) {
        if let Some(dump) = self.dumps.iter_mut().find(|d| d.id == dump_id) {
            let size = data.len() as u64;

            // Check size limit
            if self.config.max_size > 0 && dump.total_size + size > self.config.max_size {
                return; // Would exceed limit
            }

            dump.total_size += size;
            self.stats.total_bytes_captured += size;

            dump.memory_regions.push(MemoryRegion {
                vaddr,
                size,
                permissions,
                label: String::from(label),
                data,
            });
        }
    }

    /// Mark a dump as persisted to Prism.
    pub fn mark_persisted(&mut self, dump_id: u64) {
        if let Some(dump) = self.dumps.iter_mut().find(|d| d.id == dump_id) {
            dump.persisted = true;
            self.stats.dumps_persisted += 1;
        }
    }

    /// Prune old dumps when exceeding retention limit.
    fn prune_old(&mut self) {
        while self.dumps.len() > self.config.max_retained {
            self.dumps.remove(0);
            self.stats.dumps_pruned += 1;
        }
    }

    /// Get the most recent dump.
    pub fn latest(&self) -> Option<&CoreDump> {
        self.dumps.last()
    }

    /// Get a dump by ID.
    pub fn get(&self, dump_id: u64) -> Option<&CoreDump> {
        self.dumps.iter().find(|d| d.id == dump_id)
    }

    /// Get all dumps for a specific Silo.
    pub fn dumps_for_silo(&self, silo_id: u64) -> Vec<&CoreDump> {
        self.dumps.iter()
            .filter(|d| d.silo_id == Some(silo_id))
            .collect()
    }

    /// Generate a human-readable summary of a dump.
    pub fn summarize(&self, dump_id: u64) -> Option<String> {
        let dump = self.get(dump_id)?;

        let mut summary = String::new();
        summary.push_str(&alloc::format!(
            "=== Core Dump #{} ===\n\
             Reason: {:?}\n\
             Type: {:?}\n\
             CPU Core: {}\n\
             Silo: {:?}\n\
             Size: {} bytes\n\
             Backtrace ({} frames):\n",
            dump.id, dump.reason, dump.dump_type,
            dump.cpu_core, dump.silo_id,
            dump.total_size, dump.backtrace.len()
        ));

        for frame in &dump.backtrace {
            if let Some(ref sym) = frame.symbol {
                summary.push_str(&alloc::format!(
                    "  #{}: 0x{:016x} {}\n", frame.index, frame.rip, sym
                ));
            } else {
                summary.push_str(&alloc::format!(
                    "  #{}: 0x{:016x} <unknown>\n", frame.index, frame.rip
                ));
            }
        }

        if let Some(ref msg) = dump.panic_message {
            summary.push_str(&alloc::format!("Panic: {}\n", msg));
        }

        summary.push_str(&alloc::format!(
            "Memory regions: {}\n\
             RIP: 0x{:016x}  RSP: 0x{:016x}\n\
             CR2: 0x{:016x}  CR3: 0x{:016x}\n",
            dump.memory_regions.len(),
            dump.registers.rip, dump.registers.rsp,
            dump.registers.cr2, dump.registers.cr3,
        ));

        Some(summary)
    }
}
