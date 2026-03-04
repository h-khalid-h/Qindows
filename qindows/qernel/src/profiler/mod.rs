//! # Kernel Profiler
//!
//! Performance monitoring for all Qernel subsystems.
//! Tracks cycle counts, function call frequency, memory pressure,
//! and generates flame-graph-compatible output.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// A profiling sample point.
#[derive(Debug, Clone)]
pub struct ProfileSample {
    /// Function/region name
    pub name: &'static str,
    /// Subsystem category
    pub subsystem: &'static str,
    /// Start timestamp (TSC cycles)
    pub start_tsc: u64,
    /// End timestamp (TSC cycles)
    pub end_tsc: u64,
    /// Core ID where this ran
    pub core_id: u8,
}

impl ProfileSample {
    /// Duration in CPU cycles.
    pub fn cycles(&self) -> u64 {
        self.end_tsc.saturating_sub(self.start_tsc)
    }
}

/// Aggregated statistics for a profiled region.
#[derive(Debug, Clone, Default)]
pub struct RegionStats {
    /// Number of times this region was entered
    pub count: u64,
    /// Total cycles spent in this region
    pub total_cycles: u64,
    /// Minimum cycles for a single invocation
    pub min_cycles: u64,
    /// Maximum cycles for a single invocation
    pub max_cycles: u64,
}

impl RegionStats {
    /// Average cycles per invocation.
    pub fn avg_cycles(&self) -> u64 {
        if self.count == 0 { 0 } else { self.total_cycles / self.count }
    }

    /// Update stats with a new sample.
    pub fn record(&mut self, cycles: u64) {
        self.count += 1;
        self.total_cycles += cycles;
        if self.count == 1 || cycles < self.min_cycles {
            self.min_cycles = cycles;
        }
        if cycles > self.max_cycles {
            self.max_cycles = cycles;
        }
    }
}

/// Global profiler state.
pub struct Profiler {
    /// Per-region statistics
    pub regions: BTreeMap<&'static str, RegionStats>,
    /// Raw sample buffer
    pub samples: Vec<ProfileSample>,
    /// Is profiling enabled?
    pub enabled: bool,
    /// Maximum samples to store
    pub max_samples: usize,
}

static PROFILER: Mutex<Option<Profiler>> = Mutex::new(None);

impl Profiler {
    pub fn new() -> Self {
        Profiler {
            regions: BTreeMap::new(),
            samples: Vec::new(),
            enabled: false,
            max_samples: 10_000,
        }
    }

    /// Begin a profiled region.
    pub fn begin(&self) -> u64 {
        rdtsc()
    }

    /// End a profiled region and record it.
    pub fn end(&mut self, name: &'static str, subsystem: &'static str, start: u64, core_id: u8) {
        let end = rdtsc();
        let cycles = end.saturating_sub(start);

        // Update region stats
        self.regions.entry(name).or_default().record(cycles);

        // Store raw sample if enabled
        if self.enabled && self.samples.len() < self.max_samples {
            self.samples.push(ProfileSample {
                name,
                subsystem,
                start_tsc: start,
                end_tsc: end,
                core_id,
            });
        }
    }

    /// Get the top N hottest regions by total cycles.
    pub fn hotspots(&self, n: usize) -> Vec<(&'static str, &RegionStats)> {
        let mut sorted: Vec<_> = self.regions.iter().collect();
        sorted.sort_by(|a, b| b.1.total_cycles.cmp(&a.1.total_cycles));
        sorted.truncate(n);
        sorted.into_iter().map(|(k, v)| (*k, v)).collect()
    }

    /// Get total profiled cycles.
    pub fn total_cycles(&self) -> u64 {
        self.regions.values().map(|r| r.total_cycles).sum()
    }

    /// Reset all profiling data.
    pub fn reset(&mut self) {
        self.regions.clear();
        self.samples.clear();
    }

    /// Generate a report.
    pub fn report(&self) -> String {
        let mut output = String::from("╔═══════════════════════════════════════════════════╗\n");
        output.push_str(                "║           QERNEL PROFILER REPORT                  ║\n");
        output.push_str(                "╚═══════════════════════════════════════════════════╝\n\n");

        let total = self.total_cycles();

        let hotspots = self.hotspots(20);
        for (name, stats) in &hotspots {
            let pct = if total > 0 {
                (stats.total_cycles as f64 / total as f64 * 100.0) as u32
            } else {
                0
            };

            let bar_len = (pct as usize).min(30);
            let bar: String = "█".repeat(bar_len);

            output.push_str(&alloc::format!(
                "{:30} {:>6} calls  {:>12} avg cy  {:>3}% {}\n",
                name,
                stats.count,
                stats.avg_cycles(),
                pct,
                bar
            ));
        }

        output.push_str(&alloc::format!(
            "\nTotal regions: {}  Total cycles: {}\n",
            self.regions.len(),
            total
        ));

        output
    }
}

/// Initialize the global profiler.
pub fn init() {
    *PROFILER.lock() = Some(Profiler::new());
}

/// Start profiling.
pub fn enable() {
    if let Some(ref mut p) = *PROFILER.lock() {
        p.enabled = true;
    }
}

/// Stop profiling.
pub fn disable() {
    if let Some(ref mut p) = *PROFILER.lock() {
        p.enabled = false;
    }
}

/// Begin a profiled region (returns start TSC).
pub fn begin() -> u64 {
    rdtsc()
}

/// End a profiled region.
pub fn end(name: &'static str, subsystem: &'static str, start: u64, core_id: u8) {
    if let Some(ref mut p) = *PROFILER.lock() {
        p.end(name, subsystem, start, core_id);
    }
}

/// Generate a report.
pub fn report() -> Option<String> {
    PROFILER.lock().as_ref().map(|p| p.report())
}

/// Read the CPU Time Stamp Counter.
fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// RAII profiling guard — automatically records when dropped.
pub struct ProfileGuard {
    name: &'static str,
    subsystem: &'static str,
    start: u64,
    core_id: u8,
}

impl ProfileGuard {
    pub fn new(name: &'static str, subsystem: &'static str, core_id: u8) -> Self {
        ProfileGuard {
            name,
            subsystem,
            start: rdtsc(),
            core_id,
        }
    }
}

impl Drop for ProfileGuard {
    fn drop(&mut self) {
        end(self.name, self.subsystem, self.start, self.core_id);
    }
}
