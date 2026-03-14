//! # Black Box Recorder — Sentinel Post-Mortem Object (Phase 84)
//!
//! ARCHITECTURE.md §7 — Sentinel: Black Box Recorder:
//! > "On vaporization → saves a **Post-Mortem Object** to Prism:"
//! > "- Full time-travel debugger log"
//! > "- Last 5 seconds of the Silo's instruction trace"
//! > "- Enables root-cause analysis without re-running the bug"
//!
//! ## Architecture Guardian: What this module provides
//! Sentinel already handles vaporization enforcement. This module provides the
//! **recording infrastructure** — the rolling ring buffer of Silo execution events
//! and the Post-Mortem Object assembly on vaporization.
//!
//! ```text
//! Normal execution (every Silo, every tick)
//!     │  Sentinel::record_trace_event(silo_id, event) → BlackBoxRecorder ring buffer
//!     │  [Keeps last 5 seconds = ~5000 events per Silo]
//!     ▼
//! Vaporization event
//!     │  BlackBoxRecorder::seal_post_mortem(silo_id, cause)
//!     ▼
//! PostMortemObject {
//!     instruction_trace,      // last 5 seconds of events
//!     syscall_log,            // last N syscalls issued
//!     memory_map_snapshot,    // final virtual address space layout
//!     violation_chain,        // sequence of law violations leading to vaporization
//!     cause,                  // what specifically triggered termination
//!     binary_oid,             // which binary was running
//! }
//!     ▼
//! Prism Ghost-Write: stored as read-only object → accessible via Timeline Slider
//! ```
//!
//! ## Why this matters for security
//! - Zero-day exploits leave traces in the instruction stream even after vaporization
//! - The time-travel trace lets security researchers replay the attack
//! - Post-Mortem feeds `digital_antibody.rs` with a precise behavioural signature

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use alloc::collections::VecDeque;

// ── Trace Event ───────────────────────────────────────────────────────────────

/// A single recorded execution event in a Silo's trace log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceEventKind {
    /// Syscall dispatched: (syscall_id, arg0)
    Syscall,
    /// Page fault handled: (fault_addr, is_write)
    PageFault,
    /// CapToken check: (token_id, granted=1/denied=0)
    CapCheck,
    /// IPC message sent to another Silo: (target_silo_id, msg_size)
    IpcSend,
    /// IPC message received: (source_silo_id, msg_size)
    IpcRecv,
    /// Network packet sent (Law 7 event): (dest_hash, size)
    NetSend,
    /// Fiber started or resumed: (fiber_id, cpu_core)
    FiberResume,
    /// Fiber preempted or yield: (fiber_id, ticks_ran)
    FiberYield,
    /// Memory allocation: (size_bytes, total_heap_bytes)
    MemAlloc,
    /// Memory free: (size_bytes, total_heap_bytes)
    MemFree,
    /// Law violation detected: (law_num, severity)
    LawViolation,
    /// GenericMarker (app-submitted, only if TRACE_WRITE CapToken held)
    AppMarker,
}

/// One entry in the trace ring buffer.
#[derive(Debug, Clone, Copy)]
pub struct TraceEvent {
    /// Which Silo this event belongs to
    pub silo_id: u64,
    /// Kernel tick of the event
    pub tick: u64,
    /// Event type
    pub kind: TraceEventKind,
    /// Two context values (meaning depends on kind)
    pub arg0: u64,
    pub arg1: u64,
}

// ── Syscall Log Entry ─────────────────────────────────────────────────────────

/// A minimal syscall log entry for post-mortem analysis.
#[derive(Debug, Clone, Copy)]
pub struct SyscallLogEntry {
    pub tick: u64,
    pub syscall_id: u32,
    pub arg0: u64,
    pub arg1: u64,
    pub result: i64, // 0 = success, negative = error code
}

// ── Vaporization Cause ────────────────────────────────────────────────────────

