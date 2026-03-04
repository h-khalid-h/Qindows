//! # SMP (Symmetric Multi-Processing) Bootstrap
//!
//! Boots Application Processors (APs) on multi-core systems.
//! The BSP (Bootstrap Processor) sends INIT-SIPI-SIPI sequences
//! to each AP discovered via ACPI MADT.

use core::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use alloc::vec::Vec;

/// Number of APs that have successfully booted.
static AP_COUNT: AtomicU32 = AtomicU32::new(0);

/// Flag: APs should start executing.
static AP_READY: AtomicBool = AtomicBool::new(false);

/// Per-core boot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreState {
    /// Not yet started
    Offline,
    /// INIT-SIPI sent, waiting for AP to respond
    Starting,
    /// AP is running but hasn't reached idle loop
    Booting,
    /// AP is idle and ready for work
    Online,
    /// AP has been parked (power saving)
    Parked,
    /// AP has hit an error
    Faulted,
}

/// Per-core data structure.
#[derive(Debug, Clone)]
pub struct CoreInfo {
    /// APIC ID (from ACPI MADT)
    pub apic_id: u8,
    /// Core state
    pub state: CoreState,
    /// Stack top address (each core gets its own stack)
    pub stack_top: u64,
    /// Is this the BSP?
    pub is_bsp: bool,
    /// Current fiber running on this core
    pub current_fiber: Option<u64>,
    /// Load metric (number of runnable fibers)
    pub load: u32,
}

/// SMP state.
pub struct SmpManager {
    /// All cores
    pub cores: Vec<CoreInfo>,
    /// BSP APIC ID
    pub bsp_apic_id: u8,
    /// Number of online cores
    pub online_count: u32,
}

/// Stack size per AP (64 KiB).
const AP_STACK_SIZE: usize = 64 * 1024;

impl SmpManager {
    /// Initialize SMP from ACPI MADT data.
    pub fn new(apic_ids: &[u8], bsp_apic_id: u8) -> Self {
        let mut cores = Vec::with_capacity(apic_ids.len());

        for &apic_id in apic_ids {
            cores.push(CoreInfo {
                apic_id,
                state: if apic_id == bsp_apic_id {
                    CoreState::Online
                } else {
                    CoreState::Offline
                },
                stack_top: 0,
                is_bsp: apic_id == bsp_apic_id,
                current_fiber: None,
                load: 0,
            });
        }

        SmpManager {
            cores,
            bsp_apic_id,
            online_count: 1, // BSP is already online
        }
    }

    /// Boot all Application Processors.
    ///
    /// For each AP:
    /// 1. Allocate a stack
    /// 2. Copy the AP trampoline code to a low-memory page
    /// 3. Send INIT IPI
    /// 4. Wait 10ms
    /// 5. Send SIPI with trampoline page address
    /// 6. Wait for AP to increment AP_COUNT
    pub unsafe fn boot_aps(&mut self, lapic_base: u64) {
        for core in &mut self.cores {
            if core.is_bsp {
                continue;
            }

            core.state = CoreState::Starting;

            // Allocate stack for this AP
            // In production: use the frame allocator
            let stack = alloc::alloc::alloc(
                alloc::alloc::Layout::from_size_align(AP_STACK_SIZE, 16).unwrap()
            );
            core.stack_top = stack as u64 + AP_STACK_SIZE as u64;

            let apic_id = core.apic_id;
            let expected_count = AP_COUNT.load(Ordering::Relaxed) + 1;

            // Send INIT IPI
            send_ipi(lapic_base, apic_id, 0x500); // INIT
            spin_wait_ms(10);

            // Send SIPI (Startup IPI) — trampoline at physical page 0x8
            send_ipi(lapic_base, apic_id, 0x608); // SIPI, vector = 0x08
            spin_wait_ms(1);

            // If AP didn't respond, send SIPI again
            if AP_COUNT.load(Ordering::Relaxed) < expected_count {
                send_ipi(lapic_base, apic_id, 0x608);
                spin_wait_ms(1);
            }

            // Check if AP booted
            if AP_COUNT.load(Ordering::Relaxed) >= expected_count {
                core.state = CoreState::Online;
                self.online_count += 1;
            } else {
                core.state = CoreState::Faulted;
            }
        }

        crate::serial_println!(
            "[OK] SMP: {}/{} cores online",
            self.online_count,
            self.cores.len()
        );
    }

    /// Get load-balanced core for scheduling a new fiber.
    pub fn least_loaded_core(&self) -> Option<u8> {
        self.cores.iter()
            .filter(|c| c.state == CoreState::Online)
            .min_by_key(|c| c.load)
            .map(|c| c.apic_id)
    }

    /// Park a core (deep sleep for power saving).
    pub fn park_core(&mut self, apic_id: u8) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.apic_id == apic_id) {
            if !core.is_bsp {
                core.state = CoreState::Parked;
                self.online_count -= 1;
            }
        }
    }

    /// Wake a parked core.
    pub fn wake_core(&mut self, apic_id: u8, lapic_base: u64) {
        if let Some(core) = self.cores.iter_mut().find(|c| c.apic_id == apic_id) {
            if core.state == CoreState::Parked {
                // Send NMI to wake the core
                unsafe { send_ipi(lapic_base, apic_id, 0x400); }
                core.state = CoreState::Online;
                self.online_count += 1;
            }
        }
    }
}

/// Send an Inter-Processor Interrupt via the Local APIC.
unsafe fn send_ipi(lapic_base: u64, target_apic_id: u8, vector_and_flags: u32) {
    let icr_high = (lapic_base + 0x310) as *mut u32;
    let icr_low = (lapic_base + 0x300) as *mut u32;

    // Set destination APIC ID
    core::ptr::write_volatile(icr_high, (target_apic_id as u32) << 24);
    // Send the IPI
    core::ptr::write_volatile(icr_low, vector_and_flags);

    // Wait for delivery
    while core::ptr::read_volatile(icr_low) & (1 << 12) != 0 {
        core::hint::spin_loop();
    }
}

/// Spin-wait for approximately N milliseconds.
fn spin_wait_ms(ms: u64) {
    // Rough estimate: ~1M iterations ≈ 1ms on modern CPUs
    for _ in 0..ms * 1_000_000 {
        core::hint::spin_loop();
    }
}

/// AP entry point — called by each AP after SIPI trampoline.
///
/// # Safety
/// This runs on the AP with its own stack. Must not access
/// BSP-owned data without synchronization.
pub extern "C" fn ap_entry() -> ! {
    AP_COUNT.fetch_add(1, Ordering::Relaxed);

    // Wait until BSP signals ready
    while !AP_READY.load(Ordering::Relaxed) {
        core::hint::spin_loop();
    }

    // Enter the scheduler's idle loop
    loop {
        // Would call: scheduler::run_next_fiber()
        unsafe { core::arch::asm!("hlt"); }
    }
}
