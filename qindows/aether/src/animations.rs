//! # Aether Animation System
//!
//! Declarative, interruptible animations for the Qindows UI.
//! Supports spring physics, easing curves, keyframes,
//! and orchestrated sequences.

extern crate alloc;

use alloc::string::String;
use crate::math_ext::F32Ext;
use alloc::vec::Vec;

/// Easing function type.
#[derive(Debug, Clone, Copy)]
pub enum Easing {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// Cubic bezier (control points)
    CubicBezier(f32, f32, f32, f32),
    /// Spring physics
    Spring { stiffness: f32, damping: f32, mass: f32 },
    /// Bounce
    BounceOut,
    /// Elastic
    ElasticOut,
}

impl Easing {
    /// Evaluate the easing function at t (0.0 - 1.0).
    pub fn apply(&self, t: f32) -> f32 {
        match self {
            Easing::Linear => t,
            Easing::EaseIn => t * t * t,
            Easing::EaseOut => {
                let inv = 1.0 - t;
                1.0 - inv * inv * inv
            }
            Easing::EaseInOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let inv = -2.0 * t + 2.0;
                    1.0 - (inv * inv * inv) / 2.0
                }
            }
            Easing::CubicBezier(x1, y1, x2, y2) => {
                // Simplified cubic bezier (approximate)
                let ct = t;
                let ya = 3.0 * y1 - 3.0 * y2 + 1.0;
                let yb = 3.0 * y2 - 6.0 * y1;
                let yc = 3.0 * y1;
                let _ = (x1, x2); // x-axis control points used in full Newton-Raphson solve
                ya * ct * ct * ct + yb * ct * ct + yc * ct
            }
            Easing::Spring { stiffness, damping, mass } => {
                let omega = (stiffness / mass).sqrt();
                let zeta = damping / (2.0 * (stiffness * mass).sqrt());
                if zeta < 1.0 {
                    // Underdamped spring
                    let wd = omega * (1.0 - zeta * zeta).sqrt();
                    let decay = (-zeta * omega * t).exp();
                    1.0 - decay * ((wd * t).cos() + (zeta * omega / wd) * (wd * t).sin())
                } else {
                    // Critically/overdamped
                    1.0 - (-omega * t).exp() * (1.0 + omega * t)
                }
            }
            Easing::BounceOut => {
                let n1 = 7.5625;
                let d1 = 2.75;
                if t < 1.0 / d1 {
                    n1 * t * t
                } else if t < 2.0 / d1 {
                    let t2 = t - 1.5 / d1;
                    n1 * t2 * t2 + 0.75
                } else if t < 2.5 / d1 {
                    let t2 = t - 2.25 / d1;
                    n1 * t2 * t2 + 0.9375
                } else {
                    let t2 = t - 2.625 / d1;
                    n1 * t2 * t2 + 0.984375
                }
            }
            Easing::ElasticOut => {
                if t <= 0.0 { return 0.0; }
                if t >= 1.0 { return 1.0; }
                let c4 = core::f32::consts::TAU / 3.0;
                2.0_f32.powf(-10.0 * t) * ((t * 10.0 - 0.75) * c4).sin() + 1.0
            }
        }
    }
}

/// An animatable property.
#[derive(Debug, Clone, Copy)]
pub enum AnimProperty {
    X, Y,
    Width, Height,
    Opacity,
    Scale,
    ScaleX, ScaleY,
    Rotation,
    BorderRadius,
    BlurRadius,
    Color(u8), // 0=R, 1=G, 2=B, 3=A
}

/// A single animation.
#[derive(Debug, Clone)]
pub struct Animation {
    /// Unique ID
    pub id: u64,
    /// Widget being animated
    pub target_id: u64,
    /// Property being animated
    pub property: AnimProperty,
    /// Start value
    pub from: f32,
    /// End value
    pub to: f32,
    /// Duration in ms
    pub duration_ms: u32,
    /// Delay before starting (ms)
    pub delay_ms: u32,
    /// Easing function
    pub easing: Easing,
    /// Current progress (0.0 - 1.0)
    pub progress: f32,
    /// Is this animation playing?
    pub playing: bool,
    /// Loop count (0 = infinite, 1 = once)
    pub loop_count: u32,
    /// Current loop iteration
    pub current_loop: u32,
    /// Reverse on alternate loops?
    pub alternate: bool,
    /// Callback name on completion
    pub on_complete: Option<String>,
}

