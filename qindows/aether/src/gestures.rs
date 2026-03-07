//! # Aether Gesture Recognition
//!
//! Touch and trackpad gesture recognition for the Aether compositor.
//! Detects multi-finger gestures (pinch, rotate, swipe, pan) and
//! translates them into window/workspace actions.
//!
//! Gesture state machine:
//!   Idle → Possible → Recognized → Tracking → Completed
//!
//! Integrates with `input.rs` for raw touch events and
//! `tiling.rs` for workspace switching.

#![allow(dead_code)]

extern crate alloc;

use crate::math_ext::F32Ext;
use alloc::vec::Vec;

// ─── Touch Points ───────────────────────────────────────────────────────────

/// A single touch/finger contact point.
#[derive(Debug, Clone, Copy)]
pub struct TouchPoint {
    /// Finger/contact ID
    pub id: u32,
    /// X position (pixels, f32 for sub-pixel precision)
    pub x: f32,
    /// Y position
    pub y: f32,
    /// Contact pressure (0.0 – 1.0)
    pub pressure: f32,
    /// Contact area (radius in pixels)
    pub radius: f32,
    /// Timestamp (ns)
    pub timestamp: u64,
}

/// Touch event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchEventType {
    /// Finger touched the surface
    Down,
    /// Finger moved
    Move,
    /// Finger lifted
    Up,
    /// Touch cancelled (palm rejection, etc.)
    Cancel,
}

/// A raw touch event.
#[derive(Debug, Clone, Copy)]
pub struct TouchEvent {
    pub event_type: TouchEventType,
    pub point: TouchPoint,
}

// ─── Gesture Types ──────────────────────────────────────────────────────────

/// Recognized gesture types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureType {
    /// Single finger tap
    Tap,
    /// Double tap
    DoubleTap,
    /// Long press
    LongPress,
    /// Two-finger pinch (zoom)
    Pinch,
    /// Two-finger rotation
    Rotate,
    /// Single/multi-finger swipe (directional)
    Swipe(SwipeDirection),
    /// Two-finger pan/scroll
    Pan,
    /// Three-finger swipe (workspace switch)
    ThreeFingerSwipe(SwipeDirection),
    /// Four-finger spread (show desktop)
    FourFingerSpread,
    /// Edge swipe (notification center, task view)
    EdgeSwipe(Edge),
}

/// Swipe directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Screen edges for edge swipes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
}

// ─── Gesture State Machine ─────────────────────────────────────────────────

/// Gesture recognition state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureState {
    /// No gesture active
    Idle,
    /// Touch detected, gesture not yet classified
    Possible,
    /// Gesture type recognized, tracking parameters
    Recognized,
    /// Gesture in progress (continuous: pinch, rotate, pan)
    Tracking,
    /// Gesture completed
    Completed,
    /// Gesture cancelled
    Cancelled,
}

/// A recognized gesture with parameters.
#[derive(Debug, Clone)]
pub struct Gesture {
    /// Gesture type
    pub gesture_type: GestureType,
    /// Current state
    pub state: GestureState,
    /// Number of fingers involved
    pub finger_count: u8,
    /// Center point of the gesture
    pub center_x: f32,
    pub center_y: f32,
    /// Pinch scale factor (1.0 = no change)
    pub scale: f32,
    /// Rotation angle (radians from start)
    pub rotation: f32,
    /// Translation delta from start
    pub delta_x: f32,
    pub delta_y: f32,
    /// Velocity (pixels/sec)
    pub velocity_x: f32,
    pub velocity_y: f32,
    /// Start timestamp
    pub started_at: u64,
    /// Duration (ns)
    pub duration: u64,
}

// ─── Gesture Configuration ─────────────────────────────────────────────────

/// Gesture recognition thresholds.
#[derive(Debug, Clone)]
pub struct GestureConfig {
    /// Minimum distance (px) before a swipe is recognized
    pub swipe_threshold: f32,
    /// Minimum velocity (px/s) for a swipe
    pub swipe_velocity: f32,
    /// Maximum time (ns) for a tap
    pub tap_max_duration_ns: u64,
    /// Maximum time between taps for a double-tap (ns)
    pub double_tap_interval_ns: u64,
    /// Minimum time (ns) for a long press
    pub long_press_min_ns: u64,
    /// Minimum scale change for pinch detection
    pub pinch_threshold: f32,
    /// Minimum rotation (radians) for rotate detection
    pub rotation_threshold: f32,
    /// Edge swipe zone width (px from screen edge)
    pub edge_zone: f32,
    /// Screen width (for edge detection)
    pub screen_width: f32,
    /// Screen height
    pub screen_height: f32,
}

