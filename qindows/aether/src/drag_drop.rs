//! # Aether Drag-and-Drop Engine
//!
//! Handles drag-and-drop interactions across windows, the Prism
//! explorer, and Q-Shell. Supports:
//! - Intra-window dragging (list reorder, widget move)
//! - Inter-window dragging (file transfer between Silos)
//! - Shell drops (drag a file into Q-Shell to get its OID)
//! - Live preview during drag (Aether renders a ghost thumbnail)

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// What is being dragged.
#[derive(Debug, Clone)]
pub enum DragPayload {
    /// One or more Prism objects (files, images, etc.)
    Objects(Vec<u64>), // OIDs
    /// Plain text selection
    Text(String),
    /// A UI widget being repositioned
    Widget { window_id: u64, widget_id: u64 },
    /// A window being tiled / rearranged
    Window(u64),
    /// Raw binary data (clipboard-style)
    Binary { mime: String, data: Vec<u8> },
}

/// Drag source information.
#[derive(Debug, Clone)]
pub struct DragSource {
    /// Source Silo ID
    pub silo_id: u64,
    /// Source window ID
    pub window_id: u64,
    /// Start position (logical pixels)
    pub start_x: f32,
    pub start_y: f32,
    /// Allowed operations from source
    pub allowed_ops: DragOps,
}

/// Drag target information.
#[derive(Debug, Clone)]
pub struct DropTarget {
    /// Target Silo ID
    pub silo_id: u64,
    /// Target window ID
    pub window_id: u64,
    /// Target zone (where in the window)
    pub zone: DropZone,
    /// Accepted payload types
    pub accepted: Vec<PayloadKind>,
}

/// What operations a drag source allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragOps {
    pub copy: bool,
    pub r#move: bool,
    pub link: bool,
}

