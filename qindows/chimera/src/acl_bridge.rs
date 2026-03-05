//! # Chimera ACL → Capability Translation Bridge
//!
//! Legacy Win32 applications use Access Control Lists (ACLs) with
//! DACL/SACL security descriptors. Qindows uses Capability Tokens.
//! This bridge translates between the two models seamlessly.
//!
//! Mapping rules:
//! - `FILE_READ_DATA` → `Permissions::READ`
//! - `FILE_WRITE_DATA` → `Permissions::WRITE`
//! - `PROCESS_CREATE_PROCESS` → `Permissions::SPAWN`
//! - `GENERIC_ALL` → full capability set (restricted by Sentinel)
//! - Group/User SIDs → Silo IDs

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Win32 access rights.
pub mod win32_rights {
    pub const FILE_READ_DATA: u32 = 0x0001;
    pub const FILE_WRITE_DATA: u32 = 0x0002;
    pub const FILE_APPEND_DATA: u32 = 0x0004;
    pub const FILE_READ_EA: u32 = 0x0008;
    pub const FILE_WRITE_EA: u32 = 0x0010;
    pub const FILE_EXECUTE: u32 = 0x0020;
    pub const FILE_DELETE_CHILD: u32 = 0x0040;
    pub const FILE_READ_ATTRIBUTES: u32 = 0x0080;
    pub const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
    pub const DELETE: u32 = 0x00010000;
    pub const READ_CONTROL: u32 = 0x00020000;
    pub const WRITE_DAC: u32 = 0x00040000;
    pub const WRITE_OWNER: u32 = 0x00080000;
    pub const SYNCHRONIZE: u32 = 0x00100000;
    pub const GENERIC_READ: u32 = 0x80000000;
    pub const GENERIC_WRITE: u32 = 0x40000000;
    pub const GENERIC_EXECUTE: u32 = 0x20000000;
    pub const GENERIC_ALL: u32 = 0x10000000;
}

/// Qindows capability permissions (matches qernel::capability).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QPermissions(pub u64);

impl QPermissions {
    pub const READ: u64 = 1 << 0;
    pub const WRITE: u64 = 1 << 1;
    pub const EXECUTE: u64 = 1 << 2;
    pub const SPAWN: u64 = 1 << 3;
    pub const NET: u64 = 1 << 4;
    pub const GPU: u64 = 1 << 5;
    pub const DELETE: u64 = 1 << 6;

    pub fn has(&self, perm: u64) -> bool {
        self.0 & perm != 0
    }

    pub fn union(self, other: QPermissions) -> QPermissions {
        QPermissions(self.0 | other.0)
    }
}

/// A Win32 Security Identifier (SID) — simplified.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Sid {
    /// Authority (5 = NT Authority)
    pub authority: u8,
    /// Sub-authorities (e.g., [21, 12345, 67890, 500] = Administrator)
    pub sub_authorities: Vec<u32>,
}

impl Sid {
    pub fn well_known_admin() -> Self {
        Sid { authority: 5, sub_authorities: alloc::vec![21, 0, 0, 500] }
    }
    pub fn well_known_users() -> Self {
        Sid { authority: 5, sub_authorities: alloc::vec![21, 0, 0, 545] }
    }
    pub fn everyone() -> Self {
        Sid { authority: 1, sub_authorities: alloc::vec![0] }
    }
}

/// An Access Control Entry (ACE).
#[derive(Debug, Clone)]
pub struct Ace {
    /// Allow or deny
    pub ace_type: AceType,
    /// Win32 access mask
    pub access_mask: u32,
    /// SID this ACE applies to
    pub sid: Sid,
}

/// ACE type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AceType {
    AccessAllowed,
    AccessDenied,
    SystemAudit,
}

/// A Discretionary Access Control List (DACL).
#[derive(Debug, Clone)]
pub struct Dacl {
    pub aces: Vec<Ace>,
}

/// A Security Descriptor.
#[derive(Debug, Clone)]
pub struct SecurityDescriptor {
    pub owner: Sid,
    pub group: Sid,
    pub dacl: Option<Dacl>,
}

/// Translation result.
#[derive(Debug, Clone)]
pub struct CapabilityGrant {
    /// Silo ID to grant to
    pub silo_id: u64,
    /// Translated permissions
    pub permissions: QPermissions,
    /// Object ID the capability applies to
    pub object_oid: u64,
    /// Was any access denied?
    pub denied: bool,
}

