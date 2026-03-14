//! # Q-Metrics — Performance Counters & Observatory Telemetry (Phase 71)
//!
//! Q-Metrics is the kernel's internal health observatory. It tracks the
//! real-time performance numbers that back the ARCHITECTURE.md benchmarks:
//!
//! ## ARCHITECTURE.md § System Benchmarks (targets this module validates):
//! > Cold Boot: <1.5 seconds
//! > Input Latency: <2ms
//! > RAM (Idle): ~450 MB
//! > System Update: Atomic hot-swap, zero reboot
//!
//! ## Architecture Guardian: Observation Without Coupling
//! Q-Metrics is a **purely passive** observer. Every other module calls
//! `Q_METRICS.record(event)` — Q-Metrics does NOT call back into them.
//! This is the Observer pattern: no tight coupling, no circular imports.
//!
//! ```text
//! Any kernel module → Q_METRICS.record(MetricEvent)
//!                           ↓
//!                   MetricStore (this module)
//!                           ↓
//!            Aether Perf Overlay / Sentinel thresholds
//! ```
//!
//! ## Relationship to PMC (pmc.rs)
//! - `pmc.rs`: reads raw Hardware Performance Monitoring Counters (CPU cycles, cache misses)
//! - `q_metrics.rs`: high-level OS-semantic events (boot time, silo latency, syscall rate)
//! These are complementary, not overlapping.

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ── Metric Events ─────────────────────────────────────────────────────────────

/// A discrete metric event recorded by any kernel module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    // ── Boot metrics ──────────────────────────────────────────────────────────
    /// Kernel _start() entry to first HLT — total boot time (ticks)
    BootTimeTotal,
    /// UEFI GOP handoff to framebuffer init
    BootPhaseGop,
    /// IDT + GDT init complete
    BootPhaseInterrupts,
    /// All AP cores awake (SMP ready)
    BootPhaseSmpReady,
    /// First Silo (Q-Shell) launched
    BootPhaseFirstSilo,

    // ── Latency metrics ───────────────────────────────────────────────────────
    /// Keyboard/mouse input to Aether update (input pipeline latency, ticks)
    InputLatency,
    /// Q-Ring syscall door-bell to kernel handler dispatch (ticks)
    SyscallDispatchLatency,
    /// Silo-spawn time: silo_launch() to first instruction (ticks)
    SiloSpawnLatency,
    /// Context switch duration (ticks)
    ContextSwitchTicks,

    // ── Memory metrics ────────────────────────────────────────────────────────
    /// Current physical frames in use (absolute count)
    PhysFramesUsed,
    /// Kernel heap bytes in use
    KernelHeapBytes,
    /// Silo virtual memory bytes committed
    SiloVmBytes,
    /// Page cache hits (unified buffer cache)
    PageCacheHits,
    /// Page cache misses
    PageCacheMisses,

    // ── Throughput metrics ────────────────────────────────────────────────────
    /// Q-Ring entries processed per second
    QRingThroughput,
    /// NVMe read throughput (bytes/sec)
    NvmeReadBps,
    /// NVMe write throughput (bytes/sec)
    NvmeWriteBps,
    /// Q-Fabric network egress (bytes/sec)
    NetEgressBps,

    // ── Sentinel / security metrics ───────────────────────────────────────────
    /// Number of Silo vaporizations since boot
    SiloVaporizations,
    /// Law violations detected (by law index 1–10)
    LawViolation { law: u8 },
    /// Sentinel scan duration per Silo (ticks)
    SentinelScanTicks,

    // ── Energy metrics ────────────────────────────────────────────────────────
    /// Estimated system power draw (milliwatts)
    SystemPowerMw,
    /// Number of deep-sleeping Fibers (Law 8)
    FibersDeepSleeping,
}

// ── Metric Value ──────────────────────────────────────────────────────────────

/// A recorded metric value with timestamp.
#[derive(Debug, Clone, Copy)]
pub struct MetricSample {
    pub kind: MetricKind,
    /// Value (interpretation depends on kind)
    pub value: u64,
    /// Kernel tick when recorded
    pub tick: u64,
}

// ── Aggregated Counter ────────────────────────────────────────────────────────

