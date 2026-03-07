//! # Aether Drag-and-Drop System
//!
//! Enables drag-and-drop between Qindows apps and the desktop.
//! Supports text, files, images, and custom MIME types.
//! Data is transferred via the IPC message bus with Silo isolation.

extern crate alloc;

use alloc::string::String;
use crate::math_ext::F32Ext;
use alloc::vec::Vec;

/// Drag data format.
#[derive(Debug, Clone)]
pub enum DragFormat {
    /// Plain text
    PlainText(String),
    /// Rich text (HTML)
    RichText(String),
    /// File paths
    Files(Vec<String>),
    /// Image data (raw RGBA)
    Image { width: u32, height: u32, data: Vec<u8> },
    /// URL
    Url(String),
    /// Custom MIME type with binary data
    Custom { mime: String, data: Vec<u8> },
}

/// Drag-and-drop operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragOperation {
    /// No operation allowed
    None,
    /// Copy data
    Copy,
    /// Move data (delete source)
    Move,
    /// Create a link/shortcut
    Link,
}

/// Drag source info.
#[derive(Debug, Clone)]
pub struct DragSource {
    /// Source Silo ID
    pub silo_id: u64,
    /// Source widget ID
    pub widget_id: u64,
    /// Allowed operations
    pub allowed_ops: Vec<DragOperation>,
    /// Data being dragged
    pub data: Vec<DragFormat>,
    /// Drag icon (widget ID to render as cursor icon)
    pub icon_widget: Option<u64>,
}

/// Current drag state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragState {
    /// No drag in progress
    Idle,
    /// Drag started (button pressed, awaiting threshold)
    Pending,
    /// Actively dragging
    Dragging,
    /// Over a valid drop target
    OverTarget,
    /// Drop completed
    Dropped,
    /// Drag cancelled (Escape pressed)
    Cancelled,
}

/// A drop target registration.
#[derive(Debug, Clone)]
pub struct DropTarget {
    /// Target Silo ID
    pub silo_id: u64,
    /// Target widget ID
    pub widget_id: u64,
    /// Accepted formats (MIME types)
    pub accepted_formats: Vec<String>,
    /// Accepted operations
    pub accepted_ops: Vec<DragOperation>,
    /// Hit rectangle
    pub bounds: (f32, f32, f32, f32), // x, y, w, h
}

impl DropTarget {
    /// Check if a point is within this target's bounds.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.bounds.0 && x < self.bounds.0 + self.bounds.2
            && y >= self.bounds.1 && y < self.bounds.1 + self.bounds.3
    }

    /// Check if this target accepts a given format.
    pub fn accepts(&self, format: &DragFormat) -> bool {
        let mime = match format {
            DragFormat::PlainText(_) => "text/plain",
            DragFormat::RichText(_) => "text/html",
            DragFormat::Files(_) => "application/x-file-list",
            DragFormat::Image { .. } => "image/rgba",
            DragFormat::Url(_) => "text/uri-list",
            DragFormat::Custom { mime, .. } => mime.as_str(),
        };
        self.accepted_formats.iter().any(|f| f == mime || f == "*/*")
    }
}

/// Drag-and-drop event for callbacks.
#[derive(Debug, Clone)]
pub enum DndEvent {
    /// Drag entered a target
    DragEnter { target_widget: u64, x: f32, y: f32 },
    /// Drag is moving over a target
    DragOver { target_widget: u64, x: f32, y: f32, operation: DragOperation },
    /// Drag left a target
    DragLeave { target_widget: u64 },
    /// Drop occurred
    Drop { target_widget: u64, source_silo: u64, data: Vec<DragFormat>, operation: DragOperation },
    /// Drag cancelled
    Cancelled,
}

/// The Drag-and-Drop Manager.
pub struct DndManager {
    /// Current drag state
    pub state: DragState,
    /// Active drag source
    pub source: Option<DragSource>,
    /// Registered drop targets
    pub targets: Vec<DropTarget>,
    /// Currently hovered target
    pub hover_target: Option<u64>,
    /// Current cursor position during drag
    pub cursor_x: f32,
    pub cursor_y: f32,
    /// Drag start position (for threshold detection)
    pub start_x: f32,
    pub start_y: f32,
    /// Minimum distance before drag starts (pixels)
    pub drag_threshold: f32,
    /// Pending events
    pub events: Vec<DndEvent>,
    /// Stats
    pub stats: DndStats,
}

