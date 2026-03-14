//! # Q-View Window Manager — Multi-Window Tiling Engine (Phase 92)
//!
//! ARCHITECTURE.md §5 — Q-View: Window Management:
//! > "Q-View manages window layout with predictive AI placement"
//! > "Tiling, floating, and full-screen modes"
//! > "Windows remember their last position per Silo binary"
//! > "AI predicts where to place new windows based on usage patterns"
//!
//! ## Architecture Guardian: What was missing
//! `aether.rs` handles SDF rendering. Individual windows are SDF node trees.
//! But no module manages the **spatial layout** of multiple Silo windows:
//! - Which workspace they're on
//! - How they tile relative to each other
//! - Which window has keyboard focus
//! - AI placement prediction based on usage history
//!
//! ## Layout Modes
//! ```text
//! Tiling:                  Floating:              Monocle:
//! ┌──────┬──────┐          ┌─────────────┐        ┌─────────────────┐
//! │  A   │  B   │          │    A   ┌──┐ │        │                 │
//! │      │      │          │        │B │ │        │        A        │
//! ├──────┴──────┤          │        └──┘ │        │   (fullscreen)  │
//! │      C      │          └─────────────┘        └─────────────────┘
//! └─────────────┘
//! ```
//!
//! ## AI Placement
//! - `Terminal` type opened right-of-center by default (learned from usage history)
//! - `Browser` type → full screen on primary monitor
//! - `DM/Chat` type → right side panel
//! - Manual overrides stored per (binary_oid, workspace) → learned for next session

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

// ── Monitor Layout ────────────────────────────────────────────────────────────

/// A physical display monitor.
#[derive(Debug, Clone, Copy)]
pub struct Monitor {
    pub id: u32,
    pub x: u32, pub y: u32,      // top-left in virtual desktop space
    pub width: u32, pub height: u32,
    pub refresh_hz: u32,
    pub is_primary: bool,
}

// ── Layout Mode ───────────────────────────────────────────────────────────────

/// How windows are arranged on a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutMode {
    /// Windows tile perfectly — no overlaps, fill the monitor
    Tiling,
    /// Classic overlapping float
    Floating,
    /// Active window fills entire monitor
    Monocle,
    /// Two-column split: master left + stack right
    MasterStack,
}

impl LayoutMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Tiling      => "[]=",
            Self::Floating    => "><>",
            Self::Monocle     => "[M]",
            Self::MasterStack => "[]|",
        }
    }
}

// ── Window Geometry ───────────────────────────────────────────────────────────

/// Position and size of a window on screen.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub x: f32, pub y: f32,
    pub w: f32, pub h: f32,
}

impl Geometry {
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px < self.x + self.w &&
        py >= self.y && py < self.y + self.h
    }

    pub fn center(&self) -> (f32, f32) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }
}

// ── Window Type ───────────────────────────────────────────────────────────────

/// Semantic type of a Silo window (used for AI placement).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowType {
    Terminal,
    Browser,
    Chat,
    MediaPlayer,
    TextEditor,
    Settings,
    FileManager,
    Dashboard,    // Aether dashboard/status windows
    Dialog,       // Small modal dialogs
    Notification, // Transient notification bubbles
    Generic,
}

impl WindowType {
    /// Default layout mode preference for this window type.
    pub fn preferred_layout(self) -> LayoutMode {
        match self {
            Self::Browser      => LayoutMode::Monocle,
            Self::MediaPlayer  => LayoutMode::Monocle,
            Self::Dialog       => LayoutMode::Floating,
            Self::Notification => LayoutMode::Floating,
            _                  => LayoutMode::Tiling,
        }
    }
}

// ── Window State ──────────────────────────────────────────────────────────────

/// State of one managed window.
#[derive(Debug, Clone)]
pub struct QViewWindow {
    pub silo_id: u64,
    pub binary_oid: [u8; 32],
    pub window_type: WindowType,
    pub title: String,
    /// Current geometry (screen coordinates, logical pixels)
    pub geometry: Geometry,
    /// Geometry in floating mode (saved when switching to tiling)
    pub float_geometry: Geometry,
    pub workspace_id: u32,
    pub monitor_id: u32,
    pub is_focused: bool,
    pub is_minimized: bool,
    pub is_fullscreen: bool,
    pub is_floating: bool,
    /// Z-order (higher = on top) — relevant only in floating mode
    pub z_order: u32,
    /// Opacity (0.0-1.0, for glassmorphism effects)
    pub opacity: f32,
    /// Border highlight (used for focus indicator in tiling)
    pub border_focused_color: u32, // RGBA
    pub border_unfocused_color: u32,
}