/// Aggregated statistics for a metric kind.
#[derive(Debug, Clone, Copy, Default)]
pub struct MetricAggregate {
    /// Total number of samples recorded
    pub count: u64,
    /// Sum of all sample values
    pub sum: u64,
    /// Minimum sample value seen
    pub min: u64,
    /// Maximum sample value seen
    pub max: u64,
    /// Most recent sample value
    pub last: u64,
    /// Most recent sample tick
    pub last_tick: u64,
}

impl MetricAggregate {
    pub fn update(&mut self, value: u64, tick: u64) {
        if self.count == 0 {
            self.min = value;
            self.max = value;
        }
        self.count  += 1;
        self.sum    += value;
        self.last   = value;
        self.last_tick = tick;
        if value < self.min { self.min = value; }
        if value > self.max { self.max = value; }
    }

    /// Average value across all recorded samples.
    pub fn avg(&self) -> u64 {
        if self.count == 0 { 0 } else { self.sum / self.count }
    }
}

// ── Benchmark Report ──────────────────────────────────────────────────────────

/// Structured report compared against ARCHITECTURE.md benchmark targets.
#[derive(Debug, Clone)]
pub struct BenchmarkReport {
    /// Boot time in milliseconds (target: <1500ms)
    pub boot_time_ms: u64,
    /// Input latency in microseconds (target: <2000µs = 2ms)
    pub input_latency_us: u64,
    /// RAM used at idle in megabytes (target: ~450MB)
    pub idle_ram_mb: u64,
    /// Q-Ring syscall throughput (target: high, millions/sec)
    pub syscall_throughput_k: u64,
    /// Silo spawn time in microseconds
    pub silo_spawn_us: u64,
    /// Are all targets passing?
    pub all_targets_met: bool,
    /// Human-readable pass/fail per target
    pub target_status: Vec<(&'static str, bool, String)>,
}

// ── Architecture benchmark targets (from ARCHITECTURE.md) ───────────────────

const TARGET_BOOT_MS:        u64 = 1500;   // <1.5 seconds
const TARGET_INPUT_US:       u64 = 2000;   // <2ms
const TARGET_IDLE_RAM_MB:    u64 = 500;    // ~450MB (allow 500 headroom)
const TARGET_SPAWN_US:       u64 = 5000;   // <5ms silo spawn

// ── Q-Metrics Store ───────────────────────────────────────────────────────────

/// The kernel metric store — passive observer, no callbacks.
pub struct QMetricsStore {
    /// Aggregated counters keyed by a simplified metric ID
    pub counters: BTreeMap<u32, MetricAggregate>,
    /// Rolling sample ring (last 256 samples for timeline)
    pub ring: [Option<MetricSample>; 256],
    ring_head: usize,
    /// Total samples recorded since boot
    pub total_samples: u64,
    /// Assumed tick frequency for ms/us conversion (default 1MHz = 1 tick/µs)
    pub tick_freq_khz: u64,
}

/// Stable integer ID for each MetricKind (for BTreeMap keying without Hash).
impl MetricKind {
    pub fn stable_id(self) -> u32 {
        match self {
            Self::BootTimeTotal         => 1,
            Self::BootPhaseGop          => 2,
            Self::BootPhaseInterrupts   => 3,
            Self::BootPhaseSmpReady     => 4,
            Self::BootPhaseFirstSilo    => 5,
            Self::InputLatency          => 10,
            Self::SyscallDispatchLatency => 11,
            Self::SiloSpawnLatency      => 12,
            Self::ContextSwitchTicks    => 13,
            Self::PhysFramesUsed        => 20,
            Self::KernelHeapBytes       => 21,
            Self::SiloVmBytes           => 22,
            Self::PageCacheHits         => 23,
            Self::PageCacheMisses       => 24,
            Self::QRingThroughput       => 30,
            Self::NvmeReadBps           => 31,
            Self::NvmeWriteBps          => 32,
            Self::NetEgressBps          => 33,
            Self::SiloVaporizations     => 40,
            Self::LawViolation { law }  => 40 + law as u32,
            Self::SentinelScanTicks     => 50,
            Self::SystemPowerMw         => 60,
            Self::FibersDeepSleeping    => 61,
        }
    }
}

impl QMetricsStore {
    pub fn new(tick_freq_khz: u64) -> Self {
        QMetricsStore {
            counters: BTreeMap::new(),
            ring: [None; 256],
            ring_head: 0,
            total_samples: 0,
            tick_freq_khz,
        }
    }

