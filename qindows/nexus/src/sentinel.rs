//! # Sentinel — AI Security Overseer
//!
//! The Sentinel is the always-on security watchdog for Qindows.
//! It uses ML-based anomaly detection to identify and respond to
//! threats in real time, without human intervention.
//!
//! Capabilities (from spec Section 7):
//! - **Behavioral Profiling**: Learns the "normal" pattern of each Silo
//! - **Anomaly Detection**: Flags deviations (unusual syscalls, memory access)
//! - **Threat Response**: Auto-kill, isolate, rate-limit, or alert
//! - **Reputation Scoring**: Tracks trust scores for mesh peers
//! - **Audit Logging**: Tamper-proof log of all security events

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Threat severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Informational (logged, no action)
    Info,
    /// Low (rate-limited, monitored)
    Low,
    /// Medium (restricted capabilities)
    Medium,
    /// High (Silo isolated, human notified)
    High,
    /// Critical (Silo killed immediately)
    Critical,
}

/// Types of security events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventType {
    /// Unusual syscall pattern
    AnomalousSyscall,
    /// Memory access outside capability bounds
    MemoryViolation,
    /// Excessive resource consumption
    ResourceAbuse,
    /// Network exfiltration attempt
    DataExfiltration,
    /// Privilege escalation attempt
    PrivilegeEscalation,
    /// Tampered binary detected
    IntegrityViolation,
    /// Unknown peer on mesh
    UnknownPeer,
    /// Brute-force authentication attempt
    BruteForce,
    /// Abnormal IPC traffic pattern
    IpcAnomaly,
    /// Kernel exploit attempt
    KernelExploit,
}

/// A security event.
#[derive(Debug, Clone)]
pub struct SecurityEvent {
    /// Event ID
    pub id: u64,
    /// Event type
    pub event_type: EventType,
    /// Severity
    pub severity: Severity,
    /// Source Silo ID
    pub silo_id: u64,
    /// Timestamp
    pub timestamp: u64,
    /// Description
    pub description: String,
    /// Was the threat auto-mitigated?
    pub mitigated: bool,
    /// Response action taken
    pub action: ResponseAction,
}

/// Automatic response actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseAction {
    /// Log only
    LogOnly,
    /// Rate-limit the Silo
    RateLimit,
    /// Remove specific capabilities
    RestrictCaps,
    /// Isolate from network
    NetworkIsolate,
    /// Suspend the Silo
    Suspend,
    /// Kill the Silo immediately
    Kill,
    /// Alert the user
    Alert,
}

/// Behavioral profile for a Silo.
#[derive(Debug, Clone)]
pub struct SiloProfile {
    /// Silo ID
    pub silo_id: u64,
    /// Average syscalls per second (baseline)
    pub avg_syscalls_per_sec: f32,
    /// Average memory usage (bytes)
    pub avg_memory: u64,
    /// Average network bytes per second
    pub avg_net_bytes_per_sec: u64,
    /// Average IPC messages per second
    pub avg_ipc_per_sec: f32,
    /// Trust score (0-100)
    pub trust_score: u8,
    /// Number of violations recorded
    pub violations: u32,
    /// Last updated timestamp
    pub last_updated: u64,
}

impl SiloProfile {
    pub fn new(silo_id: u64) -> Self {
        SiloProfile {
            silo_id,
            avg_syscalls_per_sec: 0.0,
            avg_memory: 0,
            avg_net_bytes_per_sec: 0,
            avg_ipc_per_sec: 0.0,
            trust_score: 75, // Default trust
            violations: 0,
            last_updated: 0,
        }
    }
}

/// Anomaly check result.
#[derive(Debug, Clone)]
pub struct AnomalyResult {
    /// Is this an anomaly?
    pub is_anomaly: bool,
    /// Deviation factor (1.0 = normal, >2.0 = suspicious)
    pub deviation: f32,
    /// Suggested severity
    pub severity: Severity,
    /// What triggered the anomaly
    pub trigger: EventType,
}

/// Sentinel statistics.
#[derive(Debug, Clone, Default)]
pub struct SentinelStats {
    pub events_logged: u64,
    pub anomalies_detected: u64,
    pub silos_killed: u64,
    pub silos_isolated: u64,
    pub silos_rate_limited: u64,
    pub false_positives: u64,
    pub profiles_updated: u64,
}

/// The Sentinel AI Security Overseer.
pub struct Sentinel {
    /// Behavioral profiles per Silo
    pub profiles: BTreeMap<u64, SiloProfile>,
    /// Security event log (append-only)
    pub events: Vec<SecurityEvent>,
    /// Next event ID
    next_event_id: u64,
    /// Anomaly detection threshold (deviation multiplier)
    pub anomaly_threshold: f32,
    /// Auto-kill threshold (deviation multiplier)
    pub kill_threshold: f32,
    /// Statistics
    pub stats: SentinelStats,
}

impl Sentinel {
    pub fn new() -> Self {
        Sentinel {
            profiles: BTreeMap::new(),
            events: Vec::new(),
            next_event_id: 1,
            anomaly_threshold: 2.0,
            kill_threshold: 5.0,
            stats: SentinelStats::default(),
        }
    }

    /// Register a Silo for monitoring.
    pub fn register_silo(&mut self, silo_id: u64) {
        self.profiles.insert(silo_id, SiloProfile::new(silo_id));
    }

