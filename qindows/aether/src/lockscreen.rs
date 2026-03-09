//! # Aether Lockscreen
//!
//! The Qindows lock screen — the first thing users see.
//! Renders a clock, status indicators, and authentication UI
//! on top of the Aether compositor.

extern crate alloc;

use alloc::string::String;

/// Lock screen state machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockState {
    /// Screen is locked, showing clock and notifications
    Locked,
    /// User has interacted — show authentication input
    Authenticating,
    /// Auth in progress (verifying biometrics, PIN, etc.)
    Verifying,
    /// Auth succeeded — transitioning to desktop
    Unlocking,
    /// Auth failed — show error, return to Authenticating
    Failed { attempts: u32 },
    /// Too many failures — locked out for N seconds
    LockedOut { until_tick: u64 },
}

/// Authentication methods supported.
#[derive(Debug, Clone, Copy)]
pub enum AuthMethod {
    /// PIN code (4-8 digits)
    Pin,
    /// Password (alphanumeric)
    Password,
    /// Fingerprint reader (via Silo driver)
    Fingerprint,
    /// Face recognition (via camera Silo)
    FaceId,
    /// Synapse neural signature
    NeuralSig,
    /// Hardware security key (FIDO2)
    SecurityKey,
}

/// Clock display data.
#[derive(Debug, Clone)]
pub struct ClockDisplay {
    pub hours: u8,
    pub minutes: u8,
    pub day_of_week: String,
    pub date: String,
}

/// A notification shown on the lock screen.
#[derive(Debug, Clone)]
pub struct LockNotification {
    pub app_name: String,
    pub title: String,
    pub summary: String,
    pub icon_oid: u64,
    pub timestamp: u64,
    pub priority: NotificationPriority,
}

/// Notification priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationPriority {
    Low,
    Normal,
    High,
    Urgent,
}

/// The lock screen manager.
pub struct LockScreen {
    /// Current state
    pub state: LockState,
    /// Available auth methods
    pub auth_methods: alloc::vec::Vec<AuthMethod>,
    /// Active auth method
    pub active_method: AuthMethod,
    /// Clock data
    pub clock: ClockDisplay,
    /// Pending notifications
    pub notifications: alloc::vec::Vec<LockNotification>,
    /// PIN/password input buffer (masked)
    pub input_buffer: String,
    /// Max failed attempts before lockout (default: 5)
    pub max_attempts: u32,
    /// Lockout duration in ticks (default: 30s × tps)
    pub lockout_duration: u64,
    /// Background wallpaper OID (Prism object)
    pub wallpaper_oid: Option<u64>,
}

impl LockScreen {
    pub fn new() -> Self {
        LockScreen {
            state: LockState::Locked,
            auth_methods: alloc::vec![AuthMethod::Pin, AuthMethod::Password],
            active_method: AuthMethod::Pin,
            clock: ClockDisplay {
                hours: 0,
                minutes: 0,
                day_of_week: String::from("Monday"),
                date: String::from("March 4, 2026"),
            },
            notifications: alloc::vec::Vec::new(),
            input_buffer: String::new(),
            max_attempts: 5,
            lockout_duration: 3000, // ~30 seconds at 100 tps
            wallpaper_oid: None,
        }
    }

    /// Handle user input on the lock screen.
    pub fn on_key_press(&mut self, ch: char) {
        match &self.state {
            LockState::Locked => {
                // Any key → show auth input
                self.state = LockState::Authenticating;
            }
            LockState::Authenticating | LockState::Failed { .. } => {
                if ch == '\n' {
                    // Submit authentication
                    self.state = LockState::Verifying;
                    self.verify_auth();
                } else if ch == '\x08' {
                    // Backspace
                    self.input_buffer.pop();
                } else {
                    self.input_buffer.push(ch);
                }
            }
            LockState::LockedOut { until_tick } => {
                // Ignore input during lockout
                let _ = until_tick;
            }
            _ => {}
        }
    }

    /// Verify authentication credentials.
    fn verify_auth(&mut self) {
        // In production: hash the input and compare against stored credential
        // Genesis default PIN: "0000" (user sets custom PIN during OOBE)
        let valid = match self.active_method {
            AuthMethod::Pin => self.input_buffer == "0000",
            AuthMethod::Password => self.input_buffer == "qindows",
            _ => false,
        };

        if valid {
            self.state = LockState::Unlocking;
            self.input_buffer.clear();
        } else {
            let attempts = match &self.state {
                LockState::Failed { attempts } => *attempts + 1,
                _ => 1,
            };

            if attempts >= self.max_attempts {
                self.state = LockState::LockedOut {
                    until_tick: 0, // Would be set to now_ticks() + lockout_duration
                };
            } else {
                self.state = LockState::Failed { attempts };
            }
            self.input_buffer.clear();
        }
    }

    /// Lock the screen.
    pub fn lock(&mut self) {
        self.state = LockState::Locked;
        self.input_buffer.clear();
    }

    /// Check if the screen is unlocked.
    pub fn is_unlocked(&self) -> bool {
        self.state == LockState::Unlocking
    }

    /// Add a notification.
    pub fn add_notification(&mut self, notif: LockNotification) {
        // Keep max 10 notifications
        if self.notifications.len() >= 10 {
            self.notifications.remove(0);
        }
        self.notifications.push(notif);
    }

    /// Get the masked input display (e.g., "●●●●" for a 4-digit PIN).
    pub fn masked_input(&self) -> String {
        "●".repeat(self.input_buffer.len()).into()
    }
}