/// Drag-and-drop statistics.
#[derive(Debug, Clone, Default)]
pub struct DndStats {
    pub drags_started: u64,
    pub drops_completed: u64,
    pub drags_cancelled: u64,
    pub cross_silo_drops: u64,
}

impl DndManager {
    pub fn new() -> Self {
        DndManager {
            state: DragState::Idle,
            source: None,
            targets: Vec::new(),
            hover_target: None,
            cursor_x: 0.0,
            cursor_y: 0.0,
            start_x: 0.0,
            start_y: 0.0,
            drag_threshold: 5.0,
            events: Vec::new(),
            stats: DndStats::default(),
        }
    }

    /// Register a drop target.
    pub fn register_target(&mut self, target: DropTarget) {
        self.targets.push(target);
    }

    /// Unregister a drop target.
    pub fn unregister_target(&mut self, widget_id: u64) {
        self.targets.retain(|t| t.widget_id != widget_id);
    }

    /// Begin a drag operation (on mouse press).
    pub fn begin_drag(&mut self, source: DragSource, x: f32, y: f32) {
        self.source = Some(source);
        self.state = DragState::Pending;
        self.start_x = x;
        self.start_y = y;
        self.cursor_x = x;
        self.cursor_y = y;
    }

    /// Update cursor position during drag.
    pub fn move_to(&mut self, x: f32, y: f32) {
        self.cursor_x = x;
        self.cursor_y = y;

        // Check threshold
        if self.state == DragState::Pending {
            let dx = x - self.start_x;
            let dy = y - self.start_y;
            if (dx * dx + dy * dy).sqrt() >= self.drag_threshold {
                self.state = DragState::Dragging;
                self.stats.drags_started += 1;
            }
            return;
        }

        if self.state != DragState::Dragging && self.state != DragState::OverTarget {
            return;
        }

        // Hit-test drop targets
        let source = match &self.source {
            Some(s) => s,
            None => return,
        };

        let mut found_target = false;
        for target in &self.targets {
            if target.silo_id == source.silo_id && target.widget_id == source.widget_id {
                continue; // Can't drop on self
            }

            if target.contains(x, y) {
                // Check format compatibility
                let accepts = source.data.iter().any(|d| target.accepts(d));
                if accepts {
                    if self.hover_target != Some(target.widget_id) {
                        // Entering new target
                        if let Some(old) = self.hover_target {
                            self.events.push(DndEvent::DragLeave { target_widget: old });
                        }
                        self.events.push(DndEvent::DragEnter {
                            target_widget: target.widget_id, x, y,
                        });
                        self.hover_target = Some(target.widget_id);
                    }

                    self.state = DragState::OverTarget;
                    self.events.push(DndEvent::DragOver {
                        target_widget: target.widget_id, x, y,
                        operation: DragOperation::Copy,
                    });
                    found_target = true;
                    break;
                }
            }
        }

        if !found_target && self.hover_target.is_some() {
            self.events.push(DndEvent::DragLeave {
                target_widget: self.hover_target.unwrap(),
            });
            self.hover_target = None;
            self.state = DragState::Dragging;
        }
    }

    /// Complete the drop (on mouse release).
    pub fn drop_at(&mut self, _x: f32, _y: f32) {
        if self.state == DragState::OverTarget {
            if let (Some(source), Some(target_id)) = (&self.source, self.hover_target) {
                let is_cross_silo = self.targets.iter()
                    .find(|t| t.widget_id == target_id)
                    .map(|t| t.silo_id != source.silo_id)
                    .unwrap_or(false);

                if is_cross_silo { self.stats.cross_silo_drops += 1; }

                self.events.push(DndEvent::Drop {
                    target_widget: target_id,
                    source_silo: source.silo_id,
                    data: source.data.clone(),
                    operation: DragOperation::Copy,
                });
                self.stats.drops_completed += 1;
            }
            self.state = DragState::Dropped;
        } else {
            self.cancel();
        }

        self.cleanup();
    }

    /// Cancel the current drag.
    pub fn cancel(&mut self) {
        if self.state != DragState::Idle {
            self.events.push(DndEvent::Cancelled);
            self.stats.drags_cancelled += 1;
            self.state = DragState::Cancelled;
            self.cleanup();
        }
    }

    fn cleanup(&mut self) {
        self.source = None;
        self.hover_target = None;
        self.state = DragState::Idle;
    }

    /// Drain pending events.
    pub fn drain_events(&mut self) -> Vec<DndEvent> {
        core::mem::take(&mut self.events)
    }
}
