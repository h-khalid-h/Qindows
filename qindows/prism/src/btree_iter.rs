//! # Prism B-tree Range Iterator
//!
//! Provides efficient range queries over the Prism B-tree.
//! Supports forward/reverse iteration, prefix scans, and
//! cursor-based pagination for the VFS and Q-Shell.

extern crate alloc;

use alloc::vec::Vec;

/// A key-value pair from the B-tree.
#[derive(Debug, Clone)]
pub struct BTreeEntry {
    /// Key bytes
    pub key: Vec<u8>,
    /// Value bytes
    pub value: Vec<u8>,
    /// Object ID
    pub oid: u64,
    /// Entry timestamp
    pub timestamp: u64,
    /// Is this entry a tombstone (deleted)?
    pub deleted: bool,
}

/// Range bound.
#[derive(Debug, Clone)]
pub enum Bound {
    /// No bound (open)
    Unbounded,
    /// Inclusive bound
    Included(Vec<u8>),
    /// Exclusive bound
    Excluded(Vec<u8>),
}

/// Iteration direction.
#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Forward,
    Reverse,
}

/// A query filter.
#[derive(Debug, Clone)]
pub enum Filter {
    /// Key prefix match
    Prefix(Vec<u8>),
    /// Key suffix match
    Suffix(Vec<u8>),
    /// Key contains substring
    Contains(Vec<u8>),
    /// Value size range (min, max)
    ValueSize(u64, u64),
    /// Only non-deleted entries
    ExcludeDeleted,
    /// Only entries after timestamp
    AfterTimestamp(u64),
}

/// A B-tree range iterator / cursor.
pub struct BTreeCursor {
    /// Lower bound
    pub lower: Bound,
    /// Upper bound
    pub upper: Bound,
    /// Direction
    pub direction: Direction,
    /// Filters
    pub filters: Vec<Filter>,
    /// Maximum entries to return (0 = unlimited)
    pub limit: usize,
    /// Skip this many entries before returning
    pub offset: usize,
    /// Current position (key of last returned entry)
    pub position: Option<Vec<u8>>,
    /// Has the cursor been exhausted?
    pub exhausted: bool,
    /// Total entries scanned
    pub scanned: u64,
    /// Total entries returned
    pub returned: u64,
}

impl BTreeCursor {
    /// Create a cursor for a full scan.
    pub fn full_scan() -> Self {
        BTreeCursor {
            lower: Bound::Unbounded,
            upper: Bound::Unbounded,
            direction: Direction::Forward,
            filters: Vec::new(),
            limit: 0,
            offset: 0,
            position: None,
            exhausted: false,
            scanned: 0,
            returned: 0,
        }
    }

    /// Create a cursor for a key range [start, end).
    pub fn range(start: Vec<u8>, end: Vec<u8>) -> Self {
        BTreeCursor {
            lower: Bound::Included(start),
            upper: Bound::Excluded(end),
            direction: Direction::Forward,
            filters: Vec::new(),
            limit: 0,
            offset: 0,
            position: None,
            exhausted: false,
            scanned: 0,
            returned: 0,
        }
    }

    /// Create a cursor for a key prefix scan.
    pub fn prefix_scan(prefix: Vec<u8>) -> Self {
        let mut upper = prefix.clone();
        // Increment last byte for exclusive upper bound
        if let Some(last) = upper.last_mut() {
            if *last < 255 {
                *last += 1;
            } else {
                upper.push(0);
            }
        }

        BTreeCursor {
            lower: Bound::Included(prefix.clone()),
            upper: Bound::Excluded(upper),
            direction: Direction::Forward,
            filters: alloc::vec![Filter::Prefix(prefix)],
            limit: 0,
            offset: 0,
            position: None,
            exhausted: false,
            scanned: 0,
            returned: 0,
        }
    }

