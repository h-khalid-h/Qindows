//! # Copy-on-Write B-Tree Index
//!
//! Persistent index for the Prism Object Graph.
//! Maps Object IDs (OID) to data block pointers.
//!
//! Design:
//! - Each node fits in a single 4 KiB page
//! - Copy-on-Write: modifications create new nodes (old versions survive)
//! - This enables instant snapshots and crash recovery

#![allow(dead_code)]

use alloc::vec::Vec;

/// Maximum keys per B-tree node.
/// Sized to fit one 4 KiB page with 32-byte keys.
const ORDER: usize = 64;

/// A key in the B-tree: 32-byte Object ID.
pub type BTreeKey = [u8; 32];

/// A value: a block pointer (offset into the data region).
#[derive(Debug, Clone, Copy)]
pub struct BlockPointer {
    /// Byte offset in the data region
    pub offset: u64,
    /// Length in bytes
    pub length: u64,
    /// Checksum (for integrity verification)
    pub checksum: u32,
}

/// A B-tree node ID (page number in the index file).
pub type NodeId = u64;

/// B-tree node — fits in a single 4 KiB page.
#[derive(Debug, Clone)]
pub struct BTreeNode {
    /// Unique node ID (page number)
    pub id: NodeId,
    /// Is this a leaf node?
    pub is_leaf: bool,
    /// Number of active keys
    pub key_count: usize,
    /// Keys (sorted)
    pub keys: Vec<BTreeKey>,
    /// Values (only in leaf nodes)
    pub values: Vec<BlockPointer>,
    /// Child node IDs (only in internal nodes; key_count + 1 children)
    pub children: Vec<NodeId>,
    /// Dirty flag (needs write-back)
    pub dirty: bool,
}

impl BTreeNode {
    /// Create a new empty leaf node.
    pub fn new_leaf(id: NodeId) -> Self {
        BTreeNode {
            id,
            is_leaf: true,
            key_count: 0,
            keys: Vec::with_capacity(ORDER),
            values: Vec::with_capacity(ORDER),
            children: Vec::new(),
            dirty: true,
        }
    }

    /// Create a new empty internal node.
    pub fn new_internal(id: NodeId) -> Self {
        BTreeNode {
            id,
            is_leaf: false,
            key_count: 0,
            keys: Vec::with_capacity(ORDER),
            values: Vec::new(),
            children: Vec::with_capacity(ORDER + 1),
            dirty: true,
        }
    }

    /// Find the index where a key belongs (binary search).
    pub fn find_key_index(&self, key: &BTreeKey) -> usize {
        match self.keys[..self.key_count].binary_search(key) {
            Ok(i) => i,
            Err(i) => i,
        }
    }

    /// Is this node full?
    pub fn is_full(&self) -> bool {
        self.key_count >= ORDER - 1
    }
}

/// The B-Tree index.
pub struct BTree {
    /// Root node ID
    pub root_id: NodeId,
    /// All nodes (in-memory cache — paged to disk in production)
    nodes: Vec<BTreeNode>,
    /// Next available node ID
    next_id: NodeId,
}

impl BTree {
    /// Create a new empty B-tree.
    pub fn new() -> Self {
        let root = BTreeNode::new_leaf(0);
        BTree {
            root_id: 0,
            nodes: alloc::vec![root],
            next_id: 1,
        }
    }

