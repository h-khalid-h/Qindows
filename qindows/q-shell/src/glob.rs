//! # Q-Shell Glob Pattern Matcher
//!
//! Implements glob pattern matching for file paths.
//! Supports `*`, `?`, `**`, `[abc]`, `[!abc]`, `{a,b,c}`,
//! and `\` escaping.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// Match a glob pattern against a string.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pat: Vec<char> = pattern.chars().collect();
    let txt: Vec<char> = text.chars().collect();
    glob_match_inner(&pat, 0, &txt, 0)
}

fn glob_match_inner(pat: &[char], mut pi: usize, txt: &[char], mut ti: usize) -> bool {
    while pi < pat.len() {
        match pat[pi] {
            '?' => {
                // Match exactly one character (but not path separator)
                if ti >= txt.len() || txt[ti] == '/' { return false; }
                pi += 1;
                ti += 1;
            }
            '*' => {
                // Check for `**` (matches across directories)
                if pi + 1 < pat.len() && pat[pi + 1] == '*' {
                    pi += 2;
                    // Skip optional path separator after **
                    if pi < pat.len() && pat[pi] == '/' { pi += 1; }

                    // Try matching the rest of the pattern at every position
                    for i in ti..=txt.len() {
                        if glob_match_inner(pat, pi, txt, i) {
                            return true;
                        }
                    }
                    return false;
                }

                // Single `*` — match any chars except `/`
                pi += 1;
                for i in ti..=txt.len() {
                    if i > ti && i <= txt.len() && txt[i - 1] == '/' {
                        break; // Don't cross directory boundary
                    }
                    if glob_match_inner(pat, pi, txt, i) {
                        return true;
                    }
                }
                return false;
            }
            '[' => {
                // Character class
                pi += 1;
                if ti >= txt.len() { return false; }

                let negate = pi < pat.len() && (pat[pi] == '!' || pat[pi] == '^');
                if negate { pi += 1; }

                let mut matched = false;
                let ch = txt[ti];

                while pi < pat.len() && pat[pi] != ']' {
                    if pi + 2 < pat.len() && pat[pi + 1] == '-' {
                        // Range: [a-z]
                        let lo = pat[pi];
                        let hi = pat[pi + 2];
                        if ch >= lo && ch <= hi { matched = true; }
                        pi += 3;
                    } else {
                        if pat[pi] == ch { matched = true; }
                        pi += 1;
                    }
                }

                if pi < pat.len() { pi += 1; } // Skip ']'

                if negate { matched = !matched; }
                if !matched { return false; }
                ti += 1;
            }
            '{' => {
                // Brace expansion: {foo,bar,baz}
                pi += 1;
                let mut alternatives = Vec::new();
                let mut current = String::new();
                let mut depth = 1;

                while pi < pat.len() && depth > 0 {
                    match pat[pi] {
                        '{' => { depth += 1; current.push(pat[pi]); }
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                alternatives.push(current.clone());
                            } else {
                                current.push(pat[pi]);
                            }
                        }
                        ',' if depth == 1 => {
                            alternatives.push(current.clone());
                            current.clear();
                        }
                        _ => current.push(pat[pi]),
                    }
                    pi += 1;
                }

                // Try each alternative
                let rest_pat: Vec<char> = pat[pi..].to_vec();
                for alt in &alternatives {
                    let mut combined: Vec<char> = alt.chars().collect();
                    combined.extend_from_slice(&rest_pat);
                    if glob_match_inner(&combined, 0, txt, ti) {
                        return true;
                    }
                }
                return false;
            }
            '\\' => {
                // Escape: next char is literal
                pi += 1;
                if pi >= pat.len() { return false; }
                if ti >= txt.len() || pat[pi] != txt[ti] { return false; }
                pi += 1;
                ti += 1;
            }
            ch => {
                // Literal character
                if ti >= txt.len() || ch != txt[ti] { return false; }
                pi += 1;
                ti += 1;
            }
        }
    }

    // Pattern exhausted — text must also be exhausted
    ti == txt.len()
}

/// Expand brace patterns into all combinations.
pub fn brace_expand(pattern: &str) -> Vec<String> {
    let chars: Vec<char> = pattern.chars().collect();

    // Find the first top-level brace group
    let mut depth = 0;
    let mut brace_start = None;
    let mut brace_end = None;

    for (i, &ch) in chars.iter().enumerate() {
        match ch {
            '{' => {
                if depth == 0 { brace_start = Some(i); }
                depth += 1;
            }
            '}' => {
                depth -= 1;
                if depth == 0 {
                    brace_end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }

    let (start, end) = match (brace_start, brace_end) {
        (Some(s), Some(e)) => (s, e),
        _ => return alloc::vec![pattern.into()],
    };

    let prefix: String = chars[..start].iter().collect();
    let suffix: String = chars[end + 1..].iter().collect();
    let inner: String = chars[start + 1..end].iter().collect();

    // Split by top-level commas
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut inner_depth = 0;

    for ch in inner.chars() {
        match ch {
            '{' => { inner_depth += 1; current.push(ch); }
            '}' => { inner_depth -= 1; current.push(ch); }
            ',' if inner_depth == 0 => {
                parts.push(current.clone());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    parts.push(current);

    let mut results = Vec::new();
    for part in &parts {
        let expanded = alloc::format!("{}{}{}", prefix, part, suffix);
        // Recursively expand nested braces
        results.extend(brace_expand(&expanded));
    }
    results
}

/// Check if a filename matches any pattern in a list.
pub fn matches_any(patterns: &[&str], filename: &str) -> bool {
    patterns.iter().any(|p| glob_match(p, filename))
}

/// Filter a list of filenames by a glob pattern.
pub fn filter_glob<'a>(pattern: &str, filenames: &'a [&str]) -> Vec<&'a str> {
    filenames.iter().filter(|f| glob_match(pattern, f)).copied().collect()
}