    /// Set pagination (LIMIT + OFFSET).
    pub fn paginate(mut self, limit: usize, offset: usize) -> Self {
        self.limit = limit;
        self.offset = offset;
        self
    }

    /// Set direction to reverse.
    pub fn reverse(mut self) -> Self {
        self.direction = Direction::Reverse;
        self
    }

    /// Add a filter.
    pub fn with_filter(mut self, filter: Filter) -> Self {
        self.filters.push(filter);
        self
    }

    /// Check if an entry passes all filters.
    pub fn matches(&self, entry: &BTreeEntry) -> bool {
        for filter in &self.filters {
            match filter {
                Filter::Prefix(prefix) => {
                    if !entry.key.starts_with(prefix) { return false; }
                }
                Filter::Suffix(suffix) => {
                    if !entry.key.ends_with(suffix) { return false; }
                }
                Filter::Contains(sub) => {
                    if !contains_subsequence(&entry.key, sub) { return false; }
                }
                Filter::ValueSize(min, max) => {
                    let size = entry.value.len() as u64;
                    if size < *min || size > *max { return false; }
                }
                Filter::ExcludeDeleted => {
                    if entry.deleted { return false; }
                }
                Filter::AfterTimestamp(ts) => {
                    if entry.timestamp <= *ts { return false; }
                }
            }
        }
        true
    }

    /// Check if a key is within the cursor's range bounds.
    pub fn in_range(&self, key: &[u8]) -> bool {
        let lower_ok = match &self.lower {
            Bound::Unbounded => true,
            Bound::Included(bound) => key >= bound.as_slice(),
            Bound::Excluded(bound) => key > bound.as_slice(),
        };

        let upper_ok = match &self.upper {
            Bound::Unbounded => true,
            Bound::Included(bound) => key <= bound.as_slice(),
            Bound::Excluded(bound) => key < bound.as_slice(),
        };

        lower_ok && upper_ok
    }

    /// Process a batch of entries from the B-tree.
    pub fn process_batch(&mut self, entries: &[BTreeEntry]) -> Vec<BTreeEntry> {
        let mut results = Vec::new();

        for entry in entries {
            if self.exhausted { break; }
            self.scanned += 1;

            // Range check
            if !self.in_range(&entry.key) {
                // On forward scan, if key is past the upper bound, we're done.
                // But if key is below the lower bound, just skip it.
                match self.direction {
                    Direction::Forward => {
                        let past_upper = match &self.upper {
                            Bound::Unbounded => false,
                            Bound::Included(b) => entry.key.as_slice() > b.as_slice(),
                            Bound::Excluded(b) => entry.key.as_slice() >= b.as_slice(),
                        };
                        if past_upper { self.exhausted = true; break; }
                    }
                    Direction::Reverse => {
                        let past_lower = match &self.lower {
                            Bound::Unbounded => false,
                            Bound::Included(b) => entry.key.as_slice() < b.as_slice(),
                            Bound::Excluded(b) => entry.key.as_slice() <= b.as_slice(),
                        };
                        if past_lower { self.exhausted = true; break; }
                    }
                }
                continue;
            }

            // Filter check
            if !self.matches(entry) { continue; }

            // Offset
            if self.offset > 0 {
                self.offset -= 1;
                continue;
            }

            // Limit
            if self.limit > 0 && self.returned as usize >= self.limit {
                self.exhausted = true;
                break;
            }

            self.returned += 1;
            self.position = Some(entry.key.clone());
            results.push(entry.clone());
        }

        results
    }
}

impl PartialEq for Direction {
    fn eq(&self, other: &Self) -> bool {
        matches!((self, other), (Direction::Forward, Direction::Forward) | (Direction::Reverse, Direction::Reverse))
    }
}

/// Check if `haystack` contains `needle` as a subsequence.
fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() { return true; }
    if needle.len() > haystack.len() { return false; }
    haystack.windows(needle.len()).any(|w| w == needle)
}
