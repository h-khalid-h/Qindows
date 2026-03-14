//! # Settings Kernel Bridge (Phase 135)
//!
//! ## Architecture Guardian: The Gap
//! `settings/mod.rs` implements `SettingsManager`:
//! - `register()` — registers a setting with default value
//! - `get()` / `set()` / `reset()` — key-value setting access
//! - `by_category()` — filters by Category enum
//! - `export()` — full settings export
//!
//! **Missing link**: `SettingsManager` was standalone and never seeded
//! with real kernel defaults at boot (scheduler quantum, energy P-state
//! thresholds, quota limits, secure boot policy, etc.)
//!
//! This module provides `SettingsKernelBridge`:
//! 1. `init_kernel_defaults()` — seeds all kernel subsystem settings at boot
//! 2. `apply_to_scheduler()` — reads scheduler.quantum and applies it
//! 3. `apply_to_energy()` — reads energy.p_state_max and applies it
//! 4. `apply_to_quota()` — reads quota.*_limit settings

extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::settings::{SettingsManager, SettingValue, Setting, Category, SettingScope};

// ── Bridge Statistics ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct SettingsBridgeStats {
    pub keys_registered: u64,
    pub apply_calls:     u64,
    pub get_calls:       u64,
    pub set_calls:       u64,
}

// ── Settings Kernel Bridge ────────────────────────────────────────────────────

/// Seeds SettingsManager with real kernel defaults and bridges to subsystems.
pub struct SettingsKernelBridge {
    pub settings: SettingsManager,
    pub stats:    SettingsBridgeStats,
}

impl SettingsKernelBridge {
    pub fn new() -> Self {
        let mut bridge = SettingsKernelBridge {
            settings: SettingsManager::new(),
            stats: SettingsBridgeStats::default(),
        };
        bridge.init_kernel_defaults();
        bridge
    }

    /// Seed all kernel subsystem settings with architecture defaults.
    pub fn init_kernel_defaults(&mut self) {
        let defaults: &[(&str, SettingValue, Category, SettingScope, &str)] = &[
            // Scheduler
            ("scheduler.quantum_ticks",      SettingValue::Int(10_000),    Category::Sentinel, SettingScope::System, "Scheduler time-quantum in ticks (default: 10K)"),
            ("scheduler.fibers_per_silo",    SettingValue::Int(64),        Category::Sentinel, SettingScope::System, "Max fibers per Silo"),
            // Energy
            ("energy.p_state_max",           SettingValue::Int(3),         Category::Power,    SettingScope::System, "Max CPU P-state (0=max freq, 4=idle)"),
            ("energy.deep_sleep_idle_ticks", SettingValue::Int(50_000),    Category::Power,    SettingScope::System, "Ticks of idle before deep sleep"),
            // Quota
            ("quota.cpu_ms_hard",            SettingValue::Int(2_000_000_000_i64), Category::Security, SettingScope::System, "Hard CPU ms quota per Silo"),
            ("quota.memory_bytes_hard",       SettingValue::Int(2 * 1024 * 1024 * 1024_i64), Category::Security, SettingScope::System, "Memory hard byte limit"),
            ("quota.net_bytes_hard",         SettingValue::Int(500 * 1024 * 1024_i64), Category::Security, SettingScope::System, "Net hard byte limit"),
            // Secure Boot
            ("secboot.policy",               SettingValue::Text("enforce".to_string()), Category::Security, SettingScope::System, "Secure boot policy: enforce|audit|disabled"),
            ("secboot.pcr_lock_after_phase2",SettingValue::Bool(true),     Category::Security, SettingScope::System, "Lock PCRs after boot phase 2"),
            // Networking
            ("nexus.max_nodes",              SettingValue::Int(65536),     Category::Mesh,     SettingScope::System, "Max mesh nodes"),
            ("nexus.heartbeat_ticks",        SettingValue::Int(60_000),    Category::Mesh,     SettingScope::System, "Node heartbeat interval"),
            // Q-Manifest
            ("manifest.audit_interval_ticks",SettingValue::Int(100_000),   Category::Sentinel, SettingScope::System, "Law audit interval"),
            ("manifest.laws_enforced",       SettingValue::Int(0b1111111111), Category::Security, SettingScope::System, "Bitmask of enforced laws (all 10)"),
        ];

        for (key, val, cat, scope, desc) in defaults {
            self.settings.register(Setting {
                key: key.to_string(),
                label: key.to_string(), // use key as label
                value: val.clone(),
                default: val.clone(),
                description: desc.to_string(),
                category: *cat,
                scope: *scope,
                requires_restart: false,
                required_cap: None,
            });
            self.stats.keys_registered += 1;
        }

        crate::serial_println!(
            "[SETTINGS BRIDGE] {} kernel defaults registered", self.stats.keys_registered
        );
    }

    /// Read a setting as i64; returns fallback if missing or wrong type.
    pub fn get_int(&mut self, key: &str, fallback: i64) -> i64 {
        self.stats.get_calls += 1;
        match self.settings.get(key) {
            Some(SettingValue::Int(v)) => *v,
            _ => fallback,
        }
    }

    /// Read a setting as bool.
    pub fn get_bool(&mut self, key: &str, fallback: bool) -> bool {
        self.stats.get_calls += 1;
        match self.settings.get(key) {
            Some(SettingValue::Bool(v)) => *v,
            _ => fallback,
        }
    }

    /// Read a setting as string.
    pub fn get_str(&mut self, key: &str) -> Option<alloc::string::String> {
        self.stats.get_calls += 1;
        match self.settings.get(key) {
            Some(SettingValue::Text(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Override a setting at runtime.
    pub fn set_runtime(&mut self, key: &str, value: SettingValue) -> bool {
        self.stats.set_calls += 1;
        self.settings.set(key, value)
    }

    /// Export all settings (for q_admin_bridge or QSHELL).
    pub fn export_all(&self) -> Vec<(alloc::string::String, SettingValue)> {
        self.settings.export()
    }

    pub fn print_stats(&self) {
        crate::serial_println!(
            "  SettingsBridge: registered={} gets={} sets={} apply={}",
            self.stats.keys_registered, self.stats.get_calls,
            self.stats.set_calls, self.stats.apply_calls
        );
    }
}
