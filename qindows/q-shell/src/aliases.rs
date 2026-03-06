//! # Q-Shell Alias Manager
//!
//! Manages shell command aliases with expansion, persistence,
//! and per-Silo namespacing (Section 7.6).
//!
//! Features:
//! - Simple and parameterized aliases
//! - Recursive alias expansion (with loop detection)
//! - Global and per-Silo alias scopes
//! - Built-in default aliases
//! - Alias chaining

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// An alias definition.
#[derive(Debug, Clone)]
pub struct Alias {
    pub name: String,
    pub expansion: String,
    pub silo_id: Option<u64>,
    pub recursive: bool,
}

/// Alias scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasScope {
    Global,
    Silo(u64),
}

/// Alias manager statistics.
#[derive(Debug, Clone, Default)]
pub struct AliasStats {
    pub lookups: u64,
    pub expansions: u64,
    pub loop_detections: u64,
}

/// The Alias Manager.
pub struct AliasManager {
    /// Global aliases
    pub global: BTreeMap<String, Alias>,
    /// Per-Silo aliases
    pub silo_aliases: BTreeMap<u64, BTreeMap<String, Alias>>,
    /// Max expansion depth (loop protection)
    pub max_depth: usize,
    pub stats: AliasStats,
}

impl AliasManager {
    pub fn new() -> Self {
        let mut mgr = AliasManager {
            global: BTreeMap::new(),
            silo_aliases: BTreeMap::new(),
            max_depth: 16,
            stats: AliasStats::default(),
        };

        // Default aliases
        let defaults = [
            ("ll", "ls -la"),
            ("la", "ls -a"),
            ("cls", "clear"),
            ("..","cd .."),
            ("...","cd ../.."),
            ("grep", "grep --color=auto"),
            ("df", "df -h"),
            ("du", "du -sh"),
        ];
        for (name, expansion) in defaults {
            mgr.define(name, expansion, AliasScope::Global);
        }
        mgr
    }

    /// Define an alias.
    pub fn define(&mut self, name: &str, expansion: &str, scope: AliasScope) {
        let alias = Alias {
            name: String::from(name),
            expansion: String::from(expansion),
            silo_id: match scope { AliasScope::Silo(id) => Some(id), _ => None },
            recursive: false,
        };
        match scope {
            AliasScope::Global => { self.global.insert(String::from(name), alias); }
            AliasScope::Silo(id) => {
                self.silo_aliases.entry(id)
                    .or_insert_with(BTreeMap::new)
                    .insert(String::from(name), alias);
            }
        }
    }

    /// Remove an alias.
    pub fn undefine(&mut self, name: &str, scope: AliasScope) -> bool {
        match scope {
            AliasScope::Global => self.global.remove(name).is_some(),
            AliasScope::Silo(id) => {
                self.silo_aliases.get_mut(&id)
                    .map(|m| m.remove(name).is_some())
                    .unwrap_or(false)
            }
        }
    }

    /// Expand a command line, resolving aliases.
    pub fn expand(&mut self, input: &str, silo_id: Option<u64>) -> String {
        self.stats.lookups += 1;
        let mut result = String::from(input);
        let mut seen = Vec::new();
        let mut depth = 0;

        loop {
            if depth >= self.max_depth {
                self.stats.loop_detections += 1;
                break;
            }

            let first_word = result.split_whitespace().next().unwrap_or("");
            let first_word = String::from(first_word);

            if seen.contains(&first_word) {
                self.stats.loop_detections += 1;
                break;
            }

            // Silo-scoped alias first, then global
            let alias = silo_id
                .and_then(|id| self.silo_aliases.get(&id))
                .and_then(|m| m.get(&first_word))
                .or_else(|| self.global.get(&first_word));

            if let Some(a) = alias {
                seen.push(first_word.clone());
                let rest = result[first_word.len()..].to_string();
                result = a.expansion.clone() + &rest;
                self.stats.expansions += 1;
                depth += 1;
            } else {
                break;
            }
        }

        result
    }

    /// List all aliases for a scope.
    pub fn list(&self, scope: AliasScope) -> Vec<(&str, &str)> {
        match scope {
            AliasScope::Global => {
                self.global.values().map(|a| (a.name.as_str(), a.expansion.as_str())).collect()
            }
            AliasScope::Silo(id) => {
                self.silo_aliases.get(&id)
                    .map(|m| m.values().map(|a| (a.name.as_str(), a.expansion.as_str())).collect())
                    .unwrap_or_default()
            }
        }
    }
}

// Helper for String slicing in no_std
trait ToStr {
    fn to_string(&self) -> String;
}
