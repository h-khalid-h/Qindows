//! # Qindows App Manifest
//!
//! Every Qindows app ships with a manifest that declares its
//! identity, capabilities, dependencies, and resource requirements.
//! The Sentinel validates the manifest before creating a Silo.
//!
//! Format: human-readable TOML parsed into this structure.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// A Qindows App Manifest.
#[derive(Debug, Clone)]
pub struct AppManifest {
    /// Package identity
    pub package: PackageInfo,
    /// Required capabilities
    pub capabilities: Vec<Capability>,
    /// Dependencies on other apps/libs
    pub dependencies: Vec<Dependency>,
    /// Resource requirements
    pub resources: ResourceReqs,
    /// Aether window preferences
    pub window: WindowPrefs,
    /// Chimera compatibility (if legacy Win32 app)
    pub chimera: Option<ChimeraCompat>,
}

/// Package identity.
#[derive(Debug, Clone)]
pub struct PackageInfo {
    /// Unique app ID (reverse-domain: "com.qindows.browser")
    pub id: String,
    /// Display name
    pub name: String,
    /// Version (SemVer)
    pub version: String,
    /// Publisher
    pub publisher: String,
    /// Description
    pub description: String,
    /// Icon OID (Prism object)
    pub icon_oid: Option<u64>,
    /// Category
    pub category: AppCategory,
    /// Entry point binary
    pub entry_point: String,
}

/// App categories.
#[derive(Debug, Clone, Copy)]
pub enum AppCategory {
    Productivity,
    Communication,
    Media,
    Development,
    Gaming,
    System,
    Security,
    Education,
    Finance,
    Utility,
}

/// Capability tokens an app can request.
#[derive(Debug, Clone)]
pub enum Capability {
    /// Read/write to filesystem
    FileSystem { paths: Vec<String> },
    /// Network access
    Network { domains: Vec<String> },
    /// Camera access
    Camera,
    /// Microphone access
    Microphone,
    /// Location access
    Location,
    /// Bluetooth access
    Bluetooth,
    /// USB device access
    Usb,
    /// Background execution
    Background,
    /// Notifications
    Notifications,
    /// Clipboard read
    ClipboardRead,
    /// Clipboard write
    ClipboardWrite,
    /// System settings (read-only)
    SettingsRead,
    /// System settings (write)
    SettingsWrite,
    /// Inter-Silo communication with specific app
    Ipc { target_app: String },
    /// Mesh network participation
    Mesh,
    /// GPU compute
    GpuCompute,
    /// Raw hardware access (privileged)
    HardwareAccess,
}

/// A dependency on another app or library.
#[derive(Debug, Clone)]
pub struct Dependency {
    /// Package ID
    pub id: String,
    /// Minimum version
    pub min_version: String,
    /// Is this optional?
    pub optional: bool,
}

/// Resource requirements.
#[derive(Debug, Clone)]
pub struct ResourceReqs {
    /// Minimum memory (bytes)
    pub min_memory: u64,
    /// Maximum memory (bytes, for Silo limit)
    pub max_memory: u64,
    /// CPU cores desired
    pub cpu_cores: u8,
    /// GPU required?
    pub gpu_required: bool,
    /// Storage quota (bytes)
    pub storage_quota: u64,
    /// Network bandwidth limit (bytes/sec, 0=unlimited)
    pub bandwidth_limit: u64,
}

impl Default for ResourceReqs {
    fn default() -> Self {
        ResourceReqs {
            min_memory: 16 * 1024 * 1024,       // 16 MiB
            max_memory: 256 * 1024 * 1024,       // 256 MiB
            cpu_cores: 1,
            gpu_required: false,
            storage_quota: 100 * 1024 * 1024,    // 100 MiB
            bandwidth_limit: 0,
        }
    }
}

/// Aether window preferences.
#[derive(Debug, Clone)]
pub struct WindowPrefs {
    /// Default window width
    pub width: u32,
    /// Default window height
    pub height: u32,
    /// Minimum size
    pub min_width: u32,
    pub min_height: u32,
    /// Is resizable?
    pub resizable: bool,
    /// Window decorations (title bar, borders)
    pub decorated: bool,
    /// Transparency support
    pub transparent: bool,
    /// Startup mode
    pub startup: StartupMode,
}

/// Window startup mode.
#[derive(Debug, Clone, Copy)]
pub enum StartupMode {
    Normal,
    Maximized,
    Fullscreen,
    Hidden,
    Tray,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        WindowPrefs {
            width: 900,
            height: 600,
            min_width: 320,
            min_height: 240,
            resizable: true,
            decorated: true,
            transparent: false,
            startup: StartupMode::Normal,
        }
    }
}

/// Chimera compatibility settings.
#[derive(Debug, Clone)]
pub struct ChimeraCompat {
    /// Windows version to emulate
    pub windows_version: String,
    /// Required DLLs
    pub required_dlls: Vec<String>,
    /// DirectX version needed
    pub directx_version: Option<u8>,
    /// .NET CLR version needed
    pub dotnet_version: Option<String>,
    /// Registry keys to pre-populate
    pub registry_keys: Vec<(String, String)>,
}

/// Validate a manifest before creating a Silo.
pub fn validate(manifest: &AppManifest) -> Vec<String> {
    let mut errors = Vec::new();

    if manifest.package.id.is_empty() {
        errors.push(String::from("Package ID is required"));
    }
    if manifest.package.name.is_empty() {
        errors.push(String::from("Package name is required"));
    }
    if manifest.package.version.is_empty() {
        errors.push(String::from("Package version is required"));
    }
    if manifest.package.entry_point.is_empty() {
        errors.push(String::from("Entry point is required"));
    }

    // Check for dangerous capability combinations
    let has_network = manifest.capabilities.iter().any(|c| matches!(c, Capability::Network { .. }));
    let has_hardware = manifest.capabilities.iter().any(|c| matches!(c, Capability::HardwareAccess));

    if has_hardware && has_network {
        errors.push(String::from("WARNING: App requests both HardwareAccess and Network — high risk"));
    }

    // Validate resource limits
    if manifest.resources.max_memory < manifest.resources.min_memory {
        errors.push(String::from("max_memory must be >= min_memory"));
    }

    errors
}
