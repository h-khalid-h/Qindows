//! # Capability Types
//!
//! Zero-trust capability tokens used throughout Qindows.
//! Every resource access requires a valid capability token —
//! there are no ambient permissions.

use alloc::vec::Vec;

/// A capability token — unforgeable proof of a granted permission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityToken {
    /// Unique token ID
    pub id: u64,
    /// Silo that owns this token
    pub owner: super::silo::SiloId,
    /// What this token grants
    pub capability: Capability,
    /// When this token expires (0 = never)
    pub expires_at: u64,
    /// Can this token be delegated to child Silos?
    pub delegatable: bool,
}

/// Individual capability (permission).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    /// Read from a Prism path
    FileRead(u64),
    /// Write to a Prism path
    FileWrite(u64),
    /// Create child Silos
    SpawnSilo,
    /// Open network connections
    NetConnect,
    /// Listen for incoming connections
    NetListen,
    /// Access GPU compute
    GpuCompute,
    /// Access audio output
    AudioOutput,
    /// Access camera/sensors
    SensorAccess,
    /// System administration
    Admin,
    /// IPC to a specific channel
    IpcChannel(u64),
    /// Access hardware device by class
    DeviceAccess(u16),
    /// Full (kernel-level, only for system Silos)
    Full,
}

/// Result of a capability check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapCheck {
    /// Access granted
    Granted,
    /// Token not found
    NotFound,
    /// Token expired
    Expired,
    /// Token doesn't cover this operation
    Insufficient,
    /// Token belongs to a different Silo
    WrongOwner,
}

/// A capability set — holds all tokens for a Silo.
#[derive(Debug, Clone, Default)]
pub struct CapabilitySet {
    pub tokens: Vec<CapabilityToken>,
}

impl CapabilitySet {
    pub fn new() -> Self {
        Self { tokens: Vec::new() }
    }

    /// Grant a new capability.
    pub fn grant(&mut self, token: CapabilityToken) {
        self.tokens.push(token);
    }

    /// Check if a capability is held.
    pub fn check(&self, cap: &Capability, now: u64) -> CapCheck {
        for token in &self.tokens {
            if token.expires_at > 0 && now > token.expires_at {
                continue; // expired
            }
            if &token.capability == cap || token.capability == Capability::Full {
                return CapCheck::Granted;
            }
        }
        CapCheck::NotFound
    }

    /// Revoke a capability by token ID.
    pub fn revoke(&mut self, token_id: u64) -> bool {
        let before = self.tokens.len();
        self.tokens.retain(|t| t.id != token_id);
        self.tokens.len() < before
    }
}