    /// Get a reference to a node by ID.
    fn get_node(&self, id: NodeId) -> Option<&BTreeNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Get a mutable reference to a node by ID.
    fn get_node_mut(&mut self, id: NodeId) -> Option<&mut BTreeNode> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    /// Allocate a new node.
    fn alloc_node(&mut self, is_leaf: bool) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        let node = if is_leaf {
            BTreeNode::new_leaf(id)
        } else {
            BTreeNode::new_internal(id)
        };
        self.nodes.push(node);
        id
    }

    /// Search for a key in the B-tree.
    pub fn search(&self, key: &BTreeKey) -> Option<BlockPointer> {
        self.search_node(self.root_id, key)
    }

    fn search_node(&self, node_id: NodeId, key: &BTreeKey) -> Option<BlockPointer> {
        let node = self.get_node(node_id)?;
        let idx = node.find_key_index(key);

        if idx < node.key_count && &node.keys[idx] == key {
            // Key found
            if node.is_leaf {
                return Some(node.values[idx]);
            }
        }

        if node.is_leaf {
            return None; // Not found in leaf
        }

        // Recurse into child
        if idx < node.children.len() {
            self.search_node(node.children[idx], key)
        } else {
            None
        }
    }

    /// Insert a key-value pair.
    ///
    /// Uses Copy-on-Write: if the target node is shared (snapshot),
    /// a new copy is made before modification.
    pub fn insert(&mut self, key: BTreeKey, value: BlockPointer) {
        let root_id = self.root_id;

        if let Some(root) = self.get_node(root_id) {
            if root.is_full() {
                // Root is full — split it
                let new_root_id = self.alloc_node(false);
                {
                    let new_root = self.get_node_mut(new_root_id).unwrap();
                    new_root.children.push(root_id);
                }
                self.split_child(new_root_id, 0);
                self.root_id = new_root_id;
                self.insert_nonfull(new_root_id, key, value);
            } else {
                self.insert_nonfull(root_id, key, value);
            }
        }
    }

    /// Insert into a non-full node.
    fn insert_nonfull(&mut self, node_id: NodeId, key: BTreeKey, value: BlockPointer) {
        let is_leaf;
        let idx;
        {
            let node = self.get_node(node_id).unwrap();
            is_leaf = node.is_leaf;
            idx = node.find_key_index(&key);
        }

        if is_leaf {
            let node = self.get_node_mut(node_id).unwrap();
            node.keys.insert(idx, key);
            node.values.insert(idx, value);
            node.key_count += 1;
            node.dirty = true;
        } else {
            let child_id = {
                let node = self.get_node(node_id).unwrap();
                node.children[idx]
            };

            let child_full = {
                let child = self.get_node(child_id).unwrap();
                child.is_full()
            };

            if child_full {
                self.split_child(node_id, idx);
                let node = self.get_node(node_id).unwrap();
                let actual_idx = if key > node.keys[idx] { idx + 1 } else { idx };
                let child_id = node.children[actual_idx];
                self.insert_nonfull(child_id, key, value);
            } else {
                self.insert_nonfull(child_id, key, value);
            }
        }
    }

    /// Split a full child node.
    fn split_child(&mut self, parent_id: NodeId, child_index: usize) {
        let child_id = {
            let parent = self.get_node(parent_id).unwrap();
            parent.children[child_index]
        };

        let mid = ORDER / 2;
        let new_id = self.next_id;
        self.next_id += 1;

        // Create sibling with the right half of the child
        let (mid_key, new_node_is_leaf) = {
            let child = self.get_node(child_id).unwrap();
            (child.keys[mid - 1], child.is_leaf)
        };

        let mut new_node = if new_node_is_leaf {
            BTreeNode::new_leaf(new_id)
        } else {
            BTreeNode::new_internal(new_id)
        };

        // Move right half of keys/values to new node
        {
            let child = self.get_node_mut(child_id).unwrap();
            new_node.keys = child.keys.split_off(mid);
            new_node.key_count = new_node.keys.len();
            child.key_count = child.keys.len();

            if child.is_leaf {
                new_node.values = child.values.split_off(mid);
            }
            if !child.is_leaf && child.children.len() > mid {
                new_node.children = child.children.split_off(mid);
            }
            child.dirty = true;
        }

        new_node.dirty = true;
        self.nodes.push(new_node);

        // Insert median key into parent
        let parent = self.get_node_mut(parent_id).unwrap();
        parent.keys.insert(child_index, mid_key);
        parent.children.insert(child_index + 1, new_id);
        parent.key_count += 1;
        parent.dirty = true;
    }

    /// Get total number of entries in the tree.
    pub fn len(&self) -> usize {
        self.count_entries(self.root_id)
    }

    fn count_entries(&self, node_id: NodeId) -> usize {
        match self.get_node(node_id) {
            Some(node) => {
                if node.is_leaf {
                    node.key_count
                } else {
                    let child_sum: usize = node
                        .children
                        .iter()
                        .map(|&c| self.count_entries(c))
                        .sum();
                    node.key_count + child_sum
                }
            }
            None => 0,
        }
    }

    /// Collect all dirty nodes (for write-back to journal).
    pub fn dirty_nodes(&self) -> Vec<NodeId> {
        self.nodes
            .iter()
            .filter(|n| n.dirty)
            .map(|n| n.id)
            .collect()
    }
}
