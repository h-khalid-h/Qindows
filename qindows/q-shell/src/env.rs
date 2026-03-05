//! # Q-Shell Environment Variables
//!
//! Per-Silo environment variable management with inheritance,
//! variable expansion, and PATH manipulation.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// An environment variable entry.
#[derive(Debug, Clone)]
pub struct EnvVar {
    /// Variable name (case-insensitive stored uppercase)
    pub name: String,
    /// Value
    pub value: String,
    /// Read-only (set by system)
    pub readonly: bool,
    /// Exported to child processes
    pub exported: bool,
}

/// The environment variable store.
pub struct Environment {
    /// Variables (uppercase name → EnvVar)
    pub vars: BTreeMap<String, EnvVar>,
    /// Silo ID
    pub silo_id: u64,
}

impl Environment {
    pub fn new(silo_id: u64) -> Self {
        let mut env = Environment {
            vars: BTreeMap::new(),
            silo_id,
        };
        env.set_defaults();
        env
    }

    fn set_defaults(&mut self) {
        self.set_system("HOME", "/home/user");
        self.set_system("USER", "user");
        self.set_system("SHELL", "/bin/qsh");
        self.set_system("TERM", "qterm-256color");
        self.set_system("LANG", "en_US.UTF-8");
        self.set_system("PATH", "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin");
        self.set_system("PWD", "/home/user");
        self.set_system("HOSTNAME", "qindows");
        self.set_system("SILO_ID", &alloc::format!("{}", self.silo_id));
    }

    fn set_system(&mut self, name: &str, value: &str) {
        let key = name.to_uppercase();
        self.vars.insert(key.clone(), EnvVar {
            name: key,
            value: String::from(value),
            readonly: true,
            exported: true,
        });
    }

    /// Get a variable value.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars.get(&name.to_uppercase()).map(|v| v.value.as_str())
    }

    /// Set a variable.
    pub fn set(&mut self, name: &str, value: &str) -> bool {
        let key = name.to_uppercase();

        // Check readonly
        if let Some(existing) = self.vars.get(&key) {
            if existing.readonly { return false; }
        }

        self.vars.insert(key.clone(), EnvVar {
            name: key,
            value: String::from(value),
            readonly: false,
            exported: false,
        });
        true
    }

    /// Unset a variable.
    pub fn unset(&mut self, name: &str) -> bool {
        let key = name.to_uppercase();
        if let Some(v) = self.vars.get(&key) {
            if v.readonly { return false; }
        }
        self.vars.remove(&key).is_some()
    }

    /// Export a variable (make visible to child processes).
    pub fn export(&mut self, name: &str) {
        let key = name.to_uppercase();
        if let Some(v) = self.vars.get_mut(&key) {
            v.exported = true;
        }
    }

    /// Expand variables in a string: $VAR and ${VAR}.
    pub fn expand(&self, input: &str) -> String {
        let mut result = String::new();
        let chars: Vec<char> = input.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                i += 1;
                if chars[i] == '{' {
                    // ${VAR} form
                    i += 1;
                    let start = i;
                    while i < chars.len() && chars[i] != '}' { i += 1; }
                    let name: String = chars[start..i].iter().collect();
                    if let Some(val) = self.get(&name) {
                        result.push_str(val);
                    }
                    if i < chars.len() { i += 1; } // skip '}'
                } else if chars[i] == '?' {
                    // $? = last exit code (stub)
                    result.push('0');
                    i += 1;
                } else {
                    // $VAR form
                    let start = i;
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        i += 1;
                    }
                    let name: String = chars[start..i].iter().collect();
                    if let Some(val) = self.get(&name) {
                        result.push_str(val);
                    }
                }
            } else if chars[i] == '\\' && i + 1 < chars.len() && chars[i + 1] == '$' {
                result.push('$');
                i += 2;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        result
    }

    /// Prepend a directory to PATH.
    pub fn path_prepend(&mut self, dir: &str) {
        if let Some(v) = self.vars.get_mut("PATH") {
            v.value = alloc::format!("{}:{}", dir, v.value);
        }
    }

    /// Append a directory to PATH.
    pub fn path_append(&mut self, dir: &str) {
        if let Some(v) = self.vars.get_mut("PATH") {
            v.value = alloc::format!("{}:{}", v.value, dir);
        }
    }

    /// Get PATH entries as a list.
    pub fn path_entries(&self) -> Vec<&str> {
        self.get("PATH")
            .map(|p| p.split(':').collect())
            .unwrap_or_default()
    }

    /// Get all exported variables.
    pub fn exported(&self) -> Vec<(&str, &str)> {
        self.vars.values()
            .filter(|v| v.exported)
            .map(|v| (v.name.as_str(), v.value.as_str()))
            .collect()
    }

    /// List all variables.
    pub fn list_all(&self) -> Vec<(&str, &str, bool)> {
        self.vars.values()
            .map(|v| (v.name.as_str(), v.value.as_str(), v.readonly))
            .collect()
    }

    /// Create a child environment (inherits exported vars).
    pub fn fork(&self, child_silo_id: u64) -> Environment {
        let mut child = Environment {
            vars: BTreeMap::new(),
            silo_id: child_silo_id,
        };

        for v in self.vars.values() {
            if v.exported {
                child.vars.insert(v.name.clone(), EnvVar {
                    name: v.name.clone(),
                    value: v.value.clone(),
                    readonly: false, // Not readonly in child
                    exported: true,
                });
            }
        }

        // Override child-specific vars
        child.set("SILO_ID", &alloc::format!("{}", child_silo_id));
        child
    }
}
