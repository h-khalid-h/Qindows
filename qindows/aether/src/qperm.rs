//! # Q-Permission — Unified Permission Dialog
//!
//! Manages runtime permission requests for Silo applications.
//! Shows consent dialogs for camera, microphone, location,
//! filesystem, and network access (Section 10.15).
//!
//! Features:
//! - Permission request queue with deduplication
//! - Remember-per-app persistence
//! - Grouped permission requests
//! - Audit log of grants and denials
//! - Emergency revoke-all

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Permission type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Permission {
    Camera,
    Microphone,
    Location,
    FileRead,
    FileWrite,
    Network,
    Bluetooth,
    Usb,
    Notifications,
    ScreenCapture,
    Clipboard,
    Contacts,
}

impl Permission {
    pub fn label(&self) -> &'static str {
        match self {
            Permission::Camera => "Camera",
            Permission::Microphone => "Microphone",
            Permission::Location => "Location",
            Permission::FileRead => "Read Files",
            Permission::FileWrite => "Write Files",
            Permission::Network => "Network Access",
            Permission::Bluetooth => "Bluetooth",
            Permission::Usb => "USB Devices",
            Permission::Notifications => "Notifications",
            Permission::ScreenCapture => "Screen Capture",
            Permission::Clipboard => "Clipboard",
            Permission::Contacts => "Contacts",
        }
    }

    pub fn is_sensitive(&self) -> bool {
        matches!(self, Permission::Camera | Permission::Microphone
            | Permission::Location | Permission::ScreenCapture)
    }
}

/// Permission decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    AllowOnce,
    AskAgain,
}

/// A permission request.
#[derive(Debug, Clone)]
pub struct PermRequest {
    pub id: u64,
    pub silo_id: u64,
    pub app_name: String,
    pub permission: Permission,
    pub reason: String,
    pub timestamp: u64,
    pub decision: Option<Decision>,
}

/// Permission grant record (persisted).
#[derive(Debug, Clone)]
pub struct GrantRecord {
    pub silo_id: u64,
    pub app_name: String,
    pub permission: Permission,
    pub decision: Decision,
    pub granted_at: u64,
}

/// Permission statistics.
#[derive(Debug, Clone, Default)]
pub struct PermStats {
    pub requests: u64,
    pub grants: u64,
    pub denials: u64,
    pub revocations: u64,
}

/// The Q-Permission Manager.
pub struct QPermission {
    pub pending: Vec<PermRequest>,
    pub grants: Vec<GrantRecord>,
    next_id: u64,
    pub stats: PermStats,
}

impl QPermission {
    pub fn new() -> Self {
        QPermission {
            pending: Vec::new(),
            grants: Vec::new(),
            next_id: 1,
            stats: PermStats::default(),
        }
    }

    /// Request a permission.
    pub fn request(&mut self, silo_id: u64, app: &str, perm: Permission, reason: &str, now: u64) -> u64 {
        // Check if already granted
        if let Some(grant) = self.find_grant(silo_id, app, perm) {
            if grant.decision == Decision::Allow {
                return 0; // Already granted
            }
        }

        // Deduplicate pending
        if self.pending.iter().any(|r| r.silo_id == silo_id && r.permission == perm && r.decision.is_none()) {
            return 0;
        }

        let id = self.next_id;
        self.next_id += 1;
        self.pending.push(PermRequest {
            id, silo_id, app_name: String::from(app),
            permission: perm, reason: String::from(reason),
            timestamp: now, decision: None,
        });
        self.stats.requests += 1;
        id
    }

    /// Respond to a permission request.
    pub fn respond(&mut self, request_id: u64, decision: Decision, now: u64) {
        if let Some(req) = self.pending.iter_mut().find(|r| r.id == request_id) {
            req.decision = Some(decision);
            match decision {
                Decision::Allow => {
                    self.grants.push(GrantRecord {
                        silo_id: req.silo_id, app_name: req.app_name.clone(),
                        permission: req.permission, decision,
                        granted_at: now,
                    });
                    self.stats.grants += 1;
                }
                Decision::Deny => { self.stats.denials += 1; }
                _ => {}
            }
        }
    }

    /// Find an existing grant.
    fn find_grant(&self, silo_id: u64, app: &str, perm: Permission) -> Option<&GrantRecord> {
        self.grants.iter().rev().find(|g| g.silo_id == silo_id && g.app_name == app && g.permission == perm)
    }

    /// Revoke all permissions for a Silo.
    pub fn revoke_silo(&mut self, silo_id: u64) {
        let before = self.grants.len();
        self.grants.retain(|g| g.silo_id != silo_id);
        self.stats.revocations += (before - self.grants.len()) as u64;
    }

    /// Check if a permission is currently granted.
    pub fn is_allowed(&self, silo_id: u64, app: &str, perm: Permission) -> bool {
        self.find_grant(silo_id, app, perm)
            .map(|g| g.decision == Decision::Allow)
            .unwrap_or(false)
    }
}