/// What specifically caused the Silo to be vaporized.
#[derive(Debug, Clone)]
pub enum VaporizationCause {
    /// Q-Manifest law violation (law number + evidence)
    LawViolation { law: u8, evidence: String },
    /// Unhandled page fault at address
    PageFaultUnhandled { fault_addr: u64 },
    /// Explicit termination by another Silo (with CapToken Kill)
    ExplicitTermination { killer_silo: u64 },
    /// Out-of-memory
    MemoryExhaustion { peak_bytes: u64 },
    /// Stack overflow
    StackOverflow { stack_top: u64 },
    /// Fiber spin-loop detected (Law 3 + Law 8)
    SpinLoopDetected { fiber_id: u64, blocked_ticks: u64 },
    /// Sentinel AI anomaly detection above threshold
    SentinelAnomalyScore { score: u8 },
    /// User-requested (graceful exit via Q-Shell or UI)
    UserRequested,
    /// Binary hash mismatch (Law 2 tamper detected)
    BinaryTampered { binary_oid: [u8; 32] },
}

// ── Memory Map Entry ──────────────────────────────────────────────────────────

/// A snapshot entry of the Silo's virtual address space at vaporization time.
#[derive(Debug, Clone, Copy)]
pub struct MemMapEntry {
    pub virt_start: u64,
    pub virt_end: u64,
    pub permissions: u8, // bit 0=R, bit 1=W, bit 2=X
    pub kind: MemRegionKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemRegionKind {
    Code,    // text segment (Prism-backed, immutable)
    Heap,    // dynamic allocation region
    Stack,   // fiber stacks
    Mmio,    // memory-mapped I/O (should be rare in Qindows Silos)
    Shared,  // IPC shared region (Law 6 violation if present without CapToken)
}

// ── Post-Mortem Object ────────────────────────────────────────────────────────

/// The complete post-mortem record saved to Prism on Silo vaporization.
#[derive(Debug, Clone)]
pub struct PostMortemObject {
    /// Unique post-mortem ID
    pub pm_id: u64,
    /// Silo that was vaporized
    pub silo_id: u64,
    /// Binary OID being executed
    pub binary_oid: [u8; 32],
    /// Total time Silo was alive (ticks)
    pub lifetime_ticks: u64,
    /// Tick of vaporization
    pub vaporized_at: u64,
    /// What caused vaporization
    pub cause: VaporizationCause,
    /// Last 5 seconds of trace events (up to 5000)
    pub instruction_trace: Vec<TraceEvent>,
    /// Last N syscalls
    pub syscall_log: Vec<SyscallLogEntry>,
    /// Final memory map
    pub memory_map: Vec<MemMapEntry>,
    /// Sequence of law violations leading to vaporization
    pub violation_chain: Vec<(u8, String)>, // (law_num, evidence)
    /// Peak RSS memory (bytes)
    pub peak_rss_bytes: u64,
    /// Total syscalls issued in lifetime
    pub total_syscalls: u64,
    /// Total network bytes sent (Law 7 accounting)
    pub total_net_bytes_sent: u64,
    /// Prism OID of this post-mortem (assigned on Ghost-Write)
    pub prism_oid: Option<[u8; 32]>,
    /// Behavioural signature hash (for digital_antibody.rs)
    pub behaviour_hash: [u8; 32],
}

impl PostMortemObject {
    /// Compute a behavioural signature from the trace (for antibody generation).
    pub fn compute_behaviour_hash(&self) -> [u8; 32] {
        let mut hash = [0u8; 32];
        // Mix syscall sequence into hash
        for (i, entry) in self.syscall_log.iter().enumerate() {
            let slot = i % 32;
            hash[slot] ^= (entry.syscall_id as u8).wrapping_add(i as u8);
            hash[(slot + 1) % 32] ^= (entry.arg0 & 0xFF) as u8;
        }
        // Mix violation chain
        for (law, _) in &self.violation_chain {
            hash[(*law as usize) % 32] ^= law.wrapping_mul(0x53);
        }
        hash
    }
}

// ── Per-Silo Trace Buffer ─────────────────────────────────────────────────────

/// The rolling trace ring buffer for one Silo.
struct SiloTraceBuffer {
    silo_id: u64,
    /// Binary OID (set at Silo spawn time)
    binary_oid: [u8; 32],
    /// Kernel tick when Silo was spawned
    spawn_tick: u64,
    /// Rolling event ring buffer (max 5000 events = ~5 seconds at 1000 events/sec)
    events: VecDeque<TraceEvent>,
    /// Syscall log ring buffer (last 256 syscalls)
    syscalls: VecDeque<SyscallLogEntry>,
    /// Memory map (updated on map/unmap)
    mem_map: Vec<MemMapEntry>,
    /// Running violation chain
    violations: Vec<(u8, String)>,
    /// Peak RSS
    peak_rss_bytes: u64,
    /// Current RSS
    current_rss_bytes: u64,
    /// Total syscalls
    total_syscalls: u64,
    /// Total net bytes sent
    total_net_sent: u64,
    /// Max trace events retained
    max_events: usize,
    /// Max syscall log entries
    max_syscalls: usize,
}

impl SiloTraceBuffer {
    fn new(silo_id: u64, binary_oid: [u8; 32], spawn_tick: u64) -> Self {
        SiloTraceBuffer {
            silo_id,
            binary_oid,
            spawn_tick,
            events: VecDeque::new(),
            syscalls: VecDeque::new(),
            mem_map: Vec::new(),
            violations: Vec::new(),
            peak_rss_bytes: 0,
            current_rss_bytes: 0,
            total_syscalls: 0,
            total_net_sent: 0,
            max_events: 5000,
            max_syscalls: 256,
        }
    }