/// SID → Silo mapping.
pub struct SidMapping {
    pub sid_to_silo: BTreeMap<Sid, u64>,
    pub default_silo: u64,
}

impl SidMapping {
    pub fn new(default_silo: u64) -> Self {
        let mut mapping = SidMapping {
            sid_to_silo: BTreeMap::new(),
            default_silo,
        };
        // Map well-known SIDs
        mapping.sid_to_silo.insert(Sid::everyone(), 0); // Global scope
        mapping
    }

    pub fn map_sid(&self, sid: &Sid) -> u64 {
        self.sid_to_silo.get(sid).copied().unwrap_or(self.default_silo)
    }
}

/// The ACL → Capability Bridge.
pub struct AclBridge {
    /// SID → Silo mapping
    pub mapping: SidMapping,
    /// Translation statistics
    pub stats: AclBridgeStats,
}

/// Bridge statistics.
#[derive(Debug, Clone, Default)]
pub struct AclBridgeStats {
    pub descriptors_translated: u64,
    pub aces_processed: u64,
    pub denials_enforced: u64,
    pub capabilities_granted: u64,
}

impl AclBridge {
    pub fn new(default_silo: u64) -> Self {
        AclBridge {
            mapping: SidMapping::new(default_silo),
            stats: AclBridgeStats::default(),
        }
    }

    /// Translate a Win32 access mask to Qindows permissions.
    pub fn translate_access_mask(&self, mask: u32) -> QPermissions {
        let mut perms = 0u64;

        if mask & win32_rights::FILE_READ_DATA != 0
            || mask & win32_rights::GENERIC_READ != 0
            || mask & win32_rights::FILE_READ_ATTRIBUTES != 0
        {
            perms |= QPermissions::READ;
        }

        if mask & win32_rights::FILE_WRITE_DATA != 0
            || mask & win32_rights::GENERIC_WRITE != 0
            || mask & win32_rights::FILE_APPEND_DATA != 0
            || mask & win32_rights::FILE_WRITE_ATTRIBUTES != 0
        {
            perms |= QPermissions::WRITE;
        }

        if mask & win32_rights::FILE_EXECUTE != 0
            || mask & win32_rights::GENERIC_EXECUTE != 0
        {
            perms |= QPermissions::EXECUTE;
        }

        if mask & win32_rights::DELETE != 0
            || mask & win32_rights::FILE_DELETE_CHILD != 0
        {
            perms |= QPermissions::DELETE;
        }

        if mask & win32_rights::GENERIC_ALL != 0 {
            perms |= QPermissions::READ | QPermissions::WRITE
                | QPermissions::EXECUTE | QPermissions::DELETE;
        }

        QPermissions(perms)
    }

    /// Translate a full security descriptor into capability grants.
    pub fn translate_descriptor(
        &mut self,
        desc: &SecurityDescriptor,
        object_oid: u64,
    ) -> Vec<CapabilityGrant> {
        self.stats.descriptors_translated += 1;
        let mut grants = Vec::new();

        let dacl = match &desc.dacl {
            Some(d) => d,
            None => {
                // No DACL = full access (Win32 semantics)
                grants.push(CapabilityGrant {
                    silo_id: self.mapping.map_sid(&desc.owner),
                    permissions: QPermissions(
                        QPermissions::READ | QPermissions::WRITE | QPermissions::EXECUTE
                    ),
                    object_oid,
                    denied: false,
                });
                return grants;
            }
        };

        // Process ACEs in order (deny ACEs first in Win32)
        let mut denied_sids: Vec<(Sid, u32)> = Vec::new();

        for ace in &dacl.aces {
            self.stats.aces_processed += 1;

            match ace.ace_type {
                AceType::AccessDenied => {
                    denied_sids.push((ace.sid.clone(), ace.access_mask));
                    self.stats.denials_enforced += 1;
                }
                AceType::AccessAllowed => {
                    // Check if this SID was denied
                    let effective_mask = denied_sids.iter()
                        .filter(|(sid, _)| *sid == ace.sid)
                        .fold(ace.access_mask, |mask, (_, denied)| mask & !denied);

                    if effective_mask != 0 {
                        let perms = self.translate_access_mask(effective_mask);
                        grants.push(CapabilityGrant {
                            silo_id: self.mapping.map_sid(&ace.sid),
                            permissions: perms,
                            object_oid,
                            denied: false,
                        });
                        self.stats.capabilities_granted += 1;
                    }
                }
                AceType::SystemAudit => {
                    // Log only — no capability effect
                }
            }
        }

        grants
    }
}