    /// Record a metric sample (called by any kernel module — no locking needed
    /// if running on a single core; on SMP, caller should use a per-core buffer).
    pub fn record(&mut self, kind: MetricKind, value: u64, tick: u64) {
        let id = kind.stable_id();
        self.counters.entry(id).or_insert_with(MetricAggregate::default).update(value, tick);

        // Ring buffer
        self.ring[self.ring_head] = Some(MetricSample { kind, value, tick });
        self.ring_head = (self.ring_head + 1) % 256;
        self.total_samples += 1;
    }

    /// Convenience: record with current tick (caller supplies tick).
    pub fn record_ticks(&mut self, kind: MetricKind, ticks: u64, current_tick: u64) {
        self.record(kind, ticks, current_tick);
    }

    /// Get the aggregate for a metric kind.
    pub fn get(&self, kind: MetricKind) -> Option<MetricAggregate> {
        self.counters.get(&kind.stable_id()).copied()
    }

    /// Convert kernel ticks to milliseconds.
    pub fn ticks_to_ms(&self, ticks: u64) -> u64 {
        ticks / self.tick_freq_khz
    }

    /// Convert kernel ticks to microseconds.
    pub fn ticks_to_us(&self, ticks: u64) -> u64 {
        ticks * 1000 / self.tick_freq_khz
    }

    /// Generate a benchmark report comparing against ARCHITECTURE.md targets.
    pub fn benchmark_report(&self) -> BenchmarkReport {
        let boot_ms = self.get(MetricKind::BootTimeTotal)
            .map(|a| self.ticks_to_ms(a.last)).unwrap_or(0);
        let input_us = self.get(MetricKind::InputLatency)
            .map(|a| self.ticks_to_us(a.avg())).unwrap_or(9999);
        let ram_frames = self.get(MetricKind::PhysFramesUsed)
            .map(|a| a.last).unwrap_or(0);
        let idle_ram_mb = (ram_frames * 4096) / (1024 * 1024); // 4KiB pages → MB
        let spawn_us = self.get(MetricKind::SiloSpawnLatency)
            .map(|a| self.ticks_to_us(a.avg())).unwrap_or(0);
        let syscall_k = self.get(MetricKind::QRingThroughput)
            .map(|a| a.last / 1000).unwrap_or(0);

        let t_boot  = boot_ms   <= TARGET_BOOT_MS;
        let t_input = input_us  <= TARGET_INPUT_US;
        let t_ram   = idle_ram_mb <= TARGET_IDLE_RAM_MB;
        let t_spawn = spawn_us  <= TARGET_SPAWN_US;

        let all = t_boot && t_input && t_ram && t_spawn;

        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Q-Metrics Benchmark Report         ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Boot Time:    {:>6}ms  (target <{}ms) {} ║",
            boot_ms, TARGET_BOOT_MS, if t_boot {"✅"} else {"❌"});
        crate::serial_println!("║ Input Latency:{:>6}µs  (target <{}µs) {} ║",
            input_us, TARGET_INPUT_US, if t_input {"✅"} else {"❌"});
        crate::serial_println!("║ Idle RAM:     {:>6}MB  (target <{}MB) {} ║",
            idle_ram_mb, TARGET_IDLE_RAM_MB, if t_ram {"✅"} else {"❌"});
        crate::serial_println!("║ Silo Spawn:   {:>6}µs  (target <{}µs) {} ║",
            spawn_us, TARGET_SPAWN_US, if t_spawn {"✅"} else {"❌"});
        crate::serial_println!("╚══════════════════════════════════════╝");

        BenchmarkReport {
            boot_time_ms:     boot_ms,
            input_latency_us: input_us,
            idle_ram_mb,
            syscall_throughput_k: syscall_k,
            silo_spawn_us:    spawn_us,
            all_targets_met:  all,
            target_status: alloc::vec![
                ("Cold Boot <1.5s", t_boot,  alloc::format!("{}ms", boot_ms)),
                ("Input <2ms",      t_input, alloc::format!("{}µs", input_us)),
                ("Idle RAM ~450MB", t_ram,   alloc::format!("{}MB", idle_ram_mb)),
                ("Silo Spawn <5ms", t_spawn, alloc::format!("{}µs", spawn_us)),
            ],
        }
    }
}