impl QViewWindow {
    pub fn new(silo_id: u64, binary_oid: [u8; 32], window_type: WindowType, title: &str) -> Self {
        QViewWindow {
            silo_id,
            binary_oid,
            window_type,
            title: title.to_string(),
            geometry: Geometry { x: 0.0, y: 0.0, w: 800.0, h: 600.0 },
            float_geometry: Geometry { x: 100.0, y: 100.0, w: 800.0, h: 600.0 },
            workspace_id: 0,
            monitor_id: 0,
            is_focused: false,
            is_minimized: false,
            is_fullscreen: false,
            is_floating: false,
            z_order: 0,
            opacity: 1.0,
            border_focused_color:   0x5E81_ACFF,
            border_unfocused_color: 0x2E34_40FF,
        }
    }
}

// ── Workspace ─────────────────────────────────────────────────────────────────

/// A virtual workspace (like virtual desktops).
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: u32,
    pub name: String,
    pub monitor_id: u32,
    pub layout: LayoutMode,
    /// Window IDs on this workspace, in tiling order
    pub window_order: Vec<u64>, // silo_ids
    /// Master pane ratio (tiling/master-stack mode)
    pub master_ratio: f32,
}

impl Workspace {
    pub fn new(id: u32, name: &str, monitor_id: u32) -> Self {
        Workspace {
            id,
            name: name.to_string(),
            monitor_id,
            layout: LayoutMode::Tiling,
            window_order: Vec::new(),
            master_ratio: 0.55,
        }
    }
}

// ── AI Placement History ──────────────────────────────────────────────────────

/// A remembered placement for a (binary, workspace) pair.
#[derive(Debug, Clone, Copy, Default)]
pub struct PlacementMemory {
    pub preferred_x: f32, pub preferred_y: f32,
    pub preferred_w: f32, pub preferred_h: f32,
    pub use_count: u32,
}

// ── Window Manager Statistics ─────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct WmStats {
    pub windows_created: u64,
    pub windows_destroyed: u64,
    pub focus_changes: u64,
    pub layout_recalculations: u64,
    pub ai_placements: u64,
    pub workspace_switches: u64,
}

// ── Q-View Window Manager ─────────────────────────────────────────────────────

/// Multi-window tiling engine with AI placement.
pub struct QViewWm {
    /// All managed windows: silo_id → window
    pub windows: BTreeMap<u64, QViewWindow>,
    /// Workspaces: workspace_id → workspace
    pub workspaces: BTreeMap<u32, Workspace>,
    /// Monitors: monitor_id → monitor
    pub monitors: BTreeMap<u32, Monitor>,
    /// Currently focused Silo ID
    pub focused_silo: Option<u64>,
    /// Current active workspace per monitor
    pub active_workspace: BTreeMap<u32, u32>, // monitor_id → workspace_id
    /// AI placement memory: (binary_oid_key, workspace_id) → PlacementMemory
    pub placement_history: BTreeMap<(u64, u32), PlacementMemory>,
    /// Z-order counter
    next_z: u32,
    /// Statistics
    pub stats: WmStats,
}

impl QViewWm {
    pub fn new() -> Self {
        let mut wm = QViewWm {
            windows: BTreeMap::new(),
            workspaces: BTreeMap::new(),
            monitors: BTreeMap::new(),
            focused_silo: None,
            active_workspace: BTreeMap::new(),
            placement_history: BTreeMap::new(),
            next_z: 1,
            stats: WmStats::default(),
        };
        // Default single monitor + 4 workspaces
        wm.monitors.insert(0, Monitor { id: 0, x: 0, y: 0, width: 1920, height: 1080, refresh_hz: 120, is_primary: true });
        for i in 0..4u32 {
            let name = match i { 0 => "main", 1 => "web", 2 => "dev", _ => "misc" };
            wm.workspaces.insert(i, Workspace::new(i, name, 0));
        }
        wm.active_workspace.insert(0, 0);
        wm
    }

    // ── Window Lifecycle ──────────────────────────────────────────────────────