impl Animation {
    pub fn new(target_id: u64, property: AnimProperty, from: f32, to: f32) -> Self {
        Animation {
            id: 0,
            target_id,
            property,
            from, to,
            duration_ms: 300,
            delay_ms: 0,
            easing: Easing::EaseInOut,
            progress: 0.0,
            playing: false,
            loop_count: 1,
            current_loop: 0,
            alternate: false,
            on_complete: None,
        }
    }

    /// Get the current interpolated value.
    pub fn current_value(&self) -> f32 {
        let eased = self.easing.apply(self.progress);
        let should_reverse = self.alternate && self.current_loop % 2 == 1;
        if should_reverse {
            self.to + (self.from - self.to) * eased
        } else {
            self.from + (self.to - self.from) * eased
        }
    }

    /// Is this animation complete?
    pub fn is_done(&self) -> bool {
        if self.loop_count == 0 { return false; } // Infinite
        self.current_loop >= self.loop_count && self.progress >= 1.0
    }
}

/// A keyframe in a multi-step animation.
#[derive(Debug, Clone)]
pub struct Keyframe {
    /// Time position (0.0 - 1.0)
    pub time: f32,
    /// Value at this keyframe
    pub value: f32,
    /// Easing to this keyframe
    pub easing: Easing,
}

/// An animation sequence (orchestrates multiple animations).
#[derive(Debug, Clone)]
pub struct AnimSequence {
    pub id: u64,
    pub name: String,
    pub animations: Vec<u64>, // Animation IDs
    pub parallel: bool,       // Run simultaneously or sequentially
    pub current_index: usize,
}

/// The Animation Engine.
pub struct AnimationEngine {
    /// All animations
    pub animations: Vec<Animation>,
    /// Sequences
    pub sequences: Vec<AnimSequence>,
    /// Next IDs
    next_anim_id: u64,
    next_seq_id: u64,
    /// Completed animation events (for callbacks)
    pub completed: Vec<u64>,
    /// Stats
    pub active_count: usize,
    pub total_completed: u64,
}

impl AnimationEngine {
    pub fn new() -> Self {
        AnimationEngine {
            animations: Vec::new(),
            sequences: Vec::new(),
            next_anim_id: 1,
            next_seq_id: 1,
            completed: Vec::new(),
            active_count: 0,
            total_completed: 0,
        }
    }

    /// Create and start an animation.
    pub fn animate(
        &mut self,
        target_id: u64,
        property: AnimProperty,
        from: f32,
        to: f32,
        duration_ms: u32,
        easing: Easing,
    ) -> u64 {
        let id = self.next_anim_id;
        self.next_anim_id += 1;

        let mut anim = Animation::new(target_id, property, from, to);
        anim.id = id;
        anim.duration_ms = duration_ms;
        anim.easing = easing;
        anim.playing = true;

        self.animations.push(anim);
        self.active_count += 1;
        id
    }

    /// Tick all animations (call every frame).
    pub fn update(&mut self, delta_ms: f32) {
        self.completed.clear();

        for anim in &mut self.animations {
            if !anim.playing { continue; }

            if anim.delay_ms > 0 {
                if delta_ms as u32 >= anim.delay_ms {
                    anim.delay_ms = 0;
                } else {
                    anim.delay_ms -= delta_ms as u32;
                    continue;
                }
            }

            let step = delta_ms / anim.duration_ms as f32;
            anim.progress = (anim.progress + step).min(1.0);

            if anim.progress >= 1.0 {
                anim.current_loop += 1;

                if anim.is_done() {
                    anim.playing = false;
                    self.total_completed += 1;
                } else {
                    anim.progress = 0.0;
                }
            }
        }

        // Remove completed non-looping animations
        let was = self.animations.len();
        self.animations.retain(|a| a.playing || a.loop_count == 0);
        self.active_count = self.animations.iter().filter(|a| a.playing).count();
        let _ = was; // suppress warning
    }

    /// Cancel an animation.
    pub fn cancel(&mut self, anim_id: u64) {
        self.animations.retain(|a| a.id != anim_id);
    }

    /// Cancel all animations for a widget.
    pub fn cancel_for(&mut self, target_id: u64) {
        self.animations.retain(|a| a.target_id != target_id);
    }

    /// Get the current animated value for a widget property.
    pub fn get_value(&self, target_id: u64, property: AnimProperty) -> Option<f32> {
        self.animations.iter()
            .find(|a| a.target_id == target_id && matches!(a.property, p if core::mem::discriminant(&p) == core::mem::discriminant(&property)))
            .map(|a| a.current_value())
    }
}
