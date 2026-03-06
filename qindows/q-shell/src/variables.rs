//! # Q-Shell Variables — Environment Variable Manager
//!
//! Manages shell environment variables with per-Silo
//! isolation, variable scoping, and export semantics
//! for the Q-Shell environment (Section 6.2).
//!
//! Features:
//! - Per-Silo variable namespaces
//! - Scoped variables (local to shell session)
//! - System-wide read-only variables
//! - Variable expansion ($VAR, ${VAR:-default})
//! - Export tracking (which vars propagate to child procs)

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Variable scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VarScope {
    /// Session-local (not inherited by child processes)
    Local,
    /// Exported (inherited by child processes)
    Export,
    /// System-defined read-only
    System,
}

/// A shell variable.
#[derive(Debug, Clone)]
pub struct ShellVar {
    pub name: String,
    pub value: String,
    pub scope: VarScope,
    pub readonly: bool,
    pub set_at: u64,
}

/// Variable statistics.
#[derive(Debug, Clone, Default)]
pub struct VarStats {
    pub vars_set: u64,
    pub vars_read: u64,
    pub vars_exported: u64,
    pub expansions: u64,
}

/// The Variable Manager.
pub struct VarManager {
    /// Global system variables (shared, read-only)
    pub system_vars: BTreeMap<String, ShellVar>,
    /// Per-Silo variable stores
    pub silo_vars: BTreeMap<u64, BTreeMap<String, ShellVar>>,
    pub stats: VarStats,
}

impl VarManager {
    pub fn new() -> Self {
        let mut mgr = VarManager {
            system_vars: BTreeMap::new(),
            silo_vars: BTreeMap::new(),
            stats: VarStats::default(),
        };
        // Bootstrap system variables
        mgr.set_system("QINDOWS_VERSION", "1.0.0", 0);
        mgr.set_system("SHELL", "/q-shell", 0);
        mgr.set_system("PATH", "/bin:/sbin:/usr/bin", 0);
        mgr.set_system("HOME", "/home", 0);
        mgr
    }

    /// Set a system-wide read-only variable.
    fn set_system(&mut self, name: &str, value: &str, now: u64) {
        self.system_vars.insert(String::from(name), ShellVar {
            name: String::from(name), value: String::from(value),
            scope: VarScope::System, readonly: true, set_at: now,
        });
    }

    /// Set a variable for a Silo.
    pub fn set(&mut self, silo_id: u64, name: &str, value: &str,
               scope: VarScope, now: u64) -> Result<(), &'static str> {
        // Cannot override system vars
        if self.system_vars.contains_key(name) {
            return Err("Cannot override system variable");
        }

        let store = self.silo_vars.entry(silo_id).or_insert_with(BTreeMap::new);

        // Check readonly
        if let Some(existing) = store.get(name) {
            if existing.readonly {
                return Err("Variable is read-only");
            }
        }

        store.insert(String::from(name), ShellVar {
            name: String::from(name), value: String::from(value),
            scope, readonly: false, set_at: now,
        });

        self.stats.vars_set += 1;
        if scope == VarScope::Export {
            self.stats.vars_exported += 1;
        }
        Ok(())
    }

    /// Get a variable (Silo-scoped, then system).
    pub fn get(&mut self, silo_id: u64, name: &str) -> Option<&str> {
        self.stats.vars_read += 1;
        // Silo-specific first
        if let Some(store) = self.silo_vars.get(&silo_id) {
            if let Some(var) = store.get(name) {
                return Some(&var.value);
            }
        }
        // System fallback
        self.system_vars.get(name).map(|v| v.value.as_str())
    }

    /// Expand variables in a string ($VAR and ${VAR:-default}).
    pub fn expand(&mut self, silo_id: u64, input: &str) -> String {
        let mut result = String::new();
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                if chars[i + 1] == '{' {
                    // ${VAR} or ${VAR:-default}
                    if let Some(end) = input[i + 2..].find('}') {
                        let expr = &input[i + 2..i + 2 + end];
                        let (name, default) = if let Some(pos) = expr.find(":-") {
                            (&expr[..pos], Some(&expr[pos + 2..]))
                        } else {
                            (expr, None)
                        };
                        let val = self.get(silo_id, name)
                            .map(String::from)
                            .or_else(|| default.map(String::from))
                            .unwrap_or_default();
                        result.push_str(&val);
                        i += 3 + end;
                        self.stats.expansions += 1;
                        continue;
                    }
                } else {
                    // $VAR — read until non-alphanumeric
                    let start = i + 1;
                    let mut end = start;
                    while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                        end += 1;
                    }
                    if end > start {
                        let name: String = chars[start..end].iter().collect();
                        let val = self.get(silo_id, &name).unwrap_or("").to_string();
                        result.push_str(&val);
                        i = end;
                        self.stats.expansions += 1;
                        continue;
                    }
                }
            }
            result.push(chars[i]);
            i += 1;
        }
        result
    }

    /// Unset a variable.
    pub fn unset(&mut self, silo_id: u64, name: &str) -> bool {
        if let Some(store) = self.silo_vars.get_mut(&silo_id) {
            if let Some(var) = store.get(name) {
                if var.readonly { return false; }
            }
            return store.remove(name).is_some();
        }
        false
    }

    /// Get exported variables for a Silo (for child process inheritance).
    pub fn exports(&self, silo_id: u64) -> Vec<(&str, &str)> {
        let mut result: Vec<(&str, &str)> = self.system_vars.values()
            .map(|v| (v.name.as_str(), v.value.as_str()))
            .collect();
        if let Some(store) = self.silo_vars.get(&silo_id) {
            for var in store.values() {
                if var.scope == VarScope::Export {
                    result.push((&var.name, &var.value));
                }
            }
        }
        result
    }

    /// Clean up all variables for a terminated Silo.
    pub fn cleanup_silo(&mut self, silo_id: u64) {
        self.silo_vars.remove(&silo_id);
    }
}