    /// Map a new window for a Silo. Returns initial geometry (AI-predicted).
    pub fn map_window(
        &mut self,
        silo_id: u64,
        binary_oid: [u8; 32],
        window_type: WindowType,
        title: &str,
    ) -> Geometry {
        let monitor_id = 0u32;
        let workspace_id = *self.active_workspace.get(&monitor_id).unwrap_or(&0);

        let mut window = QViewWindow::new(silo_id, binary_oid, window_type, title);
        window.workspace_id = workspace_id;
        window.monitor_id = monitor_id;
        window.z_order = self.next_z;
        window.is_floating = matches!(window_type,
            WindowType::Dialog | WindowType::Notification | WindowType::Dashboard);
        self.next_z += 1;

        // AI placement: look up placement history
        let oid_key = u64::from_le_bytes([
            binary_oid[0], binary_oid[1], binary_oid[2], binary_oid[3],
            binary_oid[4], binary_oid[5], binary_oid[6], binary_oid[7],
        ]);
        let ai_geom = if let Some(mem) = self.placement_history.get(&(oid_key, workspace_id)) {
            if mem.use_count > 2 {
                self.stats.ai_placements += 1;
                Geometry { x: mem.preferred_x, y: mem.preferred_y, w: mem.preferred_w, h: mem.preferred_h }
            } else {
                self.default_geometry(window_type)
            }
        } else {
            self.placement_history.insert((oid_key, workspace_id), PlacementMemory::default());
            self.default_geometry(window_type)
        };

        window.geometry = ai_geom;
        window.float_geometry = ai_geom;

        // Add to workspace order
        if let Some(ws) = self.workspaces.get_mut(&workspace_id) {
            ws.window_order.push(silo_id);
        }

        let geom = window.geometry;
        self.windows.insert(silo_id, window);
        self.stats.windows_created += 1;

        // Retile the workspace
        self.retile(workspace_id, monitor_id);

        crate::serial_println!(
            "[Q-VIEW WM] Mapped Silo {} ({:?}) @ ({:.0},{:.0} {:.0}×{:.0})",
            silo_id, window_type, geom.x, geom.y, geom.w, geom.h
        );

        // Auto-focus new window
        self.focus(silo_id);
        geom
    }

    /// Unmap a window (Silo vaporized).
    pub fn unmap_window(&mut self, silo_id: u64) {
        if let Some(win) = self.windows.remove(&silo_id) {
            // Save placement for AI learning
            let oid_key = u64::from_le_bytes([
                win.binary_oid[0], win.binary_oid[1], win.binary_oid[2], win.binary_oid[3],
                win.binary_oid[4], win.binary_oid[5], win.binary_oid[6], win.binary_oid[7],
            ]);
            let mem = self.placement_history.entry((oid_key, win.workspace_id))
                .or_insert_with(PlacementMemory::default);
            let g = win.geometry;
            let n = (mem.use_count + 1) as f32;
            mem.preferred_x = mem.preferred_x + (g.x - mem.preferred_x) / n;
            mem.preferred_y = mem.preferred_y + (g.y - mem.preferred_y) / n;
            mem.preferred_w = mem.preferred_w + (g.w - mem.preferred_w) / n;
            mem.preferred_h = mem.preferred_h + (g.h - mem.preferred_h) / n;
            mem.use_count += 1;

            // Remove from workspace order
            if let Some(ws) = self.workspaces.get_mut(&win.workspace_id) {
                ws.window_order.retain(|&id| id != silo_id);
            }

            self.stats.windows_destroyed += 1;
            self.retile(win.workspace_id, win.monitor_id);

            // Move focus to next window
            if self.focused_silo == Some(silo_id) {
                self.focused_silo = None;
                let ws_id = win.workspace_id;
                if let Some(ws) = self.workspaces.get(&ws_id) {
                    if let Some(&next) = ws.window_order.last() {
                        self.focus(next);
                    }
                }
            }
        }
    }

    /// Focus a window.
    pub fn focus(&mut self, silo_id: u64) {
        if let Some(prev) = self.focused_silo {
            if let Some(w) = self.windows.get_mut(&prev) { w.is_focused = false; }
        }
        if let Some(w) = self.windows.get_mut(&silo_id) {
            w.is_focused = true;
            self.focused_silo = Some(silo_id);
            self.stats.focus_changes += 1;
        }
    }

