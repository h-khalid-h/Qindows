//! # Aether File Picker Dialog
//!
//! Native open/save file dialog for the Qindows desktop.
//! Integrates with Prism VFS for path resolution and provides
//! navigation, filtering, sorting, favorites, and in-directory search.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// File picker mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerMode {
    /// Select a single file to open
    OpenFile,
    /// Select multiple files to open
    OpenMultiple,
    /// Select a directory
    OpenFolder,
    /// Save a file (requires filename input)
    SaveFile,
}

/// Sorting criteria.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortBy {
    Name,
    Size,
    DateModified,
    FileType,
}

/// Sort direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Ascending,
    Descending,
}

/// View style for the file list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewStyle {
    /// Detailed list with columns
    List,
    /// Large icon grid
    Icons,
    /// Compact list
    Compact,
    /// Thumbnail previews
    Thumbnails,
}

/// A file type filter (e.g., "Images (*.png, *.jpg)").
#[derive(Debug, Clone)]
pub struct FileFilter {
    /// Display label
    pub label: String,
    /// Extensions (without dots)
    pub extensions: Vec<String>,
}

impl FileFilter {
    pub fn new(label: &str, extensions: &[&str]) -> Self {
        FileFilter {
            label: String::from(label),
            extensions: extensions.iter().map(|e| String::from(*e)).collect(),
        }
    }

    /// Check if a filename matches this filter.
    pub fn matches(&self, filename: &str) -> bool {
        if self.extensions.is_empty() { return true; } // "All Files"
        let lower = filename.to_ascii_lowercase();
        self.extensions.iter().any(|ext| {
            lower.ends_with(&alloc::format!(".{}", ext.to_ascii_lowercase()))
        })
    }

    /// Display string (e.g., "Images (*.png, *.jpg)").
    pub fn display(&self) -> String {
        if self.extensions.is_empty() {
            return self.label.clone();
        }
        let exts: Vec<String> = self.extensions.iter()
            .map(|e| alloc::format!("*.{}", e))
            .collect();
        alloc::format!("{} ({})", self.label, exts.join(", "))
    }
}

/// A file/directory entry in the picker.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// Name (just the filename, not full path)
    pub name: String,
    /// Is this a directory?
    pub is_dir: bool,
    /// File size in bytes (0 for directories)
    pub size: u64,
    /// Last modified timestamp (ns since boot)
    pub modified: u64,
    /// File extension (lowercase, without dot)
    pub extension: String,
    /// Prism object ID
    pub oid: u64,
    /// Is this entry currently selected?
    pub selected: bool,
    /// Is this a hidden file (starts with '.')?
    pub hidden: bool,
}

