//! # Q-Ring Syscall Dispatcher Hardening (Phase 52)
//!
//! The Q-Ring is the primary IPC/syscall transport between user Silos and
//! the kernel. Before Phase 52, the dispatcher read raw slot indices from
//! user-supplied ring descriptors with no bounds checking, creating two
//! attack surfaces:
//!
//! 1. **Slot-index overflow:** A malicious Silo could write a head/tail index
//!    larger than the ring depth, causing an out-of-bounds read in the kernel.
//! 2. **Syscall ID forgery:** A Silo could place an unrecognized or privileged
//!    `SyscallId` in the ring and trigger undefined behavior in the dispatcher.
//!
//! ## Hardening approach
//!
//! All ring indices are masked with `(RING_DEPTH - 1)` before use.
//! All `SyscallId` values are validated against an allowlist before dispatch.
//! Violations are counted and reported to the Sentinel.
//!
//! ## Q-Manifest Law 1: Zero-Ambient Authority
//! A Silo that provides an invalid or privileged SyscallId receives a
//! `SyscallError::Forbidden` — it never executes the corresponding handler.

/// Maximum Q-Ring depth per channel.
pub const QRING_MAX_DEPTH: u64 = 4096;

/// Result of validating a Q-Ring slot index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotValidation {
    /// The index is safe to use.
    Ok(u64),
    /// The index was out of range — masked to the safe value.
    Masked(u64),
}

/// Validate and sanitize a Q-Ring slot index.
///
/// **Architecture Guardian (Phase 52):** This is the single choke-point
/// for all ring index arithmetic in the kernel. Every place that reads
/// a head or tail from a user-supplied ring descriptor must call this.
///
/// # Arguments
/// * `raw_index` — the user-supplied head or tail value
/// * `ring_depth` — the negotiated ring depth for this channel (1..=QRING_MAX_DEPTH)
///
/// # Returns
/// `SlotValidation::Ok(n)` if `raw_index < ring_depth`.
/// `SlotValidation::Masked(n)` if `raw_index >= ring_depth` — the value
/// is masked to  `raw_index & (ring_depth - 1)`. The caller MUST log the
/// masking event for the Sentinel.
#[inline]
pub fn validate_slot_index(raw_index: u64, ring_depth: u64) -> SlotValidation {
    debug_assert!(ring_depth > 0 && ring_depth <= QRING_MAX_DEPTH);
    // ring_depth is always a power of two (enforced at ring creation).
    let mask = ring_depth.saturating_sub(1);
    let safe = raw_index & mask;
    if raw_index == safe {
        SlotValidation::Ok(safe)
    } else {
        SlotValidation::Masked(safe)
    }
}

/// Validate that a ring depth is a power of two within bounds.
///
/// Called at ring creation time. Rings with non-power-of-two depths
/// cannot use the fast `& mask` index sanitization.
#[inline]
pub fn validate_ring_depth(depth: u64) -> Result<u64, &'static str> {
    if depth == 0 || depth > QRING_MAX_DEPTH {
        return Err("QRing depth out of range (1–4096)");
    }
    if depth & (depth - 1) != 0 {
        return Err("QRing depth must be a power of two");
    }
    Ok(depth)
}

/// Syscall IDs that are legal to place in user-facing Q-Ring slots.
///
/// **Phase 52 allowlist.** Privileged syscalls (those that cross the
/// kernel/capability boundary directly) are NOT in this set — they require
/// a CapToken presented in a dedicated register, not in the ring.
///
/// Any SyscallId not in this table returns `SyscallError::Forbidden`.
const USER_RING_ALLOWED_SYSCALLS: &[u64] = &[
    0,   // Yield
    1,   // Exit
    2,   // SpawnFiber
    10,  // PrismOpen
    11,  // PrismRead
    12,  // PrismWrite   ← CoW Ghost-Write (Phase 53)
    13,  // PrismClose
    14,  // PrismQuery
    20,  // IpcSend
    21,  // IpcRecv
    22,  // IpcCreate
    23,  // QRingSendBatch
    24,  // QRingRecvBatch
    30,  // MapShared
    31,  // UnmapShared
    50,  // GetTime
    51,  // Sleep
    52,  // GetSiloId
    60,  // AetherRegister
    61,  // AetherSubmit
    70,  // NetConnect
    71,  // NetSend
    72,  // NetRecv
    80,  // SentinelHeartbeat
    90,  // SynapseSubmit
    91,  // SynapseConfirm
    110, // BlkRead
    111, // BlkWrite
    112, // BlkFlush
    302, // CoWFork (Phase 53)
];

