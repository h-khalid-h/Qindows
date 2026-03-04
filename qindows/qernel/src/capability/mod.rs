//! # Capability Token System
//!
//! The "No-Ambient Authority" security model.
//! Every resource access in Qindows requires a cryptographic Capability Token.
//! Apps launch with zero permissions — they must be explicitly granted access.

use bitflags::bitflags;

/// Unique identifier for a capability token.
pub type CapId = u64;

bitflags! {
    /// Permission flags for capability tokens.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Permissions: u32 {
        /// Read access to the target resource
        const READ       = 0b0000_0001;
        /// Write access to the target resource
        const WRITE      = 0b0000_0010;
        /// Execute permission (for code objects)
        const EXECUTE    = 0b0000_0100;
        /// Network send capability
        const NET_SEND   = 0b0000_1000;
        /// Network receive capability
        const NET_RECV   = 0b0001_0000;
        /// Graphics/display access (Aether)
        const GRAPHICS   = 0b0010_0000;
        /// Hardware device access
        const DEVICE     = 0b0100_0000;
        /// Create child silos
        const SPAWN      = 0b1000_0000;
        /// Access to the Prism object graph
        const PRISM      = 0b0001_0000_0000;
        /// Neural interface access (Q-Synapse)
        const NEURAL     = 0b0010_0000_0000;
    }
}

/// A Capability Token — the key to everything in Qindows.
///
/// Unlike ACL-based security (Windows), capabilities are unforgeable
/// references. You can only use a resource if you hold the token.
#[derive(Debug, Clone)]
pub struct CapToken {
    /// Unique token identifier
    pub id: CapId,
    /// The Silo that owns this capability
    pub owner_silo: u64,
    /// Target Object ID this capability grants access to
    pub target_oid: u64,
    /// Granted permissions
    pub permissions: Permissions,
    /// Expiration (in scheduler ticks). 0 = permanent.
    pub expires_at: u64,
    /// Whether this token can be delegated to child silos
    pub delegatable: bool,
}

impl CapToken {
    /// Create a new capability token.
    pub fn new(
        owner_silo: u64,
        target_oid: u64,
        permissions: Permissions,
    ) -> Self {
        static NEXT_CAP: core::sync::atomic::AtomicU64 =
            core::sync::atomic::AtomicU64::new(1);

        CapToken {
            id: NEXT_CAP.fetch_add(1, core::sync::atomic::Ordering::Relaxed),
            owner_silo,
            target_oid,
            permissions,
            expires_at: 0,
            delegatable: false,
        }
    }

    /// Create a time-limited capability (Temporal Escalation).
    ///
    /// Used for temporary "admin" access:
    /// "Grant Disk-Write to this terminal for 5 minutes?"
    pub fn with_expiry(mut self, ticks: u64) -> Self {
        self.expires_at = ticks;
        self
    }

    /// Check if this capability grants the requested permission.
    pub fn has_permission(&self, required: Permissions) -> bool {
        self.permissions.contains(required)
    }

    /// Check if the token has expired.
    pub fn is_expired(&self, current_tick: u64) -> bool {
        self.expires_at > 0 && current_tick >= self.expires_at
    }

    /// Strip a specific permission from this token.
    ///
    /// The Sentinel uses this to "Live-Strip" permissions from
    /// misbehaving Silos without killing the entire process.
    pub fn revoke(&mut self, permission: Permissions) {
        self.permissions.remove(permission);
    }
}

/// Validate a capability against what the kernel expects.
///
/// This is called on every system call that accesses a resource.
pub fn validate_capability(
    token: &CapToken,
    required: Permissions,
    current_tick: u64,
) -> Result<(), CapError> {
    if token.is_expired(current_tick) {
        return Err(CapError::Expired);
    }
    if !token.has_permission(required) {
        return Err(CapError::InsufficientPermission);
    }
    Ok(())
}

/// Capability validation errors
#[derive(Debug)]
pub enum CapError {
    /// Token has expired
    Expired,
    /// Token doesn't grant the requested permission
    InsufficientPermission,
    /// Token was revoked by the Sentinel
    Revoked,
    /// Token belongs to a different Silo
    WrongOwner,
}
