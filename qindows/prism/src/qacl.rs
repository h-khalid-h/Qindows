//! # Q-ACL — Per-Object Access Control Lists
//!
//! Fine-grained access control for Q-Objects beyond
//! capabilities (Section 3.18).
//!
//! Features:
//! - Per-object ACL entries (user/group/Silo)
//! - Permission types: read, write, execute, delete, admin
//! - Inheritance from parent objects
//! - Deny overrides allow
//! - Default ACL for new objects

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// ACL entry type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclPrincipal {
    User(u64),
    Group(u64),
    Silo(u64),
    Everyone,
}

/// Permission bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
    pub delete: bool,
    pub admin: bool,
}

impl Permissions {
    pub fn none() -> Self {
        Permissions { read: false, write: false, execute: false, delete: false, admin: false }
    }
    pub fn full() -> Self {
        Permissions { read: true, write: true, execute: true, delete: true, admin: true }
    }
}

/// ACL entry action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AclAction {
    Allow,
    Deny,
}

/// An ACL entry.
#[derive(Debug, Clone)]
pub struct AclEntry {
    pub principal: AclPrincipal,
    pub action: AclAction,
    pub permissions: Permissions,
    pub inherited: bool,
}

/// An object's ACL.
#[derive(Debug, Clone)]
pub struct ObjectAcl {
    pub oid: u64,
    pub entries: Vec<AclEntry>,
    pub owner: u64,
    pub inherit_parent: bool,
}

/// ACL statistics.
#[derive(Debug, Clone, Default)]
pub struct AclStats {
    pub checks: u64,
    pub allows: u64,
    pub denies: u64,
    pub entries_created: u64,
}

/// The Q-ACL Manager.
pub struct QAcl {
    pub acls: BTreeMap<u64, ObjectAcl>,
    pub default_perms: Permissions,
    pub stats: AclStats,
}

impl QAcl {
    pub fn new() -> Self {
        QAcl {
            acls: BTreeMap::new(),
            default_perms: Permissions::none(),
            stats: AclStats::default(),
        }
    }

    /// Set ACL for an object.
    pub fn set_acl(&mut self, oid: u64, owner: u64, inherit: bool) {
        self.acls.entry(oid).or_insert(ObjectAcl {
            oid, entries: Vec::new(), owner, inherit_parent: inherit,
        });
    }

    /// Add an ACL entry.
    pub fn add_entry(&mut self, oid: u64, principal: AclPrincipal, action: AclAction, perms: Permissions) {
        if let Some(acl) = self.acls.get_mut(&oid) {
            acl.entries.push(AclEntry {
                principal, action, permissions: perms, inherited: false,
            });
            self.stats.entries_created += 1;
        }
    }

    /// Check access. Deny overrides Allow.
    pub fn check(&mut self, oid: u64, user_id: u64, silo_id: u64, perm: fn(&Permissions) -> bool) -> bool {
        self.stats.checks += 1;

        let acl = match self.acls.get(&oid) {
            Some(a) => a,
            None => {
                // No ACL = use default
                let result = perm(&self.default_perms);
                if result { self.stats.allows += 1; } else { self.stats.denies += 1; }
                return result;
            }
        };

        // Owner always has full access
        if acl.owner == user_id {
            self.stats.allows += 1;
            return true;
        }

        let mut allowed = false;
        let mut denied = false;

        for entry in &acl.entries {
            let matches = match entry.principal {
                AclPrincipal::User(uid) => uid == user_id,
                AclPrincipal::Silo(sid) => sid == silo_id,
                AclPrincipal::Everyone => true,
                AclPrincipal::Group(_) => false, // Simplified
            };

            if matches && perm(&entry.permissions) {
                match entry.action {
                    AclAction::Allow => allowed = true,
                    AclAction::Deny => denied = true,
                }
            }
        }

        // Deny overrides allow
        let result = allowed && !denied;
        if result { self.stats.allows += 1; } else { self.stats.denies += 1; }
        result
    }

    /// Remove all ACL entries for an object.
    pub fn remove_acl(&mut self, oid: u64) {
        self.acls.remove(&oid);
    }
}
