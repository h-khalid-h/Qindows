//! # Q-Shell Alias System
//!
//! Command aliases with parameter substitution, recursive
//! expansion, per-Silo namespacing, and system/user alias
//! separation (Section 7.6).
//!
//! Features:
//! - Parameter substitution ($1, $2, $@, $#)
//! - System aliases (cannot be overridden by user)
//! - Per-Silo alias scopes (Silo-local overrides global)
//! - Recursive expansion with loop detection
//! - Invocation counting and statistics
//! - Built-in defaults for navigation, safety, and Q-Shell

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Alias scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasScope {
    Global,
    Silo(u64),
}

/// An alias definition.
#[derive(Debug, Clone)]
pub struct Alias {
    /// Alias name
    pub name: String,
    /// Expansion template (may contain $1, $2, $@ for parameter substitution)
    pub expansion: String,
    /// Is this a system alias? (cannot be overridden by user)
    pub system: bool,
    /// Is this alias recursive? (can expand other aliases in its body)
    pub recursive: bool,
    /// Number of times this alias was invoked
    pub invocations: u64,
}

/// Alias manager statistics.
#[derive(Debug, Clone, Default)]
pub struct AliasStats {
    pub total_expansions: u64,
    pub expansion_errors: u64,
    pub loop_detections: u64,
}

