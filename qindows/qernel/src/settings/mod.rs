//! # Qindows Settings Manager
//!
//! Replaces the Windows Registry with a structured, type-safe
//! settings system. All settings are Prism objects — versioned,
//! per-Silo, and semantic-searchable.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A setting value — type-safe unlike the registry's raw bytes.
#[derive(Debug, Clone)]
pub enum SettingValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Color(u32),        // ARGB
    Path(String),      // Prism OID path
    List(Vec<SettingValue>),
    Map(BTreeMap<String, SettingValue>),
}

/// Setting scope — where a setting applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingScope {
    /// System-wide (requires admin capability)
    System,
    /// Per-user (per login session)
    User,
    /// Per-Silo (app-specific)
    Silo(u64),
}

/// A setting category (replaces Registry hives).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// Display settings (resolution, scaling, theme)
    Display,
    /// Input settings (keyboard layout, mouse speed)
    Input,
    /// Network settings (WiFi, mesh, proxy)
    Network,
    /// Sound settings (volume, output device)
    Sound,
    /// Power settings (sleep, battery plan)
    Power,
    /// Security settings (firewall, auth methods)
    Security,
    /// Privacy settings (telemetry, location)
    Privacy,
    /// Accessibility settings (screen reader, contrast)
    Accessibility,
    /// Aether settings (animations, effects, taskbar)
    Appearance,
    /// Prism settings (storage, sync, dedup)
    Storage,
    /// Sentinel settings (law config, scan interval)
    Sentinel,
    /// Nexus settings (mesh participation, bandwidth limit)
    Mesh,
    /// App-specific settings
    App(u64),
}

/// A single setting definition.
#[derive(Debug, Clone)]
pub struct Setting {
    /// Unique key (e.g., "display.scaling_factor")
    pub key: String,
    /// Human-readable label
    pub label: String,
    /// Description
    pub description: String,
    /// Category
    pub category: Category,
    /// Current value
    pub value: SettingValue,
    /// Default value
    pub default: SettingValue,
    /// Scope
    pub scope: SettingScope,
    /// Whether changing this requires a restart
    pub requires_restart: bool,
    /// Required capability to modify
    pub required_cap: Option<String>,
}

/// The Settings Manager.
pub struct SettingsManager {
    /// All settings indexed by key
    settings: BTreeMap<String, Setting>,
    /// Change observers (key → callback IDs)
    observers: BTreeMap<String, Vec<u64>>,
    /// Next observer ID
    next_observer: u64,
}

impl SettingsManager {
    pub fn new() -> Self {
        let mut mgr = SettingsManager {
            settings: BTreeMap::new(),
            observers: BTreeMap::new(),
            next_observer: 1,
        };

        // Register default system settings
        mgr.register_defaults();
        mgr
    }

    /// Register the default Qindows settings.
    fn register_defaults(&mut self) {
        // Display
        self.register(Setting {
            key: String::from("display.scaling"),
            label: String::from("Display Scaling"),
            description: String::from("UI scaling factor (1.0 = 100%)"),
            category: Category::Display,
            value: SettingValue::Float(1.0),
            default: SettingValue::Float(1.0),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });

        self.register(Setting {
            key: String::from("display.dark_mode"),
            label: String::from("Dark Mode"),
            description: String::from("Enable the Qindows dark theme"),
            category: Category::Display,
            value: SettingValue::Bool(true),
            default: SettingValue::Bool(true),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });

        // Appearance
        self.register(Setting {
            key: String::from("appearance.animations"),
            label: String::from("Window Animations"),
            description: String::from("Enable window open/close animations"),
            category: Category::Appearance,
            value: SettingValue::Bool(true),
            default: SettingValue::Bool(true),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });

        self.register(Setting {
            key: String::from("appearance.transparency"),
            label: String::from("Q-Glass Transparency"),
            description: String::from("Enable glass blur effects"),
            category: Category::Appearance,
            value: SettingValue::Bool(true),
            default: SettingValue::Bool(true),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });

        self.register(Setting {
            key: String::from("appearance.accent_color"),
            label: String::from("Accent Color"),
            description: String::from("Primary accent color"),
            category: Category::Appearance,
            value: SettingValue::Color(0xFF_06_D6_A0),
            default: SettingValue::Color(0xFF_06_D6_A0),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });

        // Power
        self.register(Setting {
            key: String::from("power.policy"),
            label: String::from("Power Policy"),
            description: String::from("CPU power management policy"),
            category: Category::Power,
            value: SettingValue::Text(String::from("adaptive")),
            default: SettingValue::Text(String::from("adaptive")),
            scope: SettingScope::System,
            requires_restart: false,
            required_cap: Some(String::from("system.power")),
        });

        // Sentinel
        self.register(Setting {
            key: String::from("sentinel.scan_interval"),
            label: String::from("Scan Interval"),
            description: String::from("Seconds between Sentinel health scans"),
            category: Category::Sentinel,
            value: SettingValue::Int(5),
            default: SettingValue::Int(5),
            scope: SettingScope::System,
            requires_restart: false,
            required_cap: Some(String::from("system.sentinel")),
        });

        // Mesh
        self.register(Setting {
            key: String::from("mesh.participate"),
            label: String::from("Mesh Participation"),
            description: String::from("Contribute resources to the Global Mesh"),
            category: Category::Mesh,
            value: SettingValue::Bool(true),
            default: SettingValue::Bool(true),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });

        self.register(Setting {
            key: String::from("mesh.bandwidth_limit"),
            label: String::from("Bandwidth Limit"),
            description: String::from("Max mesh bandwidth in Mbps (0 = unlimited)"),
            category: Category::Mesh,
            value: SettingValue::Int(0),
            default: SettingValue::Int(0),
            scope: SettingScope::User,
            requires_restart: false,
            required_cap: None,
        });
    }

    /// Register a new setting.
    pub fn register(&mut self, setting: Setting) {
        self.settings.insert(setting.key.clone(), setting);
    }

    /// Get a setting value.
    pub fn get(&self, key: &str) -> Option<&SettingValue> {
        self.settings.get(key).map(|s| &s.value)
    }

    /// Set a setting value.
    pub fn set(&mut self, key: &str, value: SettingValue) -> bool {
        if let Some(setting) = self.settings.get_mut(key) {
            setting.value = value;
            true
        } else {
            false
        }
    }

    /// Reset a setting to its default.
    pub fn reset(&mut self, key: &str) -> bool {
        if let Some(setting) = self.settings.get_mut(key) {
            setting.value = setting.default.clone();
            true
        } else {
            false
        }
    }

    /// Get all settings in a category.
    pub fn by_category(&self, cat: Category) -> Vec<&Setting> {
        self.settings.values().filter(|s| s.category == cat).collect()
    }

    /// Search settings by label or description.
    pub fn search(&self, query: &str) -> Vec<&Setting> {
        let q = query.to_lowercase();
        self.settings.values().filter(|s| {
            s.label.to_lowercase().contains(&q) || s.description.to_lowercase().contains(&q)
        }).collect()
    }

    /// Export all settings as key-value pairs.
    pub fn export(&self) -> Vec<(String, SettingValue)> {
        self.settings.iter().map(|(k, s)| (k.clone(), s.value.clone())).collect()
    }

    /// Get total number of settings.
    pub fn count(&self) -> usize {
        self.settings.len()
    }
}