impl Default for GestureConfig {
    fn default() -> Self {
        GestureConfig {
            swipe_threshold: 50.0,
            swipe_velocity: 200.0,
            tap_max_duration_ns: 300_000_000, // 300ms
            double_tap_interval_ns: 400_000_000, // 400ms
            long_press_min_ns: 500_000_000, // 500ms
            pinch_threshold: 0.1,
            rotation_threshold: 0.15, // ~8.6 degrees
            edge_zone: 20.0,
            screen_width: 1920.0,
            screen_height: 1080.0,
        }
    }
}

// ─── Gesture Recognizer ─────────────────────────────────────────────────────

/// Gesture recognizer statistics.
#[derive(Debug, Clone, Default)]
pub struct GestureStats {
    pub total_events: u64,
    pub gestures_recognized: u64,
    pub taps: u64,
    pub swipes: u64,
    pub pinches: u64,
    pub rotations: u64,
}

/// The Gesture Recognizer.
pub struct GestureRecognizer {
    /// Active touch points
    pub active_touches: Vec<TouchPoint>,
    /// Initial touch points (at finger-down)
    pub initial_touches: Vec<TouchPoint>,
    /// Current gesture (if any)
    pub current_gesture: Option<Gesture>,
    /// Last tap timestamp (for double-tap detection)
    pub last_tap_time: u64,
    /// Configuration
    pub config: GestureConfig,
    /// Statistics
    pub stats: GestureStats,
}

impl GestureRecognizer {
    pub fn new(config: GestureConfig) -> Self {
        GestureRecognizer {
            active_touches: Vec::new(),
            initial_touches: Vec::new(),
            current_gesture: None,
            last_tap_time: 0,
            config,
            stats: GestureStats::default(),
        }
    }

    /// Process a raw touch event.
    pub fn process_event(&mut self, event: TouchEvent) -> Option<Gesture> {
        self.stats.total_events += 1;

        match event.event_type {
            TouchEventType::Down => {
                self.on_touch_down(event.point);
                None
            }
            TouchEventType::Move => {
                self.on_touch_move(event.point)
            }
            TouchEventType::Up => {
                self.on_touch_up(event.point)
            }
            TouchEventType::Cancel => {
                self.cancel();
                None
            }
        }
    }

    /// Handle finger down.
    fn on_touch_down(&mut self, point: TouchPoint) {
        self.active_touches.push(point);
        self.initial_touches.push(point);
    }

    /// Handle finger move.
    fn on_touch_move(&mut self, point: TouchPoint) -> Option<Gesture> {
        // Update existing touch
        if let Some(touch) = self.active_touches.iter_mut().find(|t| t.id == point.id) {
            *touch = point;
        }

        let count = self.active_touches.len();
        if count == 0 { return None; }

        // Two-finger gestures: pinch / rotate / pan
        if count == 2 {
            return self.detect_two_finger(point.timestamp);
        }

        // Three-finger swipe
        if count == 3 {
            return self.detect_multi_finger_swipe(3, point.timestamp);
        }

        // Four-finger spread
        if count == 4 {
            return self.detect_four_finger_spread(point.timestamp);
        }

        None
    }

    /// Handle finger up.
    fn on_touch_up(&mut self, point: TouchPoint) -> Option<Gesture> {
        let now = point.timestamp;

        // Find the initial position for this finger
        let initial = self.initial_touches.iter()
            .find(|t| t.id == point.id)
            .copied();

        // Remove from active
        self.active_touches.retain(|t| t.id != point.id);

        // If all fingers are up, evaluate discrete gestures
        if self.active_touches.is_empty() {
            let result = if let Some(init) = initial {
                self.evaluate_discrete(init, point, now)
            } else {
                None
            };

            self.initial_touches.clear();
            return result;
        }

        None
    }

