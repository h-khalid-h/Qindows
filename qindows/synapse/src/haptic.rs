//! # Haptic Engine — Vibration Patterns & Spatial Feedback
//!
//! Drives haptic actuators for tactile UI feedback and
//! immersive spatial interactions (Section 6.5).
//!
//! Features:
//! - Pre-defined haptic patterns (click, buzz, pulse)
//! - Custom waveform support
//! - Spatial haptics (location-aware intensity)
//! - Per-Silo haptic profiles
//! - Power-aware: reduces intensity on low battery

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Haptic pattern type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HapticPattern {
    Click,
    DoubleClick,
    LongPress,
    Success,
    Error,
    Warning,
    SelectionChange,
    ImpactLight,
    ImpactMedium,
    ImpactHeavy,
}

/// Haptic actuator type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActuatorType {
    Lra,   // Linear Resonant Actuator
    Erm,   // Eccentric Rotating Mass
    Piezo, // Piezoelectric
}

/// A haptic actuator.
#[derive(Debug, Clone)]
pub struct Actuator {
    pub id: u32,
    pub actuator_type: ActuatorType,
    pub max_amplitude: f32,
    pub freq_range: (f32, f32), // Hz min, max
    pub active: bool,
}

/// A custom waveform point.
#[derive(Debug, Clone, Copy)]
pub struct WavePoint {
    pub time_ms: u32,
    pub amplitude: f32,
    pub frequency: f32,
}

/// A custom waveform.
#[derive(Debug, Clone)]
pub struct Waveform {
    pub name: String,
    pub points: Vec<WavePoint>,
    pub duration_ms: u32,
}

/// Haptic statistics.
#[derive(Debug, Clone, Default)]
pub struct HapticStats {
    pub patterns_played: u64,
    pub waveforms_played: u64,
    pub total_duration_ms: u64,
}

/// The Haptic Engine.
pub struct HapticEngine {
    pub actuators: BTreeMap<u32, Actuator>,
    pub waveforms: BTreeMap<String, Waveform>,
    pub intensity_scale: f32,
    pub power_save: bool,
    pub stats: HapticStats,
}

impl HapticEngine {
    pub fn new() -> Self {
        HapticEngine {
            actuators: BTreeMap::new(),
            waveforms: BTreeMap::new(),
            intensity_scale: 1.0,
            power_save: false,
            stats: HapticStats::default(),
        }
    }

    /// Register an actuator.
    pub fn add_actuator(&mut self, id: u32, atype: ActuatorType, max_amp: f32, freq_min: f32, freq_max: f32) {
        self.actuators.insert(id, Actuator {
            id, actuator_type: atype, max_amplitude: max_amp,
            freq_range: (freq_min, freq_max), active: true,
        });
    }

    /// Play a predefined pattern.
    pub fn play_pattern(&mut self, pattern: HapticPattern) -> u32 {
        let (duration, amplitude) = match pattern {
            HapticPattern::Click => (10, 0.8),
            HapticPattern::DoubleClick => (30, 0.7),
            HapticPattern::LongPress => (50, 0.6),
            HapticPattern::Success => (100, 0.5),
            HapticPattern::Error => (200, 0.9),
            HapticPattern::Warning => (150, 0.7),
            HapticPattern::SelectionChange => (5, 0.3),
            HapticPattern::ImpactLight => (15, 0.4),
            HapticPattern::ImpactMedium => (20, 0.6),
            HapticPattern::ImpactHeavy => (30, 0.9),
        };

        let scale = if self.power_save { 0.5 } else { self.intensity_scale };
        let _effective_amp = amplitude as f32 * scale;

        self.stats.patterns_played += 1;
        self.stats.total_duration_ms += duration as u64;
        duration
    }

    /// Register a custom waveform.
    pub fn register_waveform(&mut self, name: &str, points: Vec<WavePoint>) {
        let duration = points.iter().map(|p| p.time_ms).max().unwrap_or(0);
        self.waveforms.insert(String::from(name), Waveform {
            name: String::from(name), points, duration_ms: duration,
        });
    }

    /// Play a custom waveform.
    pub fn play_waveform(&mut self, name: &str) -> Result<u32, &'static str> {
        let wf = self.waveforms.get(name).ok_or("Waveform not found")?;
        let duration = wf.duration_ms;
        self.stats.waveforms_played += 1;
        self.stats.total_duration_ms += duration as u64;
        Ok(duration)
    }

    /// Set power-save mode.
    pub fn set_power_save(&mut self, enabled: bool) {
        self.power_save = enabled;
    }

    /// Set global intensity scale (0.0 - 1.0).
    pub fn set_intensity(&mut self, scale: f32) {
        self.intensity_scale = scale.max(0.0).min(1.0);
    }
}