    fn push_event(&mut self, event: TraceEvent) {
        if self.events.len() >= self.max_events { self.events.pop_front(); }
        match event.kind {
            TraceEventKind::MemAlloc => {
                self.current_rss_bytes = event.arg1;
                if event.arg1 > self.peak_rss_bytes { self.peak_rss_bytes = event.arg1; }
            }
            TraceEventKind::MemFree => { self.current_rss_bytes = event.arg1; }
            TraceEventKind::NetSend => { self.total_net_sent += event.arg1; }
            TraceEventKind::LawViolation => {
                self.violations.push((event.arg0 as u8, "trace-detected".to_string()));
            }
            _ => {}
        }
        self.events.push_back(event);
    }

    fn push_syscall(&mut self, entry: SyscallLogEntry) {
        if self.syscalls.len() >= self.max_syscalls { self.syscalls.pop_front(); }
        self.total_syscalls += 1;
        self.syscalls.push_back(entry);
    }
}

// ── Black Box Recorder Statistics ─────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct BlackBoxStats {
    pub silos_tracked: u64,
    pub events_recorded: u64,
    pub post_mortems_sealed: u64,
    pub antibodies_generated: u64,
    pub events_dropped: u64, // ring buffer overflow
}

// ── Black Box Recorder ────────────────────────────────────────────────────────

/// The Sentinel Black Box — continuously records all Silo execution events.
pub struct BlackBoxRecorder {
    /// Per-Silo trace buffers: silo_id → buffer
    buffers: BTreeMap<u64, SiloTraceBuffer>,
    /// Completed post-mortems (last 32 vaporized Silos)
    pub post_mortems: Vec<PostMortemObject>,
    /// Max stored post-mortems
    pub max_post_mortems: usize,
    /// Next post-mortem ID
    next_pm_id: u64,
    /// Statistics
    pub stats: BlackBoxStats,
}

impl BlackBoxRecorder {
    pub fn new() -> Self {
        BlackBoxRecorder {
            buffers: BTreeMap::new(),
            post_mortems: Vec::new(),
            max_post_mortems: 32,
            next_pm_id: 1,
            stats: BlackBoxStats::default(),
        }
    }

    /// Register a new Silo for tracing (called on spawn).
    pub fn register_silo(&mut self, silo_id: u64, binary_oid: [u8; 32], tick: u64) {
        self.buffers.insert(silo_id, SiloTraceBuffer::new(silo_id, binary_oid, tick));
        self.stats.silos_tracked += 1;
        crate::serial_println!(
            "[BLACK BOX] Silo {} registered for trace. Binary: {:02x}{:02x}...",
            silo_id, binary_oid[0], binary_oid[1]
        );
    }