    /// Evaluate discrete gestures (tap, swipe, long-press).
    fn evaluate_discrete(&mut self, start: TouchPoint, end: TouchPoint, now: u64) -> Option<Gesture> {
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let dist = (dx * dx + dy * dy).sqrt();
        let duration = now.saturating_sub(start.timestamp);

        // Check for swipe first
        if dist >= self.config.swipe_threshold {
            let time_s = duration as f32 / 1_000_000_000.0;
            let velocity = if time_s > 0.0 { dist / time_s } else { 0.0 };

            if velocity >= self.config.swipe_velocity {
                let direction = if dx.abs() > dy.abs() {
                    if dx > 0.0 { SwipeDirection::Right } else { SwipeDirection::Left }
                } else {
                    if dy > 0.0 { SwipeDirection::Down } else { SwipeDirection::Up }
                };

                // Check if it started from an edge
                let gesture_type = if start.x < self.config.edge_zone {
                    GestureType::EdgeSwipe(Edge::Left)
                } else if start.x > self.config.screen_width - self.config.edge_zone {
                    GestureType::EdgeSwipe(Edge::Right)
                } else if start.y < self.config.edge_zone {
                    GestureType::EdgeSwipe(Edge::Top)
                } else if start.y > self.config.screen_height - self.config.edge_zone {
                    GestureType::EdgeSwipe(Edge::Bottom)
                } else {
                    GestureType::Swipe(direction)
                };

                self.stats.gestures_recognized += 1;
                self.stats.swipes += 1;

                return Some(Gesture {
                    gesture_type,
                    state: GestureState::Completed,
                    finger_count: 1,
                    center_x: (start.x + end.x) / 2.0,
                    center_y: (start.y + end.y) / 2.0,
                    scale: 1.0,
                    rotation: 0.0,
                    delta_x: dx,
                    delta_y: dy,
                    velocity_x: dx / time_s.max(0.001),
                    velocity_y: dy / time_s.max(0.001),
                    started_at: start.timestamp,
                    duration,
                });
            }
        }

        // Tap detection
        if dist < self.config.swipe_threshold && duration < self.config.tap_max_duration_ns {
            // Check for double-tap
            let is_double = now.saturating_sub(self.last_tap_time) < self.config.double_tap_interval_ns;
            self.last_tap_time = now;

            let gesture_type = if is_double {
                GestureType::DoubleTap
            } else {
                GestureType::Tap
            };

            self.stats.gestures_recognized += 1;
            self.stats.taps += 1;

            return Some(Gesture {
                gesture_type,
                state: GestureState::Completed,
                finger_count: 1,
                center_x: end.x,
                center_y: end.y,
                scale: 1.0,
                rotation: 0.0,
                delta_x: 0.0,
                delta_y: 0.0,
                velocity_x: 0.0,
                velocity_y: 0.0,
                started_at: start.timestamp,
                duration,
            });
        }

        // Long press
        if dist < self.config.swipe_threshold && duration >= self.config.long_press_min_ns {
            self.stats.gestures_recognized += 1;
            return Some(Gesture {
                gesture_type: GestureType::LongPress,
                state: GestureState::Completed,
                finger_count: 1,
                center_x: end.x,
                center_y: end.y,
                scale: 1.0,
                rotation: 0.0,
                delta_x: 0.0,
                delta_y: 0.0,
                velocity_x: 0.0,
                velocity_y: 0.0,
                started_at: start.timestamp,
                duration,
            });
        }

        None
    }

    /// Detect two-finger gestures (pinch, rotate, pan).
    fn detect_two_finger(&mut self, now: u64) -> Option<Gesture> {
        if self.active_touches.len() < 2 || self.initial_touches.len() < 2 {
            return None;
        }

        let a0 = self.initial_touches[0];
        let b0 = self.initial_touches[1];
        let a1 = self.active_touches[0];
        let b1 = self.active_touches[1];

        // Initial distance and angle
        let dx0 = b0.x - a0.x;
        let dy0 = b0.y - a0.y;
        let dist0 = (dx0 * dx0 + dy0 * dy0).sqrt().max(1.0);

        // Current distance and angle
        let dx1 = b1.x - a1.x;
        let dy1 = b1.y - a1.y;
        let dist1 = (dx1 * dx1 + dy1 * dy1).sqrt().max(1.0);

        let scale = dist1 / dist0;
        let angle0 = dy0.atan2(dx0);
        let angle1 = dy1.atan2(dx1);
        let rotation = angle1 - angle0;

        let center_x = (a1.x + b1.x) / 2.0;
        let center_y = (a1.y + b1.y) / 2.0;
        let init_center_x = (a0.x + b0.x) / 2.0;
        let init_center_y = (a0.y + b0.y) / 2.0;

        let delta_x = center_x - init_center_x;
        let delta_y = center_y - init_center_y;

        let duration = now.saturating_sub(a0.timestamp);

        // Determine dominant gesture
        let scale_change = (scale - 1.0).abs();
        let rotation_change = rotation.abs();

        let gesture_type = if scale_change > self.config.pinch_threshold
            && scale_change > rotation_change
        {
            self.stats.pinches += 1;
            GestureType::Pinch
        } else if rotation_change > self.config.rotation_threshold {
            self.stats.rotations += 1;
            GestureType::Rotate
        } else {
            GestureType::Pan
        };

        self.stats.gestures_recognized += 1;

        Some(Gesture {
            gesture_type,
            state: GestureState::Tracking,
            finger_count: 2,
            center_x,
            center_y,
            scale,
            rotation,
            delta_x,
            delta_y,
            velocity_x: 0.0,
            velocity_y: 0.0,
            started_at: a0.timestamp,
            duration,
        })
    }