    /// Update a Silo's behavioral baseline.
    pub fn update_profile(
        &mut self,
        silo_id: u64,
        syscalls_per_sec: f32,
        memory: u64,
        net_bytes_per_sec: u64,
        ipc_per_sec: f32,
        now: u64,
    ) {
        let profile = self.profiles.entry(silo_id)
            .or_insert_with(|| SiloProfile::new(silo_id));

        // Exponential moving average (α = 0.1)
        let alpha = 0.1f32;
        profile.avg_syscalls_per_sec = profile.avg_syscalls_per_sec * (1.0 - alpha)
            + syscalls_per_sec * alpha;
        profile.avg_memory = ((profile.avg_memory as f32 * (1.0 - alpha))
            + (memory as f32 * alpha)) as u64;
        profile.avg_net_bytes_per_sec = ((profile.avg_net_bytes_per_sec as f32 * (1.0 - alpha))
            + (net_bytes_per_sec as f32 * alpha)) as u64;
        profile.avg_ipc_per_sec = profile.avg_ipc_per_sec * (1.0 - alpha)
            + ipc_per_sec * alpha;
        profile.last_updated = now;

        self.stats.profiles_updated += 1;
    }

    /// Check for anomalous behavior.
    pub fn check_anomaly(
        &self,
        silo_id: u64,
        syscalls_per_sec: f32,
        memory: u64,
        net_bytes_per_sec: u64,
    ) -> Option<AnomalyResult> {
        let profile = self.profiles.get(&silo_id)?;

        // Check syscall rate deviation
        if profile.avg_syscalls_per_sec > 0.0 {
            let deviation = syscalls_per_sec / profile.avg_syscalls_per_sec;
            if deviation > self.anomaly_threshold {
                return Some(AnomalyResult {
                    is_anomaly: true,
                    deviation,
                    severity: if deviation > self.kill_threshold {
                        Severity::Critical
                    } else if deviation > 3.0 {
                        Severity::High
                    } else {
                        Severity::Medium
                    },
                    trigger: EventType::AnomalousSyscall,
                });
            }
        }

        // Check memory spike
        if profile.avg_memory > 0 {
            let mem_ratio = memory as f32 / profile.avg_memory as f32;
            if mem_ratio > self.anomaly_threshold * 1.5 {
                return Some(AnomalyResult {
                    is_anomaly: true,
                    deviation: mem_ratio,
                    severity: Severity::Medium,
                    trigger: EventType::ResourceAbuse,
                });
            }
        }

        // Check network exfiltration
        if profile.avg_net_bytes_per_sec > 0 {
            let net_ratio = net_bytes_per_sec as f32 / profile.avg_net_bytes_per_sec as f32;
            if net_ratio > self.anomaly_threshold * 2.0 {
                return Some(AnomalyResult {
                    is_anomaly: true,
                    deviation: net_ratio,
                    severity: Severity::High,
                    trigger: EventType::DataExfiltration,
                });
            }
        }

        None
    }

    /// Log a security event and auto-respond.
    pub fn report_event(
        &mut self,
        event_type: EventType,
        severity: Severity,
        silo_id: u64,
        description: &str,
        now: u64,
    ) -> ResponseAction {
        let action = self.determine_response(severity, silo_id);

        let event = SecurityEvent {
            id: self.next_event_id,
            event_type,
            severity,
            silo_id,
            timestamp: now,
            description: String::from(description),
            mitigated: action != ResponseAction::LogOnly && action != ResponseAction::Alert,
            action,
        };

        self.next_event_id += 1;
        self.events.push(event);
        self.stats.events_logged += 1;
        self.stats.anomalies_detected += 1;

        // Update trust score
        if let Some(profile) = self.profiles.get_mut(&silo_id) {
            profile.violations += 1;
            let penalty = match severity {
                Severity::Info => 0,
                Severity::Low => 2,
                Severity::Medium => 5,
                Severity::High => 15,
                Severity::Critical => 50,
            };
            profile.trust_score = profile.trust_score.saturating_sub(penalty);
        }

        // Update stats
        match action {
            ResponseAction::Kill => self.stats.silos_killed += 1,
            ResponseAction::NetworkIsolate => self.stats.silos_isolated += 1,
            ResponseAction::RateLimit => self.stats.silos_rate_limited += 1,
            _ => {}
        }

        action
    }

    /// Determine response based on severity and history.
    fn determine_response(&self, severity: Severity, silo_id: u64) -> ResponseAction {
        let trust = self.profiles.get(&silo_id)
            .map(|p| p.trust_score)
            .unwrap_or(50);

        match severity {
            Severity::Info => ResponseAction::LogOnly,
            Severity::Low => {
                if trust < 30 { ResponseAction::RateLimit } else { ResponseAction::LogOnly }
            }
            Severity::Medium => {
                if trust < 20 { ResponseAction::Suspend } else { ResponseAction::RestrictCaps }
            }
            Severity::High => {
                if trust < 10 { ResponseAction::Kill } else { ResponseAction::NetworkIsolate }
            }
            Severity::Critical => ResponseAction::Kill,
        }
    }

    /// Get the trust score for a Silo.
    pub fn trust_score(&self, silo_id: u64) -> u8 {
        self.profiles.get(&silo_id).map(|p| p.trust_score).unwrap_or(0)
    }

    /// Get recent events (most recent first).
    pub fn recent_events(&self, limit: usize) -> Vec<&SecurityEvent> {
        self.events.iter().rev().take(limit).collect()
    }
}