/// Privileged syscalls — NEVER allowed inside a Q-Ring slot.
/// Must be called via direct SYSCALL/SYSRET only.
const PRIVILEGED_SYSCALLS: &[u64] = &[
    40,  // RequestCap
    41,  // DelegateCap
    42,  // RevokeCap
    100, // Win32Trap
    130, // TelemetrySnapshot
    150, // FsckRun
    160, // AuditLog
    170, // SecureBootVerify
    180, // IommuAssign
    181, // IommuMap
    300, // MapCapPage
];

/// Validate a `SyscallId` value from a user Q-Ring slot.
///
/// Returns `Ok(syscall_id)` if the syscall is safe to dispatch from the ring.
/// Returns `Err("forbidden")` if privileged or unknown.
/// Returns `Err("privileged")` specifically for calls that require direct SYSCALL.
pub fn validate_ring_syscall(syscall_id: u64) -> Result<u64, &'static str> {
    if PRIVILEGED_SYSCALLS.contains(&syscall_id) {
        crate::serial_println!(
            "[SYSCALL HARDENING] Privileged syscall {} attempted via Q-Ring — blocked.",
            syscall_id
        );
        return Err("privileged: use direct SYSCALL for this operation");
    }
    if USER_RING_ALLOWED_SYSCALLS.contains(&syscall_id) {
        return Ok(syscall_id);
    }
    crate::serial_println!(
        "[SYSCALL HARDENING] Unknown syscall {} in Q-Ring slot — blocked.",
        syscall_id
    );
    Err("forbidden: unknown syscall ID")
}

/// Harden a Q-Ring batch before dispatching it to the kernel.
///
/// Validates all slot indices and syscall IDs in the batch.
/// Returns the count of valid entries and the count of blocked entries.
///
/// ## Architecture Guardian Note
/// This function is the ONLY entry point for Q-Ring batch processing.
/// The existing `handle_qring_send_batch` in `syscall/mod.rs` must be
/// updated to call this before dispatching any slot.
pub fn harden_qring_batch(
    indices: &[u64],
    syscall_ids: &[u64],
    ring_depth: u64,
) -> (usize, usize) {
    let mut valid = 0usize;
    let mut blocked = 0usize;

    for (&raw_idx, &syscall_id) in indices.iter().zip(syscall_ids.iter()) {
        let idx_result = validate_slot_index(raw_idx, ring_depth);
        match idx_result {
            SlotValidation::Masked(_) => {
                crate::serial_println!(
                    "[SYSCALL HARDENING] Q-Ring index overflow: raw={}, depth={} — masked.",
                    raw_idx, ring_depth
                );
                blocked += 1;
                continue;
            }
            SlotValidation::Ok(_) => {}
        }

        match validate_ring_syscall(syscall_id) {
            Ok(_) => valid += 1,
            Err(_) => blocked += 1,
        }
    }

    (valid, blocked)
}

/// Fault counters for the Sentinel.
#[derive(Debug, Default, Clone)]
pub struct SyscallHardeningStats {
    /// Total Q-Ring slot index overflow attempts.
    pub index_overflows: u64,
    /// Total privileged syscall attempts via Q-Ring.
    pub privileged_attempts: u64,
    /// Total unknown syscall ID attempts.
    pub unknown_syscalls: u64,
}