impl DragOps {
    pub fn copy_only() -> Self { DragOps { copy: true, r#move: false, link: false } }
    pub fn move_only() -> Self { DragOps { copy: false, r#move: true, link: false } }
    pub fn all() -> Self { DragOps { copy: true, r#move: true, link: true } }
}

/// Kind of payload for filtering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadKind {
    Objects,
    Text,
    Widget,
    Window,
    Binary,
}

impl DragPayload {
    pub fn kind(&self) -> PayloadKind {
        match self {
            DragPayload::Objects(_) => PayloadKind::Objects,
            DragPayload::Text(_) => PayloadKind::Text,
            DragPayload::Widget { .. } => PayloadKind::Widget,
            DragPayload::Window(_) => PayloadKind::Window,
            DragPayload::Binary { .. } => PayloadKind::Binary,
        }
    }
}

/// Drop zone within a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    /// Entire window accepts drops
    Whole,
    /// Specific rectangle (x, y, w, h)
    Rect(i32, i32, i32, i32),
    /// Top/bottom/left/right edge (for tiling)
    Edge(Edge),
}

/// Window edge for tiling drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

/// Current drag state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    /// No drag active
    Idle,
    /// Drag started (threshold met)
    Dragging,
    /// Over a valid drop target
    OverTarget,
    /// Over an invalid zone
    OverInvalid,
    /// Drop accepted
    Dropped,
    /// Drag cancelled (Escape pressed or left valid area)
    Cancelled,
}

/// Result of a drag operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropEffect {
    None,
    Copy,
    Move,
    Link,
}

/// A live drag-and-drop session.
#[derive(Debug)]
pub struct DragSession {
    /// Current state
    pub state: DragState,
    /// What's being dragged
    pub payload: DragPayload,
    /// Source info
    pub source: DragSource,
    /// Current cursor position
    pub cursor_x: f32,
    pub cursor_y: f32,
    /// Current hover target (if any)
    pub hover_target: Option<DropTarget>,
    /// Visual feedback opacity (0.0–1.0)
    pub ghost_opacity: f32,
    /// Drag distance threshold was met
    pub threshold_met: bool,
}

/// Minimum pixels to move before a drag begins.
const DRAG_THRESHOLD: f32 = 5.0;

/// The Drag-and-Drop Engine.
pub struct DragDropEngine {
    /// Active drag session (at most one at a time)
    pub session: Option<DragSession>,
    /// Registered drop targets
    pub targets: Vec<DropTarget>,
    /// Stats
    pub total_drags: u64,
    pub total_drops: u64,
    pub total_cancelled: u64,
}

impl DragDropEngine {
    pub fn new() -> Self {
        DragDropEngine {
            session: None,
            targets: Vec::new(),
            total_drags: 0,
            total_drops: 0,
            total_cancelled: 0,
        }
    }

    /// Register a drop target.
    pub fn register_target(&mut self, target: DropTarget) {
        self.targets.push(target);
    }

    /// Unregister all targets for a window.
    pub fn unregister_window(&mut self, window_id: u64) {
        self.targets.retain(|t| t.window_id != window_id);
    }

    /// Begin a drag operation (called on mouse-down + move).
    pub fn begin_drag(
        &mut self,
        payload: DragPayload,
        source: DragSource,
    ) {
        self.session = Some(DragSession {
            state: DragState::Idle,
            payload,
            cursor_x: source.start_x,
            cursor_y: source.start_y,
            source,
            hover_target: None,
            ghost_opacity: 0.7,
            threshold_met: false,
        });
        self.total_drags += 1;
    }

    /// Update drag position (called on mouse-move).
    pub fn update(&mut self, x: f32, y: f32) {
        let session = match self.session.as_mut() {
            Some(s) => s,
            None => return,
        };

        session.cursor_x = x;
        session.cursor_y = y;

        // Check drag threshold
        if !session.threshold_met {
            let dx = x - session.source.start_x;
            let dy = y - session.source.start_y;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist >= DRAG_THRESHOLD {
                session.threshold_met = true;
                session.state = DragState::Dragging;
            } else {
                return;
            }
        }

        // Hit-test against registered drop targets
        session.hover_target = None;
        session.state = DragState::Dragging;

        // Find the best matching target
        // (targets are checked in reverse order — last registered wins)
        for target in self.targets.iter().rev() {
            if self.hit_test(target, x, y) {
                let payload_kind = session.payload.kind();
                if target.accepted.contains(&payload_kind) {
                    session.hover_target = Some(target.clone());
                    session.state = DragState::OverTarget;
                } else {
                    session.state = DragState::OverInvalid;
                }
                break;
            }
        }
    }

    /// Complete the drop (called on mouse-up).
    pub fn drop_it(&mut self) -> Option<(DragPayload, DropTarget, DropEffect)> {
        let session = self.session.take()?;

        if session.state != DragState::OverTarget {
            self.total_cancelled += 1;
            return None;
        }

        let target = session.hover_target?;

        // Determine the effect based on source allowed ops
        let effect = if session.source.allowed_ops.r#move {
            DropEffect::Move
        } else if session.source.allowed_ops.copy {
            DropEffect::Copy
        } else if session.source.allowed_ops.link {
            DropEffect::Link
        } else {
            DropEffect::None
        };

        self.total_drops += 1;
        Some((session.payload, target, effect))
    }

    /// Cancel the current drag.
    pub fn cancel(&mut self) {
        if self.session.take().is_some() {
            self.total_cancelled += 1;
        }
    }

    /// Is a drag currently active?
    pub fn is_dragging(&self) -> bool {
        self.session.as_ref().map_or(false, |s| s.threshold_met)
    }

    /// Hit-test a drop target.
    fn hit_test(&self, target: &DropTarget, x: f32, y: f32) -> bool {
        match target.zone {
            DropZone::Whole => true, // Simplified — would check window bounds
            DropZone::Rect(rx, ry, rw, rh) => {
                x >= rx as f32 && x < (rx + rw) as f32
                    && y >= ry as f32 && y < (ry + rh) as f32
            }
            DropZone::Edge(_) => {
                // Edge zones are thin strips along window borders
                // Simplified: always match
                true
            }
        }
    }
}
