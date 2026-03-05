//! # Q-Admin — Temporal Capability Escalation
//!
//! Replaces legacy `sudo` with time-limited, scoped hardware token
//! grants (Section 6.1). No ambient root — privilege is always
//! temporary, audited, and requires explicit user consent.
//!
//! How it works:
//! 1. App requests elevated capability (e.g., "disk-write")
//! 2. Aether shows a system dialog: "Grant Disk-Write for 5 minutes?"
//! 3. User confirms via biometric / hardware key / Thought-Gate
//! 4. Q-Admin issues a time-limited `EscalationToken`
//! 5. Token is bound to the requesting Silo — cannot be shared
//! 6. Token auto-expires — no lingering privileges

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Capability types that can be escalated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EscalatedCap {
    /// Write to system storage
    DiskWrite,
    /// Modify system configuration (Qegistry)
    SystemConfig,
    /// Install/remove applications (Ledger)
    AppInstall,
    /// Access hardware directly (drivers)
    HardwareAccess,
    /// Modify network configuration
    NetworkConfig,
    /// Access other Silo's data (requires explicit OID)
    CrossSiloRead,
    /// Kernel module hot-swap
    KernelPatch,
    /// Sentinel rule modification
    SentinelOverride,
}

/// Escalation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalationState {
    /// Request pending user approval
    Pending,
    /// Approved and active
    Active,
    /// Expired (time limit reached)
    Expired,
    /// Denied by user
    Denied,
    /// Revoked by Sentinel
    Revoked,
}

/// Authentication method used to approve escalation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// Biometric (fingerprint, face)
    Biometric,
    /// Hardware security key (FIDO2)
    HardwareKey,
    /// Thought-Gate (BCI mental double-tap)
    ThoughtGate,
    /// PIN (fallback)
    Pin,
}

/// An escalation token — time-limited elevated privilege.
#[derive(Debug, Clone)]
pub struct EscalationToken {
    /// Token ID
    pub id: u64,
    /// Requesting Silo ID
    pub silo_id: u64,
    /// Requested capability
    pub capability: EscalatedCap,
    /// Current state
    pub state: EscalationState,
    /// When the request was made
    pub requested_at: u64,
    /// When it was approved (0 if not yet)
    pub approved_at: u64,
    /// Duration in seconds
    pub duration_secs: u64,
    /// Expiration timestamp
    pub expires_at: u64,
    /// Auth method used
    pub auth_method: Option<AuthMethod>,
    /// Reason / justification
    pub reason: String,
    /// Number of times capability was used
    pub usage_count: u64,
}

impl EscalationToken {
    /// Check if this token is currently valid.
    pub fn is_valid(&self, now: u64) -> bool {
        self.state == EscalationState::Active && now < self.expires_at
    }
}

/// Q-Admin statistics.
#[derive(Debug, Clone, Default)]
pub struct AdminStats {
    pub requests: u64,
    pub approved: u64,
    pub denied: u64,
    pub expired: u64,
    pub revoked: u64,
    pub capability_uses: u64,
}

/// The Q-Admin Manager.
pub struct QAdmin {
    /// Active tokens
    pub tokens: BTreeMap<u64, EscalationToken>,
    /// Silo → active token IDs
    pub silo_tokens: BTreeMap<u64, Vec<u64>>,
    /// Next token ID
    next_id: u64,
    /// Maximum concurrent escalations per Silo
    pub max_per_silo: usize,
    /// Default duration (seconds)
    pub default_duration: u64,
    /// Statistics
    pub stats: AdminStats,
}

impl QAdmin {
    pub fn new() -> Self {
        QAdmin {
            tokens: BTreeMap::new(),
            silo_tokens: BTreeMap::new(),
            next_id: 1,
            max_per_silo: 3,
            default_duration: 300, // 5 minutes
            stats: AdminStats::default(),
        }
    }

    /// Request an escalation (returns token ID, state = Pending).
    pub fn request(
        &mut self,
        silo_id: u64,
        capability: EscalatedCap,
        reason: &str,
        duration_secs: Option<u64>,
        now: u64,
    ) -> Result<u64, &'static str> {
        // Check concurrent limit
        let active = self.silo_tokens.get(&silo_id)
            .map(|ids| ids.iter()
                .filter(|id| self.tokens.get(id)
                    .map(|t| t.is_valid(now))
                    .unwrap_or(false))
                .count())
            .unwrap_or(0);

        if active >= self.max_per_silo {
            return Err("Too many active escalations for this Silo");
        }

        let id = self.next_id;
        self.next_id += 1;
        let dur = duration_secs.unwrap_or(self.default_duration);

        self.tokens.insert(id, EscalationToken {
            id,
            silo_id,
            capability,
            state: EscalationState::Pending,
            requested_at: now,
            approved_at: 0,
            duration_secs: dur,
            expires_at: 0,
            auth_method: None,
            reason: String::from(reason),
            usage_count: 0,
        });

        self.silo_tokens.entry(silo_id).or_insert_with(Vec::new).push(id);
        self.stats.requests += 1;
        Ok(id)
    }

    /// Approve an escalation (user confirmed via auth method).
    pub fn approve(&mut self, token_id: u64, auth: AuthMethod, now: u64) -> Result<(), &'static str> {
        let token = self.tokens.get_mut(&token_id).ok_or("Token not found")?;
        if token.state != EscalationState::Pending {
            return Err("Token not in pending state");
        }

        token.state = EscalationState::Active;
        token.approved_at = now;
        token.expires_at = now + token.duration_secs;
        token.auth_method = Some(auth);

        self.stats.approved += 1;
        Ok(())
    }

    /// Deny an escalation request.
    pub fn deny(&mut self, token_id: u64) -> Result<(), &'static str> {
        let token = self.tokens.get_mut(&token_id).ok_or("Token not found")?;
        token.state = EscalationState::Denied;
        self.stats.denied += 1;
        Ok(())
    }

    /// Use an escalated capability (increments usage, checks validity).
    pub fn use_capability(&mut self, silo_id: u64, cap: EscalatedCap, now: u64) -> bool {
        if let Some(ids) = self.silo_tokens.get(&silo_id) {
            for &id in ids {
                if let Some(token) = self.tokens.get_mut(&id) {
                    if token.capability == cap && token.is_valid(now) {
                        token.usage_count += 1;
                        self.stats.capability_uses += 1;
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Expire timed-out tokens.
    pub fn expire_tokens(&mut self, now: u64) {
        for token in self.tokens.values_mut() {
            if token.state == EscalationState::Active && now >= token.expires_at {
                token.state = EscalationState::Expired;
                self.stats.expired += 1;
            }
        }
    }

    /// Revoke all tokens for a Silo (Sentinel enforcement).
    pub fn revoke_silo(&mut self, silo_id: u64) {
        if let Some(ids) = self.silo_tokens.get(&silo_id) {
            for &id in ids {
                if let Some(token) = self.tokens.get_mut(&id) {
                    if token.state == EscalationState::Active || token.state == EscalationState::Pending {
                        token.state = EscalationState::Revoked;
                        self.stats.revoked += 1;
                    }
                }
            }
        }
    }

    /// Check if a Silo currently has a specific capability.
    pub fn has_capability(&self, silo_id: u64, cap: EscalatedCap, now: u64) -> bool {
        self.silo_tokens.get(&silo_id)
            .map(|ids| ids.iter().any(|&id|
                self.tokens.get(&id)
                    .map(|t| t.capability == cap && t.is_valid(now))
                    .unwrap_or(false)
            ))
            .unwrap_or(false)
    }
}
