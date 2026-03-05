//! # Aether Power Overlay
//!
//! System power/battery HUD overlay for the Aether compositor.
//! Displays real-time power stats, per-Silo energy usage,
//! thermal status, and battery life estimates.
//!
//! Rendered as an SDF vector overlay that composites on top of
//! all windows at the highest z-order.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

// ─── Display Metrics ────────────────────────────────────────────────────────

/// A power metric value for display.
#[derive(Debug, Clone)]
pub struct PowerMetric {
    /// Metric name
    pub name: String,
    /// Current value
    pub value: f32,
    /// Unit
    pub unit: String,
    /// Color indicator (0 = green, 1 = yellow, 2 = red)
    pub severity: u8,
    /// Historical values (last 60 samples for sparkline)
    pub history: Vec<f32>,
    /// History capacity
    pub history_cap: usize,
}

impl PowerMetric {
    pub fn new(name: &str, unit: &str) -> Self {
        PowerMetric {
            name: String::from(name),
            value: 0.0,
            unit: String::from(unit),
            severity: 0,
            history: Vec::new(),
            history_cap: 60,
        }
    }

    /// Update with a new sample.
    pub fn update(&mut self, value: f32, severity: u8) {
        self.value = value;
        self.severity = severity;
        self.history.push(value);
        while self.history.len() > self.history_cap {
            self.history.remove(0);
        }
    }

    /// Average over history.
    pub fn average(&self) -> f32 {
        if self.history.is_empty() { return 0.0; }
        let sum: f32 = self.history.iter().sum();
        sum / self.history.len() as f32
    }

    /// Min/max over history.
    pub fn range(&self) -> (f32, f32) {
        let min = self.history.iter().copied()
            .fold(f32::INFINITY, f32::min);
        let max = self.history.iter().copied()
            .fold(f32::NEG_INFINITY, f32::max);
        (min, max)
    }
}

// ─── Battery State ──────────────────────────────────────────────────────────

/// Battery charging state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChargeState {
    /// On AC power, battery charging
    Charging,
    /// On battery, discharging
    Discharging,
    /// Fully charged, on AC
    Full,
    /// No battery present (desktop)
    NoBattery,
}

/// Battery information.
#[derive(Debug, Clone)]
pub struct BatteryInfo {
    /// Charge percentage (0–100)
    pub percent: u8,
    /// Charging state
    pub state: ChargeState,
    /// Estimated remaining time (minutes)
    pub remaining_min: Option<u32>,
    /// Current drain rate (mW)
    pub drain_mw: u32,
    /// Battery health (0–100%)
    pub health: u8,
    /// Cycle count
    pub cycles: u32,
}

impl BatteryInfo {
    pub fn no_battery() -> Self {
        BatteryInfo {
            percent: 100,
            state: ChargeState::NoBattery,
            remaining_min: None,
            drain_mw: 0,
            health: 100,
            cycles: 0,
        }
    }
}

// ─── Per-Silo Energy ────────────────────────────────────────────────────────

/// Per-Silo power usage for overlay display.
#[derive(Debug, Clone)]
pub struct SiloPowerEntry {
    /// Silo ID
    pub silo_id: u64,
    /// App name
    pub name: String,
    /// Power draw estimate (mW)
    pub power_mw: u32,
    /// CPU usage percent
    pub cpu_percent: f32,
    /// GPU usage percent
    pub gpu_percent: f32,
    /// Is this Silo in energy violation?
    pub in_violation: bool,
}

// ─── Thermal Status ─────────────────────────────────────────────────────────

/// Thermal zone.
#[derive(Debug, Clone)]
pub struct ThermalZone {
    /// Zone name
    pub name: String,
    /// Temperature (°C)
    pub temp_c: f32,
    /// Critical threshold (°C)
    pub critical_c: f32,
    /// Is throttling active?
    pub throttling: bool,
}

// ─── Power Overlay ──────────────────────────────────────────────────────────

/// Overlay visibility mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    /// Hidden
    Hidden,
    /// Compact (battery icon + percentage only)
    Compact,
    /// Expanded (full power dashboard)
    Expanded,
    /// Critical alert (flashing low-battery warning)
    CriticalAlert,
}

/// Overlay statistics.
#[derive(Debug, Clone, Default)]
pub struct OverlayStats {
    pub frames_rendered: u64,
    pub alerts_shown: u64,
    pub critical_events: u64,
}