impl FileEntry {
    /// Human-readable size string.
    pub fn size_display(&self) -> String {
        if self.is_dir { return String::from("—"); }
        if self.size < 1024 {
            alloc::format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            alloc::format!("{:.1} KB", self.size as f64 / 1024.0)
        } else if self.size < 1024 * 1024 * 1024 {
            alloc::format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
        } else {
            alloc::format!("{:.2} GB", self.size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Type display string.
    pub fn type_display(&self) -> String {
        if self.is_dir { return String::from("Folder"); }
        match self.extension.as_str() {
            "txt" => String::from("Text Document"),
            "rs"  => String::from("Rust Source"),
            "png" | "jpg" | "jpeg" | "gif" | "bmp" => String::from("Image"),
            "mp3" | "wav" | "flac" | "ogg" => String::from("Audio"),
            "mp4" | "avi" | "mkv" | "mov" => String::from("Video"),
            "pdf" => String::from("PDF Document"),
            "zip" | "tar" | "gz" | "7z" => String::from("Archive"),
            "exe" | "dll" => String::from("Win32 Binary"),
            "" => String::from("File"),
            ext => alloc::format!("{} File", ext.to_ascii_uppercase()),
        }
    }
}

/// A sidebar favorite/bookmark.
#[derive(Debug, Clone)]
pub struct Favorite {
    /// Display label
    pub label: String,
    /// Absolute path
    pub path: String,
    /// Icon name
    pub icon: String,
}

/// Picker result.
#[derive(Debug, Clone)]
pub enum PickerResult {
    /// User selected file(s)
    Selected(Vec<String>),
    /// User cancelled
    Cancelled,
}

/// The File Picker Dialog.
pub struct FilePicker {
    /// Picker mode
    pub mode: PickerMode,
    /// Current directory path
    pub current_path: String,
    /// Directory entries (after filtering + sorting)
    pub entries: Vec<FileEntry>,
    /// Navigation history (for back/forward)
    pub history: Vec<String>,
    /// History cursor (index into history)
    pub history_cursor: usize,
    /// File type filters
    pub filters: Vec<FileFilter>,
    /// Active filter index
    pub active_filter: usize,
    /// Sort criteria
    pub sort_by: SortBy,
    /// Sort direction
    pub sort_dir: SortDir,
    /// View style
    pub view_style: ViewStyle,
    /// Sidebar favorites
    pub favorites: Vec<Favorite>,
    /// Filename input buffer (for Save mode)
    pub filename: String,
    /// In-directory search query
    pub search_query: String,
    /// Show hidden files?
    pub show_hidden: bool,
    /// Is the dialog visible?
    pub visible: bool,
    /// Title string
    pub title: String,
    /// Selected entries count
    pub selection_count: usize,
    /// Dialog owner (window/widget ID)
    pub owner_id: u64,
    /// Stats
    pub total_entries: usize,
    pub filtered_entries: usize,
}

impl FilePicker {
    pub fn new(mode: PickerMode) -> Self {
        let title = match mode {
            PickerMode::OpenFile     => "Open File",
            PickerMode::OpenMultiple => "Open Files",
            PickerMode::OpenFolder   => "Choose Folder",
            PickerMode::SaveFile     => "Save As",
        };

        FilePicker {
            mode,
            current_path: String::from("/users"),
            entries: Vec::new(),
            history: alloc::vec![String::from("/users")],
            history_cursor: 0,
            filters: alloc::vec![
                FileFilter::new("All Files", &[]),
            ],
            active_filter: 0,
            sort_by: SortBy::Name,
            sort_dir: SortDir::Ascending,
            view_style: ViewStyle::List,
            favorites: default_favorites(),
            filename: String::new(),
            search_query: String::new(),
            show_hidden: false,
            visible: false,
            title: String::from(title),
            selection_count: 0,
            owner_id: 0,
            total_entries: 0,
            filtered_entries: 0,
        }
    }

    /// Show the dialog.
    pub fn show(&mut self, owner_id: u64) {
        self.owner_id = owner_id;
        self.visible = true;
        self.selection_count = 0;
    }

    /// Hide/cancel the dialog.
    pub fn cancel(&mut self) -> PickerResult {
        self.visible = false;
        PickerResult::Cancelled
    }

    /// Navigate to a directory.
    pub fn navigate_to(&mut self, path: &str) {
        // Trim history ahead of cursor (discard forward history)
        self.history.truncate(self.history_cursor + 1);
        self.current_path = String::from(path);
        self.history.push(String::from(path));
        self.history_cursor = self.history.len() - 1;
        self.search_query.clear();
        self.deselect_all();
    }

    /// Navigate back.
    pub fn go_back(&mut self) -> bool {
        if self.history_cursor > 0 {
            self.history_cursor -= 1;
            self.current_path = self.history[self.history_cursor].clone();
            self.deselect_all();
            true
        } else {
            false
        }
    }

    /// Navigate forward.
    pub fn go_forward(&mut self) -> bool {
        if self.history_cursor + 1 < self.history.len() {
            self.history_cursor += 1;
            self.current_path = self.history[self.history_cursor].clone();
            self.deselect_all();
            true
        } else {
            false
        }
    }

    /// Navigate to parent directory.
    pub fn go_up(&mut self) -> bool {
        if self.current_path == "/" { return false; }
        let parent = if let Some(idx) = self.current_path.rfind('/') {
            if idx == 0 { "/" } else { &self.current_path[..idx] }
        } else {
            "/"
        };
        let parent = String::from(parent);
        self.navigate_to(&parent);
        true
    }

    /// Handle double-click on an entry.
    pub fn activate_entry(&mut self, index: usize) -> Option<PickerResult> {
        if index >= self.entries.len() { return None; }

        if self.entries[index].is_dir {
            let name = self.entries[index].name.clone();
            let new_path = if self.current_path == "/" {
                alloc::format!("/{}", name)
            } else {
                alloc::format!("{}/{}", self.current_path, name)
            };
            self.navigate_to(&new_path);
            None
        } else {
            // Double-click file = select and confirm
            match self.mode {
                PickerMode::OpenFile | PickerMode::OpenMultiple => {
                    let full_path = alloc::format!("{}/{}", self.current_path, self.entries[index].name);
                    self.visible = false;
                    Some(PickerResult::Selected(alloc::vec![full_path]))
                }
                PickerMode::SaveFile => {
                    self.filename = self.entries[index].name.clone();
                    None // User needs to confirm filename
                }
                PickerMode::OpenFolder => None, // Can't select files in folder mode
            }
        }
    }

    /// Toggle selection of an entry.
    pub fn toggle_select(&mut self, index: usize) {
        if index >= self.entries.len() { return; }

        if self.mode != PickerMode::OpenMultiple {
            // Single selection — deselect all first
            self.deselect_all();
        }

        self.entries[index].selected = !self.entries[index].selected;
        self.selection_count = self.entries.iter().filter(|e| e.selected).count();
    }

    /// Deselect all entries.
    fn deselect_all(&mut self) {
        for entry in &mut self.entries {
            entry.selected = false;
        }
        self.selection_count = 0;
    }

    /// Confirm the selection (OK button).
    pub fn confirm(&mut self) -> PickerResult {
        self.visible = false;

        match self.mode {
            PickerMode::SaveFile => {
                if self.filename.is_empty() {
                    return PickerResult::Cancelled;
                }
                let path = alloc::format!("{}/{}", self.current_path, self.filename);
                PickerResult::Selected(alloc::vec![path])
            }
            PickerMode::OpenFolder => {
                PickerResult::Selected(alloc::vec![self.current_path.clone()])
            }
            PickerMode::OpenFile | PickerMode::OpenMultiple => {
                let selected: Vec<String> = self.entries.iter()
                    .filter(|e| e.selected)
                    .map(|e| alloc::format!("{}/{}", self.current_path, e.name))
                    .collect();
                if selected.is_empty() {
                    PickerResult::Cancelled
                } else {
                    PickerResult::Selected(selected)
                }
            }
        }
    }

    /// Set the active file filter.
    pub fn set_filter(&mut self, index: usize) {
        if index < self.filters.len() {
            self.active_filter = index;
            self.apply_filter();
        }
    }

    /// Add a file filter.
    pub fn add_filter(&mut self, filter: FileFilter) {
        self.filters.push(filter);
    }

    /// Apply the current filter + search to entries.
    pub fn apply_filter(&mut self) {
        let filter = self.filters.get(self.active_filter);
        let search = self.search_query.to_ascii_lowercase();

        self.filtered_entries = 0;
        for entry in &mut self.entries {
            let mut visible = true;

            // Hidden filter
            if entry.hidden && !self.show_hidden {
                visible = false;
            }

            // Extension filter (directories always visible)
            if visible && !entry.is_dir {
                if let Some(f) = filter {
                    if !f.extensions.is_empty() && !f.matches(&entry.name) {
                        visible = false;
                    }
                }
            }

            // Search filter
            if visible && !search.is_empty() {
                if !entry.name.to_ascii_lowercase().contains(&search) {
                    visible = false;
                }
            }

            if visible {
                self.filtered_entries += 1;
            }
        }
    }

    /// Set sort criteria and re-sort.
    pub fn set_sort(&mut self, sort_by: SortBy, dir: SortDir) {
        self.sort_by = sort_by;
        self.sort_dir = dir;
        self.sort_entries();
    }

    /// Sort entries by current criteria (directories first).
    pub fn sort_entries(&mut self) {
        let sort_by = self.sort_by;
        let ascending = self.sort_dir == SortDir::Ascending;

        self.entries.sort_by(|a, b| {
            // Directories always first
            if a.is_dir != b.is_dir {
                return if a.is_dir { core::cmp::Ordering::Less } else { core::cmp::Ordering::Greater };
            }

            let ord = match sort_by {
                SortBy::Name => a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()),
                SortBy::Size => a.size.cmp(&b.size),
                SortBy::DateModified => a.modified.cmp(&b.modified),
                SortBy::FileType => a.extension.cmp(&b.extension),
            };

            if ascending { ord } else { ord.reverse() }
        });
    }

    /// Get breadcrumb path segments for navigation bar.
    pub fn breadcrumbs(&self) -> Vec<(String, String)> {
        let mut crumbs = Vec::new();
        let mut current = String::new();

        for part in self.current_path.split('/').filter(|p| !p.is_empty()) {
            current = alloc::format!("{}/{}", current, part);
            crumbs.push((String::from(part), current.clone()));
        }

        if crumbs.is_empty() {
            crumbs.push((String::from("/"), String::from("/")));
        }

        crumbs
    }

    /// Populate entries from Prism VFS data.
    pub fn populate(&mut self, entries: Vec<FileEntry>) {
        self.total_entries = entries.len();
        self.entries = entries;
        self.sort_entries();
        self.apply_filter();
    }
}

/// Default sidebar favorites.
fn default_favorites() -> Vec<Favorite> {
    alloc::vec![
        Favorite { label: String::from("Desktop"),   path: String::from("/users/desktop"),    icon: String::from("desktop") },
        Favorite { label: String::from("Documents"), path: String::from("/users/documents"),  icon: String::from("folder-docs") },
        Favorite { label: String::from("Downloads"), path: String::from("/users/downloads"),  icon: String::from("folder-download") },
        Favorite { label: String::from("Pictures"),  path: String::from("/users/pictures"),   icon: String::from("folder-image") },
        Favorite { label: String::from("Music"),     path: String::from("/users/music"),      icon: String::from("folder-music") },
        Favorite { label: String::from("Apps"),      path: String::from("/apps"),             icon: String::from("grid") },
        Favorite { label: String::from("System"),    path: String::from("/system"),           icon: String::from("gear") },
    ]
}
