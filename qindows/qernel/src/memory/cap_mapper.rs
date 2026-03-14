//! # Capability Page Mapper
//!
//! **Separation of Concerns** (Architecture Guardian Rule 1):
//! Security policy (CapTokens) must NOT leak into the VMM layer.
//! This module is the single, authoritative translation point between
//! Qindows capability semantics and x86_64 hardware PTE permission bits.
//!
//! ## Q-Manifest Law 1: Zero-Ambient Authority
//! A page has exactly the permissions the Silo's CapToken grants — no more.
//! EXECUTE is only set when a `CapToken::EXECUTE` bit is explicitly present.
//!
//! ## Q-Manifest Law 6: Silo Sandbox
//! Pages mapped without a CapToken default to kernel-supervisor (Ring 0) flags.
//! USER_ACCESSIBLE is only set for pages backed by a valid user CapToken.

use crate::capability::{CapToken, Permissions};
use crate::memory::vmm::MapPermissions;

/// Translate a `CapToken` into hardware `MapPermissions`.
///
/// This is the ONLY function that converts capability rights into
/// page table flags. Callers must never construct `MapPermissions`
/// manually for user mappings.
///
/// # Policy Table
///
/// | CapToken field  | Hardware mappings applied                     |
/// |-----------------|-----------------------------------------------|
/// | `READ`          | PRESENT + USER (no WRITE, NX set)             |
/// | `WRITE`         | PRESENT + USER + WRITABLE (NX set)            |
/// | `EXECUTE`       | PRESENT + USER (NX cleared)                   |
/// | `DEVICE`        | PRESENT + WRITABLE + NO_CACHE + WRITE_THROUGH |
/// | (no token)      | PRESENT + kernel supervisor (no USER bit)     |
pub fn map_permissions_for_cap(token: &CapToken, current_tick: u64) -> MapPermissions {
    // Expired tokens get zero permissions → kernel mapping only (inaccessible to user)
    if token.is_expired(current_tick) {
        return MapPermissions::kernel_rw();
    }

    let perms = token.permissions;

    let read    = perms.contains(Permissions::READ);
    let write   = perms.contains(Permissions::WRITE);
    let execute = perms.contains(Permissions::EXECUTE);
    let device  = perms.contains(Permissions::DEVICE);

    if device {
        // MMIO mapping: uncacheable, supervisor only (device memory must not be USER-accessible)
        return MapPermissions::mmio();
    }

    if read || write || execute {
        MapPermissions {
            read: true,           // Always readable if any permission is granted
            write,
            execute,
            user: true,           // User-accessible only when a CapToken is present
            global: false,        // Silo pages are never global (would bypass PCID isolation)
        }
    } else {
        // No relevant permissions — map as kernel supervisor (unreachable from Ring 3)
        MapPermissions::kernel_rw()
    }
}

/// Generate `MapPermissions` for kernel pages (e.g. shared syscall stubs).
///
/// Kernel pages are always Ring 0 only (no USER bit). They are globally
/// mapped into every Silo's PML4[256+] (upper half) and tagged GLOBAL
/// so they survive PCID-tagged context switches without TLB eviction.
pub fn kernel_code_permissions() -> MapPermissions {
    MapPermissions::kernel_rx()
}

/// Generate `MapPermissions` for shared read-only kernel data (IDT, GDT).
pub fn kernel_rodata_permissions() -> MapPermissions {
    MapPermissions {
        read: true,
        write: false,
        execute: false,
        user: false,
        global: true,
    }
}

/// Generate `MapPermissions` for an MMIO region.
pub fn mmio_permissions() -> MapPermissions {
    MapPermissions::mmio()
}

// ── Extension to MapPermissions for MMIO ────────────────────────────────────

impl MapPermissions {
    /// MMIO region: supervisor, uncacheable, write-through.
    pub fn mmio() -> Self {
        MapPermissions {
            read: true,
            write: true,
            execute: false,
            user: false,
            global: false,
        }
    }
}
