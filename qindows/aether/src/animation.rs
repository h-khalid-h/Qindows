//! # Aether Animation Engine
//!
//! GPU-driven animations for window transitions, focus effects,
//! and micro-interactions. All animations are physics-based
//! (spring model) for natural feel.

extern crate alloc;

use alloc::vec::Vec;

/// Animation easing functions.
#[derive(Debug, Clone, Copy)]
pub enum Easing {
    /// Linear interpolation
    Linear,
    /// Smooth acceleration (ease-in)
    EaseIn,
    /// Smooth deceleration (ease-out)
    EaseOut,
    /// Smooth acceleration + deceleration
    EaseInOut,
    /// Physics-based spring (bouncy)
    Spring { stiffness: f32, damping: f32 },
    /// Fast start, overshoot, settle
    ElasticOut,
}

/// An animated property value.
#[derive(Debug, Clone)]
pub struct Animation {
    /// What this animation targets
    pub target: AnimationTarget,
    /// Start value
    pub from: f32,
    /// End value
    pub to: f32,
    /// Current interpolated value
    pub current: f32,
    /// Progress (0.0 to 1.0)
    pub progress: f32,
    /// Duration in milliseconds
    pub duration_ms: f32,
    /// Elapsed time in milliseconds
    pub elapsed_ms: f32,
    /// Easing function
    pub easing: Easing,
    /// Whether the animation has completed
    pub completed: bool,
    /// Window this animation belongs to
    pub window_id: u64,
}

/// What property is being animated
#[derive(Debug, Clone, Copy)]
pub enum AnimationTarget {
    X,
    Y,
    Width,
    Height,
    Opacity,
    CornerRadius,
    Scale,
    Rotation,
    BlurRadius,
    ShadowOpacity,
}

impl Animation {
    /// Create a new animation.
    pub fn new(
        target: AnimationTarget,
        from: f32,
        to: f32,
        duration_ms: f32,
        easing: Easing,
        window_id: u64,
    ) -> Self {
        Animation {
            target,
            from,
            to,
            current: from,
            progress: 0.0,
            duration_ms,
            elapsed_ms: 0.0,
            easing,
            completed: false,
            window_id,
        }
    }

    /// Advance the animation by `dt` milliseconds.
    pub fn tick(&mut self, dt: f32) {
        if self.completed {
            return;
        }

        self.elapsed_ms += dt;
        self.progress = (self.elapsed_ms / self.duration_ms).min(1.0);

        // Apply easing
        let t = match self.easing {
            Easing::Linear => self.progress,
            Easing::EaseIn => self.progress * self.progress,
            Easing::EaseOut => 1.0 - (1.0 - self.progress) * (1.0 - self.progress),
            Easing::EaseInOut => {
                if self.progress < 0.5 {
                    2.0 * self.progress * self.progress
                } else {
                    1.0 - (-2.0 * self.progress + 2.0).powi(2) / 2.0
                }
            }
            Easing::Spring { stiffness, damping } => {
                let omega = stiffness.sqrt();
                let zeta = damping / (2.0 * omega);
                let t = self.progress;
                if zeta < 1.0 {
                    // Underdamped — oscillates
                    let wd = omega * (1.0 - zeta * zeta).sqrt();
                    1.0 - (-zeta * omega * t).exp()
                        * ((zeta * omega * t / wd).cos() + (zeta * omega / wd) * (wd * t).sin())
                } else {
                    // Critically damped
                    1.0 - (1.0 + omega * t) * (-omega * t).exp()
                }
            }
            Easing::ElasticOut => {
                let p = 0.3;
                let t = self.progress;
                (2.0f32).powf(-10.0 * t) * ((t - p / 4.0) * (core::f32::consts::TAU / p)).sin() + 1.0
            }
        };

        // Interpolate
        self.current = self.from + (self.to - self.from) * t;

        if self.progress >= 1.0 {
            self.current = self.to;
            self.completed = true;
        }
    }
}

/// The Aether Animation Engine — manages all active animations.
pub struct AnimationEngine {
    /// Active animations
    pub animations: Vec<Animation>,
}

impl AnimationEngine {
    pub fn new() -> Self {
        AnimationEngine {
            animations: Vec::new(),
        }
    }