/// The Power Overlay.
pub struct PowerOverlay {
    /// Current mode
    pub mode: OverlayMode,
    /// Battery info
    pub battery: BatteryInfo,
    /// Power metrics
    pub metrics: Vec<PowerMetric>,
    /// Per-Silo entries (sorted by power draw)
    pub silo_entries: Vec<SiloPowerEntry>,
    /// Thermal zones
    pub thermal_zones: Vec<ThermalZone>,
    /// Total system power draw (watts)
    pub system_watts: f32,
    /// Low battery threshold
    pub low_battery_pct: u8,
    /// Critical battery threshold
    pub critical_battery_pct: u8,
    /// Statistics
    pub stats: OverlayStats,
}

impl PowerOverlay {
    pub fn new() -> Self {
        let mut metrics = Vec::new();
        metrics.push(PowerMetric::new("CPU Power", "W"));
        metrics.push(PowerMetric::new("GPU Power", "W"));
        metrics.push(PowerMetric::new("System Power", "W"));
        metrics.push(PowerMetric::new("CPU Temp", "°C"));
        metrics.push(PowerMetric::new("GPU Temp", "°C"));

        PowerOverlay {
            mode: OverlayMode::Compact,
            battery: BatteryInfo::no_battery(),
            metrics,
            silo_entries: Vec::new(),
            thermal_zones: Vec::new(),
            system_watts: 0.0,
            low_battery_pct: 20,
            critical_battery_pct: 5,
            stats: OverlayStats::default(),
        }
    }

    /// Update battery information.
    pub fn update_battery(&mut self, info: BatteryInfo) {
        // Auto-switch to critical alert if battery is critically low
        if info.state == ChargeState::Discharging && info.percent <= self.critical_battery_pct {
            self.mode = OverlayMode::CriticalAlert;
            self.stats.critical_events += 1;
        }

        self.battery = info;
    }

    /// Update per-Silo power entries.
    pub fn update_silos(&mut self, entries: Vec<SiloPowerEntry>) {
        self.silo_entries = entries;
        // Sort by power draw (descending)
        self.silo_entries.sort_by(|a, b| b.power_mw.cmp(&a.power_mw));
    }

    /// Update a metric by name.
    pub fn update_metric(&mut self, name: &str, value: f32) {
        let severity = if name.contains("Temp") {
            if value > 85.0 { 2 }
            else if value > 70.0 { 1 }
            else { 0 }
        } else {
            if value > 100.0 { 2 }
            else if value > 50.0 { 1 }
            else { 0 }
        };

        if let Some(metric) = self.metrics.iter_mut().find(|m| m.name == name) {
            metric.update(value, severity);
        }
    }

    /// Update thermal zones.
    pub fn update_thermal(&mut self, zones: Vec<ThermalZone>) {
        self.thermal_zones = zones;
    }

    /// Get the top N power-hungry Silos.
    pub fn top_consumers(&self, n: usize) -> &[SiloPowerEntry] {
        let end = n.min(self.silo_entries.len());
        &self.silo_entries[..end]
    }

    /// Should the overlay show a low-battery warning?
    pub fn should_warn(&self) -> bool {
        self.battery.state == ChargeState::Discharging
            && self.battery.percent <= self.low_battery_pct
    }

    /// Estimated time remaining as a display string.
    pub fn time_remaining_display(&self) -> String {
        match self.battery.remaining_min {
            Some(min) => {
                let hours = min / 60;
                let mins = min % 60;
                alloc::format!("{}h {}m", hours, mins)
            }
            None => String::from("--"),
        }
    }

    /// Toggle overlay mode.
    pub fn toggle(&mut self) {
        self.mode = match self.mode {
            OverlayMode::Hidden => OverlayMode::Compact,
            OverlayMode::Compact => OverlayMode::Expanded,
            OverlayMode::Expanded => OverlayMode::Hidden,
            OverlayMode::CriticalAlert => OverlayMode::Expanded,
        };
    }

    /// Dismiss critical alert (user acknowledged).
    pub fn dismiss_alert(&mut self) {
        if self.mode == OverlayMode::CriticalAlert {
            self.mode = OverlayMode::Compact;
        }
    }

    /// Render frame tick (called by Aether compositor).
    pub fn tick(&mut self) {
        if self.mode != OverlayMode::Hidden {
            self.stats.frames_rendered += 1;
        }
    }
}