    /// Record a trace event for a Silo (called from Sentinel, IRQ handlers, etc.)
    /// This must be very fast — called on every syscall and page fault.
    pub fn record(&mut self, event: TraceEvent) {
        if let Some(buf) = self.buffers.get_mut(&event.silo_id) {
            buf.push_event(event);
            self.stats.events_recorded += 1;
        }
    }

    /// Record a syscall for a Silo.
    pub fn record_syscall(&mut self, silo_id: u64, entry: SyscallLogEntry) {
        if let Some(buf) = self.buffers.get_mut(&silo_id) {
            buf.push_syscall(entry);
        }
    }

    /// Record a law violation for a Silo.
    pub fn record_violation(&mut self, silo_id: u64, law: u8, evidence: &str, tick: u64) {
        if let Some(buf) = self.buffers.get_mut(&silo_id) {
            buf.violations.push((law, evidence.to_string()));
            buf.push_event(TraceEvent {
                silo_id,
                tick,
                kind: TraceEventKind::LawViolation,
                arg0: law as u64,
                arg1: 0,
            });
        }
    }

    /// Seal a Post-Mortem Object on vaporization.
    /// Returns the completed PostMortemObject for Prism Ghost-Write and antibody generation.
    pub fn seal_post_mortem(
        &mut self,
        silo_id: u64,
        cause: VaporizationCause,
        vaporized_at: u64,
    ) -> Option<PostMortemObject> {
        let buf = self.buffers.remove(&silo_id)?;
        let pm_id = self.next_pm_id;
        self.next_pm_id += 1;
        let lifetime_ticks = vaporized_at.saturating_sub(buf.spawn_tick);

        crate::serial_println!(
            "[BLACK BOX] Sealing Post-Mortem #{} for Silo {} (lifetime={}ticks, {} events, {} syscalls).",
            pm_id, silo_id, lifetime_ticks, buf.events.len(), buf.syscalls.len()
        );

        let mut pm = PostMortemObject {
            pm_id,
            silo_id,
            binary_oid: buf.binary_oid,
            lifetime_ticks,
            vaporized_at,
            cause,
            instruction_trace: buf.events.into_iter().collect(),
            syscall_log: buf.syscalls.into_iter().collect(),
            memory_map: buf.mem_map,
            violation_chain: buf.violations,
            peak_rss_bytes: buf.peak_rss_bytes,
            total_syscalls: buf.total_syscalls,
            total_net_bytes_sent: buf.total_net_sent,
            prism_oid: None,
            behaviour_hash: [0u8; 32],
        };

        pm.behaviour_hash = pm.compute_behaviour_hash();

        crate::serial_println!(
            "[BLACK BOX] Behaviour hash: {:02x}{:02x}{:02x}{:02x}... (→ digital_antibody.rs)",
            pm.behaviour_hash[0], pm.behaviour_hash[1],
            pm.behaviour_hash[2], pm.behaviour_hash[3]
        );

        self.stats.post_mortems_sealed += 1;
        if self.post_mortems.len() >= self.max_post_mortems { self.post_mortems.remove(0); }
        self.post_mortems.push(pm.clone());

        Some(pm)
    }

    /// Retrieve a post-mortem by Silo ID (for Aether's time-travel debugger).
    pub fn get_post_mortem(&self, silo_id: u64) -> Option<&PostMortemObject> {
        self.post_mortems.iter().rev().find(|pm| pm.silo_id == silo_id)
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Sentinel Black Box Recorder (§7)   ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Silos tracked:    {:>6}              ║", self.stats.silos_tracked);
        crate::serial_println!("║ Events recorded:  {:>6}K             ║", self.stats.events_recorded / 1000);
        crate::serial_println!("║ Post-mortems:     {:>6}              ║", self.stats.post_mortems_sealed);
        crate::serial_println!("║ Stored (last):    {:>6}              ║", self.post_mortems.len());
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