    /// Start a new animation.
    pub fn animate(
        &mut self,
        target: AnimationTarget,
        from: f32,
        to: f32,
        duration_ms: f32,
        easing: Easing,
        window_id: u64,
    ) {
        // Cancel any existing animation on same target+window
        self.animations.retain(|a| {
            !(a.window_id == window_id && matches!((&a.target, &target),
                (AnimationTarget::X, AnimationTarget::X) |
                (AnimationTarget::Y, AnimationTarget::Y) |
                (AnimationTarget::Width, AnimationTarget::Width) |
                (AnimationTarget::Height, AnimationTarget::Height) |
                (AnimationTarget::Opacity, AnimationTarget::Opacity) |
                (AnimationTarget::Scale, AnimationTarget::Scale)
            ))
        });

        self.animations.push(Animation::new(
            target, from, to, duration_ms, easing, window_id,
        ));
    }

    /// Tick all animations by `dt` milliseconds.
    ///
    /// Returns a list of (window_id, target, value) tuples
    /// that need to be applied to the scene graph.
    pub fn tick(&mut self, dt: f32) -> Vec<(u64, AnimationTarget, f32)> {
        let mut updates = Vec::new();

        for anim in &mut self.animations {
            anim.tick(dt);
            updates.push((anim.window_id, anim.target, anim.current));
        }

        // Remove completed animations
        self.animations.retain(|a| !a.completed);

        updates
    }

    /// Check if any animations are running.
    pub fn is_active(&self) -> bool {
        !self.animations.is_empty()
    }

    /// Cancel all animations for a window.
    pub fn cancel_window(&mut self, window_id: u64) {
        self.animations.retain(|a| a.window_id != window_id);
    }
}

/// Predefined animation presets for consistent UX.
pub mod presets {
    use super::*;

    /// Window open animation (scale up + fade in).
    pub fn window_open(engine: &mut AnimationEngine, window_id: u64) {
        engine.animate(AnimationTarget::Scale, 0.8, 1.0, 250.0, Easing::EaseOut, window_id);
        engine.animate(AnimationTarget::Opacity, 0.0, 1.0, 200.0, Easing::EaseOut, window_id);
    }

    /// Window close animation (scale down + fade out).
    pub fn window_close(engine: &mut AnimationEngine, window_id: u64) {
        engine.animate(AnimationTarget::Scale, 1.0, 0.8, 200.0, Easing::EaseIn, window_id);
        engine.animate(AnimationTarget::Opacity, 1.0, 0.0, 150.0, Easing::EaseIn, window_id);
    }

    /// Window minimize animation (scale + move to taskbar).
    pub fn window_minimize(engine: &mut AnimationEngine, window_id: u64, taskbar_y: f32) {
        engine.animate(AnimationTarget::Scale, 1.0, 0.1, 300.0, Easing::EaseInOut, window_id);
        engine.animate(AnimationTarget::Y, 0.0, taskbar_y, 300.0, Easing::EaseInOut, window_id);
        engine.animate(AnimationTarget::Opacity, 1.0, 0.0, 250.0, Easing::EaseIn, window_id);
    }

    /// Window maximize animation (expand to fill screen).
    pub fn window_maximize(
        engine: &mut AnimationEngine,
        window_id: u64,
        from: (f32, f32, f32, f32),
        to: (f32, f32, f32, f32),
    ) {
        engine.animate(AnimationTarget::X, from.0, to.0, 250.0, Easing::EaseInOut, window_id);
        engine.animate(AnimationTarget::Y, from.1, to.1, 250.0, Easing::EaseInOut, window_id);
        engine.animate(AnimationTarget::Width, from.2, to.2, 250.0, Easing::EaseInOut, window_id);
        engine.animate(AnimationTarget::Height, from.3, to.3, 250.0, Easing::EaseInOut, window_id);
    }

    /// Focus glow pulse (subtle – Q-Glass materialize).
    pub fn focus_glow(engine: &mut AnimationEngine, window_id: u64) {
        engine.animate(
            AnimationTarget::ShadowOpacity,
            0.0, 0.6, 400.0,
            Easing::Spring { stiffness: 180.0, damping: 12.0 },
            window_id,
        );
    }
}
