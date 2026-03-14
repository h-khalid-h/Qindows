//! # PCID Allocator
//!
//! Process-Context Identifiers (PCIDs) are 12-bit tags stored in CR3[11:0].
//! When PCID is active, `MOV CR3` with bit 63 set does NOT flush the TLB —
//! instead, each Silo retains cached translations tagged by its PCID.
//!
//! ## Q-Manifest Law 6: Silo Sandbox
//! Each Q-Silo gets a unique PCID. Context switches between Silos are
//! TLB-free (NOFLUSH), eliminating the full TLB flush on every preemption.
//!
//! ## Capacity
//! x86_64 supports PCID 1–4095 (0 is reserved for the kernel identity map).
//! We track allocation via a 512-entry `AtomicU64` bitmap (512 × 64 = 4096 bits).
//!
//! ## INVPCID
//! On free, `INVPCID` is used to flush only the departing Silo's TLB entries,
//! leaving all other Silos' cached translations intact.

use core::sync::atomic::{AtomicU64, Ordering};

/// Maximum number of PCIDs (x86_64 hardware limit).
const PCID_COUNT: usize = 4096;
const BITMAP_WORDS: usize = PCID_COUNT / 64;

/// Global PCID availability bitmap.
/// Bit N = 1 means PCID N is in use.
static PCID_BITMAP: [AtomicU64; BITMAP_WORDS] = {
    // const-init trick: create array of zeros
    // AtomicU64 doesn't implement Copy but is zero-initializable
    const ZERO: AtomicU64 = AtomicU64::new(0);
    [ZERO; BITMAP_WORDS]
};

/// Reserve PCID 0 for the kernel identity-mapped address space.
/// Called once during kernel boot before any Silo is spawned.
pub fn init() {
    // Mark PCID 0 as permanently used
    PCID_BITMAP[0].fetch_or(1, Ordering::AcqRel);
}

/// Allocate the next free PCID (1–4095).
///
/// Returns `None` if all PCIDs are exhausted (extremely rare:
/// would require >4094 live Silos simultaneously).
///
/// # Architecture Guardian Note
/// This is the ONLY place a PCID is ever assigned. All callers
/// must go through this function — never hard-code a PCID.
pub fn alloc() -> Option<u16> {
    for (word_idx, word) in PCID_BITMAP.iter().enumerate() {
        let current = word.load(Ordering::Acquire);
        if current == u64::MAX {
            continue; // All 64 bits in this word are taken
        }

        // Find first zero bit
        let bit_idx = current.trailing_ones() as usize;
        let mask = 1u64 << bit_idx;

        // Attempt atomic claim
        let prev = word.fetch_or(mask, Ordering::AcqRel);
        if prev & mask == 0 {
            // We successfully claimed it
            let pcid = (word_idx * 64 + bit_idx) as u16;
            if pcid == 0 { continue; } // Skip kernel PCID 0
            return Some(pcid);
        }
        // Another core claimed the same bit — retry from this word
    }
    None
}

/// Release a PCID and flush its TLB entries via INVPCID.
///
/// # Safety
/// Caller must ensure the Silo using this PCID is no longer executing
/// on any CPU core (guaranteed by the scheduler's vaporize path).
pub fn free(pcid: u16) {
    if pcid == 0 { return; } // Never free the kernel PCID

    let idx = pcid as usize;
    let word_idx = idx / 64;
    let bit_idx = idx % 64;
    PCID_BITMAP[word_idx].fetch_and(!(1u64 << bit_idx), Ordering::AcqRel);

    // Flush all TLB entries tagged with this PCID using INVPCID type 1
    // (individual-address INVPCID for a single PCID all-context flush)
    flush_pcid(pcid);
}

/// Flush all TLB entries tagged with `pcid` using the INVPCID instruction.
///
/// INVPCID descriptor format (128-bit, 16 bytes):
/// - Bits [63:0]  = PCID (u64, only low 12 bits significant)
/// - Bits [127:64] = linear address (ignored for type 1 — flush all)
///
/// Type 1 = "Individual-address INVPCID" for a specific PCID.
/// Type 3 = "All-context INVPCID" (flushes everything, used at boot).
pub fn flush_pcid(pcid: u16) {
    // Build the 128-bit INVPCID descriptor on the stack
    let descriptor: [u64; 2] = [pcid as u64, 0];
    unsafe {
        core::arch::asm!(
            "invpcid {0}, [{1}]",
            in(reg) 1u64,          // type = 1 (single PCID, all addresses)
            in(reg) descriptor.as_ptr(),
            options(nostack, preserves_flags)
        );
    }
}

/// Flush ALL TLB entries for ALL PCIDs (used during kernel panic / full reset).
pub fn flush_all() {
    let descriptor: [u64; 2] = [0u64, 0u64];
    unsafe {
        core::arch::asm!(
            "invpcid {0}, [{1}]",
            in(reg) 3u64,          // type = 3 (all-context flush)
            in(reg) descriptor.as_ptr(),
            options(nostack, preserves_flags)
        );
    }
}

/// Count of currently allocated PCIDs (for telemetry).
pub fn allocated_count() -> u32 {
    PCID_BITMAP.iter().map(|w| w.load(Ordering::Relaxed).count_ones()).sum()
}
