//! # Q-Shell Alias System
//!
//! Command aliases with parameter substitution, recursive
//! expansion, and system/user alias separation.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

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

/// The Alias Manager.
pub struct AliasManager {
    /// Aliases: name → definition
    pub aliases: BTreeMap<String, Alias>,
    /// Maximum recursion depth for alias expansion
    pub max_depth: u8,
    /// Stats
    pub total_expansions: u64,
    pub expansion_errors: u64,
}

impl AliasManager {
    pub fn new() -> Self {
        let mut mgr = AliasManager {
            aliases: BTreeMap::new(),
            max_depth: 10,
            total_expansions: 0,
            expansion_errors: 0,
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

        // Safety
        self.add_system("rm", "rm -i");
        self.add_system("cp", "cp -i");
        self.add_system("mv", "mv -i");

        // Q-Shell specific
        self.add_system("silo-list", "qctl silo list");
        self.add_system("silo-kill", "qctl silo kill $1");
        self.add_system("silo-info", "qctl silo info $1");
        self.add_system("mem", "qctl memory status");
        self.add_system("top", "qctl process top");
    }

    fn add_system(&mut self, name: &str, expansion: &str) {
        self.aliases.insert(String::from(name), Alias {
            name: String::from(name),
            expansion: String::from(expansion),
            system: true,
            recursive: true,
            invocations: 0,
        });
    }

    /// Define a user alias.
    pub fn define(&mut self, name: &str, expansion: &str) -> bool {
        let key = String::from(name);

        // Cannot override system aliases
        if let Some(existing) = self.aliases.get(&key) {
            if existing.system { return false; }
        }

        self.aliases.insert(key.clone(), Alias {
            name: key,
            expansion: String::from(expansion),
            system: false,
            recursive: true,
            invocations: 0,
        });
        true
    }

    /// Remove a user alias.
    pub fn undefine(&mut self, name: &str) -> bool {
        if let Some(alias) = self.aliases.get(name) {
            if alias.system { return false; }
        }
        self.aliases.remove(name).is_some()
    }

    /// Expand a command line, replacing alias names with their expansions.
    ///
    /// Supports parameter substitution:
    /// - `$1`, `$2`, ... → positional arguments
    /// - `$@` → all arguments
    /// - `$#` → argument count
    pub fn expand(&mut self, input: &str) -> String {
        self.expand_recursive(input, 0)
    }

    fn expand_recursive(&mut self, input: &str, depth: u8) -> String {
        if depth >= self.max_depth {
            self.expansion_errors += 1;
            return String::from(input);
        }

        let parts: Vec<&str> = input.splitn(2, |c: char| c.is_whitespace()).collect();
        let cmd = parts[0];
        let args_str = if parts.len() > 1 { parts[1] } else { "" };

        // Look up the alias
        let alias = match self.aliases.get(cmd) {
            Some(a) => a.clone(),
            None => return String::from(input),
        };

        // Track invocation (need to re-borrow mutably)
        if let Some(a) = self.aliases.get_mut(cmd) {
            a.invocations += 1;
        }
        self.total_expansions += 1;

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
            self.expand_recursive(&expanded, depth + 1)
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

    /// Get an alias by name.
    pub fn get(&self, name: &str) -> Option<&Alias> {
        self.aliases.get(name)
    }

    /// List all aliases (sorted by name).
    pub fn list_all(&self) -> Vec<(&str, &str, bool)> {
        self.aliases.values()
            .map(|a| (a.name.as_str(), a.expansion.as_str(), a.system))
            .collect()
    }

    /// List only user aliases.
    pub fn list_user(&self) -> Vec<(&str, &str)> {
        self.aliases.values()
            .filter(|a| !a.system)
            .map(|a| (a.name.as_str(), a.expansion.as_str()))
            .collect()
    }

    /// Check if a command is aliased.
    pub fn is_alias(&self, name: &str) -> bool {
        self.aliases.contains_key(name)
    }

    /// Reset all user aliases (keep system ones).
    pub fn reset_user(&mut self) {
        self.aliases.retain(|_, a| a.system);
    }
}