    /// Retile all non-floating windows on a workspace.
    fn retile(&mut self, workspace_id: u32, monitor_id: u32) {
        let monitor = match self.monitors.get(&monitor_id) { Some(m) => *m, None => return };
        let ws = match self.workspaces.get(&workspace_id) { Some(w) => w.clone(), None => return };

        let screen_w = monitor.width as f32;
        let screen_h = monitor.height as f32;
        let gap = 8.0f32; // gap between tiled windows
        let bar_h = 28.0f32; // top status bar

        let tiling_ids: Vec<u64> = ws.window_order.iter()
            .filter(|&&id| {
                self.windows.get(&id)
                    .map(|w| !w.is_floating && !w.is_minimized && !w.is_fullscreen)
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        let n = tiling_ids.len();
        if n == 0 { self.stats.layout_recalculations += 1; return; }

        match ws.layout {
            LayoutMode::Monocle => {
                for &id in &tiling_ids {
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.geometry = Geometry { x: 0.0, y: bar_h, w: screen_w, h: screen_h - bar_h };
                    }
                }
            }
            LayoutMode::Tiling => {
                // Simple even-height rows
                let row_h = (screen_h - bar_h - gap * (n as f32 + 1.0)) / n as f32;
                for (i, &id) in tiling_ids.iter().enumerate() {
                    if let Some(w) = self.windows.get_mut(&id) {
                        w.geometry = Geometry {
                            x: gap,
                            y: bar_h + gap + i as f32 * (row_h + gap),
                            w: screen_w - 2.0 * gap,
                            h: row_h,
                        };
                    }
                }
            }
            LayoutMode::MasterStack => {
                let master_w = screen_w * ws.master_ratio;
                let stack_w  = screen_w - master_w - gap * 3.0;
                let stack_n  = n.saturating_sub(1);
                let stack_h  = if stack_n > 0 {
                    (screen_h - bar_h - gap * (stack_n as f32 + 1.0)) / stack_n as f32
                } else { 0.0 };
                for (i, &id) in tiling_ids.iter().enumerate() {
                    if let Some(w) = self.windows.get_mut(&id) {
                        if i == 0 {
                            w.geometry = Geometry {
                                x: gap, y: bar_h + gap,
                                w: master_w - gap, h: screen_h - bar_h - 2.0 * gap
                            };
                        } else {
                            let si = i - 1;
                            w.geometry = Geometry {
                                x: master_w + gap * 2.0,
                                y: bar_h + gap + si as f32 * (stack_h + gap),
                                w: stack_w,
                                h: stack_h,
                            };
                        }
                    }
                }
            }
            LayoutMode::Floating => {} // floating windows keep their own geometry
        }
        self.stats.layout_recalculations += 1;
    }

    /// Default geometry for a given window type.
    fn default_geometry(&self, wt: WindowType) -> Geometry {
        match wt {
            WindowType::Dialog       => Geometry { x: 560.0, y: 340.0, w: 800.0, h: 400.0 },
            WindowType::Notification => Geometry { x: 1560.0, y: 40.0,  w: 360.0, h: 80.0  },
            WindowType::Terminal     => Geometry { x: 100.0,  y: 100.0, w: 900.0, h: 500.0 },
            WindowType::Browser      => Geometry { x: 0.0,    y: 28.0,  w: 1920.0, h: 1052.0 },
            _                        => Geometry { x: 60.0,   y: 60.0,  w: 800.0, h: 600.0  },
        }
    }

    /// Switch active workspace on a monitor.
    pub fn switch_workspace(&mut self, monitor_id: u32, workspace_id: u32) {
        self.active_workspace.insert(monitor_id, workspace_id);
        self.stats.workspace_switches += 1;
        crate::serial_println!("[Q-VIEW WM] Monitor {} → workspace {}", monitor_id, workspace_id);
    }

    /// Toggle floating state for focused window.
    pub fn toggle_float(&mut self) {
        if let Some(id) = self.focused_silo {
            if let Some(w) = self.windows.get_mut(&id) {
                w.is_floating = !w.is_floating;
                if !w.is_floating { w.geometry = w.float_geometry; }
                let ws_id = w.workspace_id;
                let mon_id = w.monitor_id;
                self.retile(ws_id, mon_id);
            }
        }
    }

    /// Set layout mode for current workspace.
    pub fn set_layout(&mut self, monitor_id: u32, layout: LayoutMode) {
        let ws_id = *self.active_workspace.get(&monitor_id).unwrap_or(&0);
        if let Some(ws) = self.workspaces.get_mut(&ws_id) {
            ws.layout = layout;
            crate::serial_println!("[Q-VIEW WM] Layout → {}", layout.label());
        }
        self.retile(ws_id, monitor_id);
    }

    pub fn print_stats(&self) {
        crate::serial_println!("╔══════════════════════════════════════╗");
        crate::serial_println!("║   Q-View Window Manager (§5)         ║");
        crate::serial_println!("╠══════════════════════════════════════╣");
        crate::serial_println!("║ Windows live:  {:>6}                ║", self.windows.len());
        crate::serial_println!("║ Workspaces:    {:>6}                ║", self.workspaces.len());
        crate::serial_println!("║ AI placements: {:>6}                ║", self.stats.ai_placements);
        crate::serial_println!("║ Focus changes: {:>6}                ║", self.stats.focus_changes);
        crate::serial_println!("║ Layout recalc: {:>6}                ║", self.stats.layout_recalculations);
        crate::serial_println!("╚══════════════════════════════════════╝");
    }
}
