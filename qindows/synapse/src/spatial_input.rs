//! # Spatial Input — Gesture, Gaze & Hand Tracking
//!
//! Unified input pipeline for spatial computing (Section 4.5).
//! Combines data from cameras, LiDAR, and eye-tracking sensors
//! into a coherent interaction model.
//!
//! Input sources:
//! - **Hands**: Finger pinch, grab, swipe, point
//! - **Gaze**: Eye position, dwell-to-select
//! - **Head**: Orientation (6DOF)
//! - **Voice**: Integrated with Synapse voice module

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Input source type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputSource {
    LeftHand,
    RightHand,
    Gaze,
    Head,
    Controller,
}

/// Gesture type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    Pinch,
    Grab,
    Release,
    Swipe,
    Point,
    Tap,
    DoubleTap,
    Rotate,
    Scale,
    None,
}

/// A 3D position.
#[derive(Debug, Clone, Copy, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// A spatial input event.
#[derive(Debug, Clone)]
pub struct SpatialEvent {
    pub id: u64,
    pub source: InputSource,
    pub gesture: Gesture,
    pub position: Vec3,
    pub direction: Vec3,
    pub confidence: f32,
    pub timestamp: u64,
}

/// Gaze state.
#[derive(Debug, Clone)]
pub struct GazeState {
    pub position: Vec3,
    pub target_id: Option<u64>,
    pub dwell_start: u64,
    pub dwell_threshold_ms: u64,
    pub dwelling: bool,
}

/// Hand tracking state.
#[derive(Debug, Clone)]
pub struct HandState {
    pub source: InputSource,
    pub position: Vec3,
    pub fingers: [Vec3; 5],
    pub gesture: Gesture,
    pub grip_strength: f32,
    pub visible: bool,
}

/// Spatial input statistics.
#[derive(Debug, Clone, Default)]
pub struct InputStats {
    pub events_processed: u64,
    pub gestures_recognized: u64,
    pub gaze_selections: u64,
    pub hand_tracks: u64,
}

/// The Spatial Input Manager.
pub struct SpatialInput {
    pub hands: BTreeMap<u8, HandState>,
    pub gaze: GazeState,
    pub event_queue: Vec<SpatialEvent>,
    next_event_id: u64,
    pub max_queue: usize,
    pub stats: InputStats,
}

impl SpatialInput {
    pub fn new() -> Self {
        SpatialInput {
            hands: BTreeMap::new(),
            gaze: GazeState {
                position: Vec3::default(),
                target_id: None,
                dwell_start: 0,
                dwell_threshold_ms: 800,
                dwelling: false,
            },
            event_queue: Vec::new(),
            next_event_id: 1,
            max_queue: 256,
            stats: InputStats::default(),
        }
    }

    /// Update hand tracking data.
    pub fn update_hand(&mut self, source: InputSource, pos: Vec3, fingers: [Vec3; 5], gesture: Gesture, grip: f32, now: u64) {
        let idx = match source { InputSource::LeftHand => 0, _ => 1 };

        self.hands.insert(idx, HandState {
            source, position: pos, fingers, gesture, grip_strength: grip, visible: true,
        });

        if gesture != Gesture::None {
            self.push_event(source, gesture, pos, Vec3::default(), 1.0, now);
            self.stats.gestures_recognized += 1;
        }

        self.stats.hand_tracks += 1;
    }

    /// Update gaze position.
    pub fn update_gaze(&mut self, pos: Vec3, target: Option<u64>, now: u64) {
        let same_target = self.gaze.target_id == target && target.is_some();

        if same_target {
            let dwell_ms = now.saturating_sub(self.gaze.dwell_start);
            if dwell_ms >= self.gaze.dwell_threshold_ms && !self.gaze.dwelling {
                self.gaze.dwelling = true;
                self.push_event(InputSource::Gaze, Gesture::Tap, pos, Vec3::default(), 0.95, now);
                self.stats.gaze_selections += 1;
            }
        } else {
            self.gaze.dwell_start = now;
            self.gaze.dwelling = false;
        }

        self.gaze.position = pos;
        self.gaze.target_id = target;
    }

    /// Push a spatial event.
    fn push_event(&mut self, source: InputSource, gesture: Gesture, pos: Vec3, dir: Vec3, conf: f32, now: u64) {
        let id = self.next_event_id;
        self.next_event_id += 1;

        if self.event_queue.len() >= self.max_queue {
            self.event_queue.remove(0);
        }

        self.event_queue.push(SpatialEvent {
            id, source, gesture, position: pos, direction: dir, confidence: conf, timestamp: now,
        });
        self.stats.events_processed += 1;
    }

    /// Drain the event queue.
    pub fn drain_events(&mut self) -> Vec<SpatialEvent> {
        let events = core::mem::take(&mut self.event_queue);
        events
    }
}