/// The Alias Manager.
pub struct AliasManager {
    /// Global aliases: name → definition
    pub global: BTreeMap<String, Alias>,
    /// Per-Silo aliases: silo_id → (name → definition)
    pub silo_aliases: BTreeMap<u64, BTreeMap<String, Alias>>,
    /// Maximum recursion depth for alias expansion
    pub max_depth: u8,
    /// Statistics
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
        mgr.load_defaults();
        mgr
    }

    fn load_defaults(&mut self) {
        // Navigation
        self.add_system("ll", "ls -la");
        self.add_system("la", "ls -a");
        self.add_system("..", "cd ..");
        self.add_system("...", "cd ../..");
        self.add_system("~", "cd $HOME");
        self.add_system("cls", "clear");

        // Safety
        self.add_system("rm", "rm -i");
        self.add_system("cp", "cp -i");
        self.add_system("mv", "mv -i");

        // Utilities
        self.add_system("grep", "grep --color=auto");
        self.add_system("df", "df -h");
        self.add_system("du", "du -sh");

        // Q-Shell specific
        self.add_system("silo-list", "qctl silo list");
        self.add_system("silo-kill", "qctl silo kill $1");
        self.add_system("silo-info", "qctl silo info $1");
        self.add_system("mem", "qctl memory status");
        self.add_system("top", "qctl process top");
    }

    fn add_system(&mut self, name: &str, expansion: &str) {
        self.global.insert(String::from(name), Alias {
            name: String::from(name),
            expansion: String::from(expansion),
            system: true,
            recursive: true,
            invocations: 0,
        });
    }

    /// Define a user alias in any scope.
    pub fn define(&mut self, name: &str, expansion: &str, scope: AliasScope) -> bool {
        // Cannot override system aliases in global scope
        if let AliasScope::Global = scope {
            if let Some(existing) = self.global.get(name) {
                if existing.system { return false; }
            }
        }

        let alias = Alias {
            name: String::from(name),
            expansion: String::from(expansion),
            system: false,
            recursive: true,
            invocations: 0,
        };

        match scope {
            AliasScope::Global => { self.global.insert(String::from(name), alias); }
            AliasScope::Silo(id) => {
                self.silo_aliases.entry(id)
                    .or_insert_with(BTreeMap::new)
                    .insert(String::from(name), alias);
            }
        }
        true
    }

    /// Remove a user alias.
    pub fn undefine(&mut self, name: &str, scope: AliasScope) -> bool {
        match scope {
            AliasScope::Global => {
                if let Some(alias) = self.global.get(name) {
                    if alias.system { return false; }
                }
                self.global.remove(name).is_some()
            }
            AliasScope::Silo(id) => {
                self.silo_aliases.get_mut(&id)
                    .map(|m| m.remove(name).is_some())
                    .unwrap_or(false)
            }
        }
    }

    /// Expand a command line, replacing alias names with their expansions.
    ///
    /// Supports parameter substitution:
    /// - `$1`, `$2`, ... → positional arguments
    /// - `$@` → all arguments
    /// - `$#` → argument count
    ///
    /// Silo-scoped aliases take priority over global aliases.
    pub fn expand(&mut self, input: &str, silo_id: Option<u64>) -> String {
        self.expand_recursive(input, 0, silo_id, &mut Vec::new())
    }

    fn expand_recursive(
        &mut self, input: &str, depth: u8,
        silo_id: Option<u64>, seen: &mut Vec<String>,
    ) -> String {
        if depth >= self.max_depth {
            self.stats.expansion_errors += 1;
            self.stats.loop_detections += 1;
            return String::from(input);
        }

        let parts: Vec<&str> = input.splitn(2, |c: char| c.is_whitespace()).collect();
        let cmd = parts[0];
        let args_str = if parts.len() > 1 { parts[1] } else { "" };

        // Loop detection
        let cmd_string = String::from(cmd);
        if seen.contains(&cmd_string) {
            self.stats.loop_detections += 1;
            return String::from(input);
        }

        // Look up: Silo-scoped first, then global
        let alias = silo_id
            .and_then(|id| self.silo_aliases.get(&id))
            .and_then(|m| m.get(cmd))
            .or_else(|| self.global.get(cmd))
            .cloned();

        let alias = match alias {
            Some(a) => a,
            None => return String::from(input),
        };

        // Track invocation
        if let Some(a) = silo_id
            .and_then(|id| self.silo_aliases.get_mut(&id))
            .and_then(|m| m.get_mut(cmd))
        {
            a.invocations += 1;
        } else if let Some(a) = self.global.get_mut(cmd) {
            a.invocations += 1;
        }
        self.stats.total_expansions += 1;

        // Parse arguments
        let args: Vec<&str> = if args_str.is_empty() {
            Vec::new()
        } else {
            args_str.split_whitespace().collect()
        };

        // Substitute parameters in the expansion
        let expanded = self.substitute(&alias.expansion, &args);

        // Recursively expand if enabled
        if alias.recursive {
            seen.push(cmd_string);
            let result = self.expand_recursive(&expanded, depth + 1, silo_id, seen);
            seen.pop();
            result
        } else {
            expanded
        }
    }

    /// Substitute $1, $2, $@, $# in a template with actual arguments.
    fn substitute(&self, template: &str, args: &[&str]) -> String {
        let mut result = String::new();
        let chars: Vec<char> = template.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '$' && i + 1 < chars.len() {
                i += 1;
                if chars[i] == '@' {
                    // $@ → all arguments joined by space
                    result.push_str(&args.join(" "));
                    i += 1;
                } else if chars[i] == '#' {
                    // $# → argument count
                    let count_str = alloc::format!("{}", args.len());
                    result.push_str(&count_str);
                    i += 1;
                } else if chars[i].is_ascii_digit() {
                    // $N → Nth argument (1-based)
                    let start = i;
                    while i < chars.len() && chars[i].is_ascii_digit() { i += 1; }
                    let num_str: String = chars[start..i].iter().collect();
                    if let Ok(n) = num_str.parse::<usize>() {
                        if n > 0 && n <= args.len() {
                            result.push_str(args[n - 1]);
                        }
                    }
                } else if chars[i] == '$' {
                    // $$ → literal $
                    result.push('$');
                    i += 1;
                } else {
                    // Not a known substitution — keep as-is
                    result.push('$');
                    result.push(chars[i]);
                    i += 1;
                }
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }

        result
    }

    /// Get an alias by name (checks Silo scope first, then global).
    pub fn get(&self, name: &str, silo_id: Option<u64>) -> Option<&Alias> {
        silo_id
            .and_then(|id| self.silo_aliases.get(&id))
            .and_then(|m| m.get(name))
            .or_else(|| self.global.get(name))
    }

    /// List all global aliases (sorted by name).
    pub fn list_all(&self) -> Vec<(&str, &str, bool)> {
        self.global.values()
            .map(|a| (a.name.as_str(), a.expansion.as_str(), a.system))
            .collect()
    }

    /// List aliases for a specific Silo.
    pub fn list_silo(&self, silo_id: u64) -> Vec<(&str, &str)> {
        self.silo_aliases.get(&silo_id)
            .map(|m| m.values().map(|a| (a.name.as_str(), a.expansion.as_str())).collect())
            .unwrap_or_default()
    }

    /// List only user (non-system) global aliases.
    pub fn list_user(&self) -> Vec<(&str, &str)> {
        self.global.values()
            .filter(|a| !a.system)
            .map(|a| (a.name.as_str(), a.expansion.as_str()))
            .collect()
    }

    /// Check if a command is aliased.
    pub fn is_alias(&self, name: &str, silo_id: Option<u64>) -> bool {
        self.get(name, silo_id).is_some()
    }

    /// Reset all user aliases (keep system ones).
    pub fn reset_user(&mut self) {
        self.global.retain(|_, a| a.system);
        self.silo_aliases.clear();
    }
}
