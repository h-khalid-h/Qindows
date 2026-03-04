//! # TSC & HPET Timer Calibration
//!
//! Precision time measurement using the CPU's Time Stamp Counter (TSC)
//! calibrated against the High Precision Event Timer (HPET).
//! Provides nanosecond-resolution timing for the scheduler,
//! profiler, and all timed operations.

/// HPET MMIO register offsets.
const HPET_GENERAL_CAP: u64 = 0x000;
const HPET_GENERAL_CONFIG: u64 = 0x010;
const HPET_MAIN_COUNTER: u64 = 0x0F0;
const HPET_TIMER0_CONFIG: u64 = 0x100;
const HPET_TIMER0_COMPARE: u64 = 0x108;

/// TSC frequency (Hz), filled in during calibration.
static mut TSC_FREQ_HZ: u64 = 0;

/// HPET period (femtoseconds per tick), from capability register.
static mut HPET_PERIOD_FS: u64 = 0;

/// HPET base address (from ACPI).
static mut HPET_BASE: u64 = 0;

/// Read the CPU Time Stamp Counter.
#[inline(always)]
pub fn rdtsc() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!("rdtsc", out("eax") lo, out("edx") hi, options(nomem, nostack));
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Serializing RDTSC — guarantees all prior instructions have completed.
#[inline(always)]
pub fn rdtscp() -> u64 {
    let lo: u32;
    let hi: u32;
    unsafe {
        core::arch::asm!(
            "rdtscp",
            out("eax") lo, out("edx") hi, out("ecx") _,
            options(nomem, nostack)
        );
    }
    ((hi as u64) << 32) | (lo as u64)
}

/// Read an HPET register.
unsafe fn hpet_read(offset: u64) -> u64 {
    let addr = HPET_BASE + offset;
    core::ptr::read_volatile(addr as *const u64)
}

/// Write an HPET register.
unsafe fn hpet_write(offset: u64, value: u64) {
    let addr = HPET_BASE + offset;
    core::ptr::write_volatile(addr as *mut u64, value);
}

/// Initialize the HPET.
pub fn init_hpet(base_addr: u64) {
    unsafe {
        HPET_BASE = base_addr;

        // Read capabilities — bits 63:32 contain the period in femtoseconds
        let cap = hpet_read(HPET_GENERAL_CAP);
        HPET_PERIOD_FS = cap >> 32;

        // Enable the HPET main counter
        let config = hpet_read(HPET_GENERAL_CONFIG);
        hpet_write(HPET_GENERAL_CONFIG, config | 1); // Set ENABLE_CNF

        crate::serial_println!(
            "[OK] HPET initialized: period = {} fs/tick",
            HPET_PERIOD_FS
        );
    }
}

/// Read the HPET main counter value.
pub fn hpet_counter() -> u64 {
    unsafe { hpet_read(HPET_MAIN_COUNTER) }
}

/// Convert HPET ticks to nanoseconds.
pub fn hpet_ticks_to_ns(ticks: u64) -> u64 {
    unsafe {
        if HPET_PERIOD_FS == 0 { return 0; }
        // period is in femtoseconds, 1 ns = 1,000,000 fs
        ticks * HPET_PERIOD_FS / 1_000_000
    }
}

/// Calibrate the TSC against the HPET.
///
/// Measures how many TSC cycles occur during a known HPET interval
/// to determine the TSC frequency.
pub fn calibrate_tsc() {
    unsafe {
        if HPET_BASE == 0 || HPET_PERIOD_FS == 0 {
            // Fall back to a rough estimate (assume ~3 GHz)
            TSC_FREQ_HZ = 3_000_000_000;
            crate::serial_println!("[WARN] HPET not available — TSC freq estimated at 3 GHz");
            return;
        }

        // Measure TSC cycles over ~10ms of HPET time
        let target_ns: u64 = 10_000_000; // 10ms
        let target_hpet_ticks = target_ns * 1_000_000 / HPET_PERIOD_FS;

        let hpet_start = hpet_read(HPET_MAIN_COUNTER);
        let tsc_start = rdtsc();

        // Spin until enough HPET ticks have elapsed
        loop {
            let hpet_now = hpet_read(HPET_MAIN_COUNTER);
            if hpet_now.wrapping_sub(hpet_start) >= target_hpet_ticks {
                break;
            }
            core::hint::spin_loop();
        }

        let tsc_end = rdtsc();
        let hpet_end = hpet_read(HPET_MAIN_COUNTER);

        let tsc_elapsed = tsc_end - tsc_start;
        let hpet_elapsed = hpet_end.wrapping_sub(hpet_start);
        let elapsed_ns = hpet_ticks_to_ns(hpet_elapsed);

        if elapsed_ns > 0 {
            TSC_FREQ_HZ = tsc_elapsed * 1_000_000_000 / elapsed_ns;
        }

        crate::serial_println!(
            "[OK] TSC calibrated: {} MHz ({} Hz)",
            TSC_FREQ_HZ / 1_000_000,
            TSC_FREQ_HZ
        );
    }
}

/// Convert TSC cycles to nanoseconds.
pub fn tsc_to_ns(cycles: u64) -> u64 {
    unsafe {
        if TSC_FREQ_HZ == 0 { return 0; }
        cycles * 1_000_000_000 / TSC_FREQ_HZ
    }
}

/// Convert nanoseconds to TSC cycles.
pub fn ns_to_tsc(ns: u64) -> u64 {
    unsafe {
        if TSC_FREQ_HZ == 0 { return 0; }
        ns * TSC_FREQ_HZ / 1_000_000_000
    }
}

/// Get the current time in nanoseconds since boot.
pub fn now_ns() -> u64 {
    tsc_to_ns(rdtsc())
}

/// Get the TSC frequency in Hz.
pub fn tsc_frequency() -> u64 {
    unsafe { TSC_FREQ_HZ }
}

/// Sleep for approximately N nanoseconds (busy-wait).
pub fn busy_wait_ns(ns: u64) {
    let target = rdtsc() + ns_to_tsc(ns);
    while rdtsc() < target {
        core::hint::spin_loop();
    }
}

/// Sleep for approximately N microseconds (busy-wait).
pub fn busy_wait_us(us: u64) {
    busy_wait_ns(us * 1000);
}

/// Sleep for approximately N milliseconds (busy-wait).
pub fn busy_wait_ms(ms: u64) {
    busy_wait_ns(ms * 1_000_000);
}
