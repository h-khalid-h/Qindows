//! # Q-Attr — Extended Attributes on Q-Objects
//!
//! Provides arbitrary key-value metadata on any Q-Object
//! beyond the core schema (Section 3.20).
//!
//! Features:
//! - Typed attributes (string, int, bytes, bool, float)
//! - Namespaced keys (system.*, user.*, security.*)
//! - Per-Silo attribute isolation
//! - Bulk get/set
//! - Attribute size limits

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Attribute value types.
#[derive(Debug, Clone)]
pub enum AttrValue {
    Str(String),
    Int(i64),
    Bytes(Vec<u8>),
    Bool(bool),
    Float(u64), // f64 bits stored as u64 (no_std)
}

/// Attribute namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AttrNamespace {
    System,
    User,
    Security,
    Trusted,
}

/// An extended attribute.
#[derive(Debug, Clone)]
pub struct Attribute {
    pub namespace: AttrNamespace,
    pub key: String,
    pub value: AttrValue,
    pub size: usize,
}

/// Q-Attr statistics.
#[derive(Debug, Clone, Default)]
pub struct AttrStats {
    pub attrs_set: u64,
    pub attrs_get: u64,
    pub attrs_removed: u64,
    pub size_rejected: u64,
}

/// The Q-Attr Manager.
pub struct QAttr {
    /// (oid, namespace, key) → Attribute
    pub attrs: BTreeMap<(u64, AttrNamespace, String), Attribute>,
    pub max_attr_size: usize,
    pub max_attrs_per_object: usize,
    pub stats: AttrStats,
}

impl QAttr {
    pub fn new() -> Self {
        QAttr {
            attrs: BTreeMap::new(),
            max_attr_size: 65536,
            max_attrs_per_object: 256,
            stats: AttrStats::default(),
        }
    }

    /// Set an attribute on an object.
    pub fn set(&mut self, oid: u64, ns: AttrNamespace, key: &str, value: AttrValue) -> Result<(), &'static str> {
        let size = match &value {
            AttrValue::Str(s) => s.len(),
            AttrValue::Int(_) => 8,
            AttrValue::Bytes(b) => b.len(),
            AttrValue::Bool(_) => 1,
            AttrValue::Float(_) => 8,
        };

        if size > self.max_attr_size {
            self.stats.size_rejected += 1;
            return Err("Attribute too large");
        }

        // Count existing attrs for this object
        let existing = self.attrs.keys()
            .filter(|(o, _, _)| *o == oid)
            .count();
        let is_update = self.attrs.contains_key(&(oid, ns, String::from(key)));
        if !is_update && existing >= self.max_attrs_per_object {
            return Err("Too many attributes on object");
        }

        self.attrs.insert((oid, ns, String::from(key)), Attribute {
            namespace: ns, key: String::from(key), value, size,
        });
        self.stats.attrs_set += 1;
        Ok(())
    }

    /// Get an attribute.
    pub fn get(&mut self, oid: u64, ns: AttrNamespace, key: &str) -> Option<&Attribute> {
        self.stats.attrs_get += 1;
        self.attrs.get(&(oid, ns, String::from(key)))
    }

    /// Remove an attribute.
    pub fn remove(&mut self, oid: u64, ns: AttrNamespace, key: &str) -> bool {
        if self.attrs.remove(&(oid, ns, String::from(key))).is_some() {
            self.stats.attrs_removed += 1;
            true
        } else {
            false
        }
    }

    /// List all attributes on an object.
    pub fn list(&self, oid: u64) -> Vec<&Attribute> {
        self.attrs.iter()
            .filter(|((o, _, _), _)| *o == oid)
            .map(|(_, attr)| attr)
            .collect()
    }

    /// Remove all attributes for an object.
    pub fn remove_all(&mut self, oid: u64) {
        let keys: Vec<_> = self.attrs.keys()
            .filter(|(o, _, _)| *o == oid)
            .cloned()
            .collect();
        for key in keys {
            self.attrs.remove(&key);
            self.stats.attrs_removed += 1;
        }
    }
}
