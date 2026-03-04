//! # Prism Virtual Filesystem Layer (VFS)
//!
//! Provides POSIX-like path resolution on top of the Prism Object Graph.
//! Chimera uses this to translate Win32 paths (C:\Users\...)
//! into Prism OID lookups. Native Qindows apps use semantic
//! intent queries instead — but the VFS exists for compatibility.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A VFS node — either a directory or a file reference.
#[derive(Debug, Clone)]
pub enum VfsNode {
    /// Directory containing named children
    Directory {
        children: BTreeMap<String, VfsNode>,
        oid: u64,
    },
    /// File reference pointing to a Prism Q-Node
    File {
        oid: u64,
        size: u64,
    },
    /// Symbolic link to another path
    Symlink {
        target: String,
    },
    /// Mount point (another storage device or Prism sub-graph)
    Mount {
        device: String,
        root: u64,
    },
}

/// VFS errors
#[derive(Debug, Clone)]
pub enum VfsError {
    NotFound,
    NotADirectory,
    NotAFile,
    AlreadyExists,
    PermissionDenied,
    InvalidPath,
    MountPointBusy,
}

/// The Virtual Filesystem.
pub struct Vfs {
    /// Root directory
    root: VfsNode,
    /// Mount table (path → device)
    mounts: BTreeMap<String, String>,
}

impl Vfs {
    /// Create a new VFS with the standard Qindows directory layout.
    pub fn new() -> Self {
        let mut root_children = BTreeMap::new();

        // Standard Qindows directories
        root_children.insert(String::from("system"), VfsNode::Directory {
            children: BTreeMap::new(),
            oid: 100,
        });
        root_children.insert(String::from("users"), VfsNode::Directory {
            children: BTreeMap::new(),
            oid: 200,
        });
        root_children.insert(String::from("apps"), VfsNode::Directory {
            children: BTreeMap::new(),
            oid: 300,
        });
        root_children.insert(String::from("drivers"), VfsNode::Directory {
            children: BTreeMap::new(),
            oid: 400,
        });
        root_children.insert(String::from("temp"), VfsNode::Directory {
            children: BTreeMap::new(),
            oid: 500,
        });

        // Chimera compatibility mount
        root_children.insert(String::from("chimera"), VfsNode::Directory {
            children: {
                let mut c = BTreeMap::new();
                // Virtual C:\ drive for legacy apps
                c.insert(String::from("C"), VfsNode::Mount {
                    device: String::from("chimera-disk0"),
                    root: 1000,
                });
                c
            },
            oid: 600,
        });

        Vfs {
            root: VfsNode::Directory {
                children: root_children,
                oid: 0,
            },
            mounts: BTreeMap::new(),
        }
    }

    /// Resolve a path to a VFS node.
    pub fn resolve(&self, path: &str) -> Result<&VfsNode, VfsError> {
        if path == "/" || path.is_empty() {
            return Ok(&self.root);
        }

        let parts: Vec<&str> = path.trim_start_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();

        let mut current = &self.root;
        for part in &parts {
            match current {
                VfsNode::Directory { children, .. } => {
                    current = children.get(*part).ok_or(VfsError::NotFound)?;
                }
                VfsNode::Symlink { target } => {
                    // Would recursively resolve the symlink target
                    let _ = target;
                    return Err(VfsError::NotFound);
                }
                _ => return Err(VfsError::NotADirectory),
            }
        }

        Ok(current)
    }

    /// Create a directory at the given path.
    pub fn mkdir(&mut self, path: &str, oid: u64) -> Result<(), VfsError> {
        let parts: Vec<&str> = path.trim_start_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();

        if parts.is_empty() {
            return Err(VfsError::InvalidPath);
        }

        let (parent_parts, name) = parts.split_at(parts.len() - 1);
        let name = name[0];

        // Navigate to parent
        let mut current = &mut self.root;
        for part in parent_parts {
            match current {
                VfsNode::Directory { children, .. } => {
                    current = children.get_mut(*part).ok_or(VfsError::NotFound)?;
                }
                _ => return Err(VfsError::NotADirectory),
            }
        }

        // Create the new directory
        match current {
            VfsNode::Directory { children, .. } => {
                if children.contains_key(name) {
                    return Err(VfsError::AlreadyExists);
                }
                children.insert(String::from(name), VfsNode::Directory {
                    children: BTreeMap::new(),
                    oid,
                });
                Ok(())
            }
            _ => Err(VfsError::NotADirectory),
        }
    }

    /// Create a file entry at the given path.
    pub fn create_file(&mut self, path: &str, oid: u64, size: u64) -> Result<(), VfsError> {
        let parts: Vec<&str> = path.trim_start_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();

        if parts.is_empty() {
            return Err(VfsError::InvalidPath);
        }

        let (parent_parts, name) = parts.split_at(parts.len() - 1);
        let name = name[0];

        let mut current = &mut self.root;
        for part in parent_parts {
            match current {
                VfsNode::Directory { children, .. } => {
                    current = children.get_mut(*part).ok_or(VfsError::NotFound)?;
                }
                _ => return Err(VfsError::NotADirectory),
            }
        }

        match current {
            VfsNode::Directory { children, .. } => {
                children.insert(String::from(name), VfsNode::File { oid, size });
                Ok(())
            }
            _ => Err(VfsError::NotADirectory),
        }
    }

    /// List directory contents.
    pub fn readdir(&self, path: &str) -> Result<Vec<(String, bool)>, VfsError> {
        match self.resolve(path)? {
            VfsNode::Directory { children, .. } => {
                Ok(children.iter().map(|(name, node)| {
                    let is_dir = matches!(node, VfsNode::Directory { .. } | VfsNode::Mount { .. });
                    (name.clone(), is_dir)
                }).collect())
            }
            _ => Err(VfsError::NotADirectory),
        }
    }

    /// Delete a file or empty directory.
    pub fn remove(&mut self, path: &str) -> Result<(), VfsError> {
        let parts: Vec<&str> = path.trim_start_matches('/')
            .split('/')
            .filter(|p| !p.is_empty())
            .collect();

        if parts.is_empty() {
            return Err(VfsError::InvalidPath);
        }

        let (parent_parts, name) = parts.split_at(parts.len() - 1);
        let name = name[0];

        let mut current = &mut self.root;
        for part in parent_parts {
            match current {
                VfsNode::Directory { children, .. } => {
                    current = children.get_mut(*part).ok_or(VfsError::NotFound)?;
                }
                _ => return Err(VfsError::NotADirectory),
            }
        }

        match current {
            VfsNode::Directory { children, .. } => {
                // Check if target is non-empty directory
                if let Some(VfsNode::Directory { children: sub, .. }) = children.get(name) {
                    if !sub.is_empty() {
                        return Err(VfsError::MountPointBusy); // Not empty
                    }
                }
                children.remove(name).ok_or(VfsError::NotFound)?;
                Ok(())
            }
            _ => Err(VfsError::NotADirectory),
        }
    }

    /// Translate a Win32 path to a Qindows VFS path.
    pub fn translate_win32_path(path: &str) -> String {
        // C:\Users\John\Documents → /chimera/C/Users/John/Documents
        let normalized = path.replace('\\', "/");
        if normalized.len() >= 2 && normalized.as_bytes()[1] == b':' {
            let drive = &normalized[0..1];
            let rest = &normalized[2..];
            alloc::format!("/chimera/{}{}", drive, rest)
        } else {
            normalized
        }
    }
}