    /// Detect three-finger swipe.
    fn detect_multi_finger_swipe(&mut self, fingers: u8, now: u64) -> Option<Gesture> {
        if self.active_touches.len() < fingers as usize
            || self.initial_touches.len() < fingers as usize
        {
            return None;
        }

        // Compute average delta
        let mut dx_sum = 0.0f32;
        let mut dy_sum = 0.0f32;
        let n = fingers as usize;

        for i in 0..n {
            dx_sum += self.active_touches[i].x - self.initial_touches[i].x;
            dy_sum += self.active_touches[i].y - self.initial_touches[i].y;
        }
        let dx = dx_sum / n as f32;
        let dy = dy_sum / n as f32;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist < self.config.swipe_threshold { return None; }

        let direction = if dx.abs() > dy.abs() {
            if dx > 0.0 { SwipeDirection::Right } else { SwipeDirection::Left }
        } else {
            if dy > 0.0 { SwipeDirection::Down } else { SwipeDirection::Up }
        };

        let duration = now.saturating_sub(self.initial_touches[0].timestamp);
        self.stats.gestures_recognized += 1;
        self.stats.swipes += 1;

        Some(Gesture {
            gesture_type: GestureType::ThreeFingerSwipe(direction),
            state: GestureState::Completed,
            finger_count: fingers,
            center_x: self.active_touches.iter().map(|t| t.x).sum::<f32>() / n as f32,
            center_y: self.active_touches.iter().map(|t| t.y).sum::<f32>() / n as f32,
            scale: 1.0,
            rotation: 0.0,
            delta_x: dx,
            delta_y: dy,
            velocity_x: 0.0,
            velocity_y: 0.0,
            started_at: self.initial_touches[0].timestamp,
            duration,
        })
    }

    /// Detect four-finger spread (show desktop).
    fn detect_four_finger_spread(&mut self, now: u64) -> Option<Gesture> {
        if self.active_touches.len() < 4 || self.initial_touches.len() < 4 {
            return None;
        }

        // Measure spread: average distance from center
        let cx: f32 = self.active_touches.iter().map(|t| t.x).sum::<f32>() / 4.0;
        let cy: f32 = self.active_touches.iter().map(|t| t.y).sum::<f32>() / 4.0;
        let avg_dist: f32 = self.active_touches.iter()
            .map(|t| ((t.x - cx).powi(2) + (t.y - cy).powi(2)).sqrt())
            .sum::<f32>() / 4.0;

        let icx: f32 = self.initial_touches.iter().map(|t| t.x).sum::<f32>() / 4.0;
        let icy: f32 = self.initial_touches.iter().map(|t| t.y).sum::<f32>() / 4.0;
        let init_avg_dist: f32 = self.initial_touches.iter()
            .map(|t| ((t.x - icx).powi(2) + (t.y - icy).powi(2)).sqrt())
            .sum::<f32>() / 4.0;

        let spread = avg_dist / init_avg_dist.max(1.0);
        if spread < 1.5 { return None; } // Need 50% spread increase

        let duration = now.saturating_sub(self.initial_touches[0].timestamp);
        self.stats.gestures_recognized += 1;

        Some(Gesture {
            gesture_type: GestureType::FourFingerSpread,
            state: GestureState::Completed,
            finger_count: 4,
            center_x: cx,
            center_y: cy,
            scale: spread,
            rotation: 0.0,
            delta_x: 0.0,
            delta_y: 0.0,
            velocity_x: 0.0,
            velocity_y: 0.0,
            started_at: self.initial_touches[0].timestamp,
            duration,
        })
    }

    /// Cancel the current gesture.
    pub fn cancel(&mut self) {
        self.active_touches.clear();
        self.initial_touches.clear();
        self.current_gesture = None;
    }
}
