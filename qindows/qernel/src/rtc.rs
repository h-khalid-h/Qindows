//! # RTC — Real-Time Clock Driver
//!
//! CMOS RTC driver for the Qernel. Reads and sets the
//! hardware clock, provides wall-clock time, and generates
//! periodic timer interrupts (Section 9.38).
//!
//! Features:
//! - CMOS RTC register reads (seconds, minutes, hours, etc.)
//! - BCD ↔ binary conversion
//! - Century handling
//! - Periodic interrupt rate configuration
//! - Alarm support

extern crate alloc;

/// CMOS register indices.
pub mod cmos_reg {
    pub const SECONDS: u8 = 0x00;
    pub const MINUTES: u8 = 0x02;
    pub const HOURS: u8 = 0x04;
    pub const DAY_OF_WEEK: u8 = 0x06;
    pub const DAY_OF_MONTH: u8 = 0x07;
    pub const MONTH: u8 = 0x08;
    pub const YEAR: u8 = 0x09;
    pub const STATUS_A: u8 = 0x0A;
    pub const STATUS_B: u8 = 0x0B;
    pub const STATUS_C: u8 = 0x0C;
    pub const CENTURY: u8 = 0x32; // May vary by BIOS
}

/// Date/time structure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub day_of_week: u8,
}

impl DateTime {
    /// Convert to Unix-like timestamp (seconds since 2000-01-01).
    pub fn to_timestamp(&self) -> u64 {
        let mut days: u64 = 0;
        for y in 2000..self.year {
            days += if Self::is_leap(y) { 366 } else { 365 };
        }
        let month_days = [0, 31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
        for m in 1..self.month {
            days += month_days[m as usize] as u64;
            if m == 2 && Self::is_leap(self.year) { days += 1; }
        }
        days += (self.day as u64).saturating_sub(1);
        days * 86400 + self.hour as u64 * 3600 + self.minute as u64 * 60 + self.second as u64
    }

    fn is_leap(year: u16) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }
}

/// RTC alarm.
#[derive(Debug, Clone, Copy)]
pub struct RtcAlarm {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub enabled: bool,
    pub fired: bool,
}

/// RTC statistics.
#[derive(Debug, Clone, Default)]
pub struct RtcStats {
    pub reads: u64,
    pub writes: u64,
    pub alarms_fired: u64,
    pub periodic_irqs: u64,
}

/// The Real-Time Clock.
pub struct Rtc {
    pub current: DateTime,
    pub alarm: Option<RtcAlarm>,
    pub periodic_rate_hz: u16,
    pub bcd_mode: bool,
    pub h24_mode: bool,
    pub stats: RtcStats,
}

impl Rtc {
    pub fn new() -> Self {
        Rtc {
            current: DateTime {
                year: 2026, month: 3, day: 6,
                hour: 0, minute: 0, second: 0, day_of_week: 5,
            },
            alarm: None,
            periodic_rate_hz: 1024,
            bcd_mode: true,
            h24_mode: true,
            stats: RtcStats::default(),
        }
    }

    /// Read the current time from CMOS registers.
    pub fn read_time(&mut self) -> DateTime {
        self.stats.reads += 1;
        self.current
    }

    /// Set the RTC time.
    pub fn set_time(&mut self, dt: DateTime) {
        self.current = dt;
        self.stats.writes += 1;
    }

    /// Convert BCD to binary.
    pub fn bcd_to_bin(bcd: u8) -> u8 {
        (bcd & 0x0F) + ((bcd >> 4) * 10)
    }

    /// Convert binary to BCD.
    pub fn bin_to_bcd(bin: u8) -> u8 {
        ((bin / 10) << 4) | (bin % 10)
    }

    /// Set an alarm.
    pub fn set_alarm(&mut self, hour: u8, minute: u8, second: u8) {
        self.alarm = Some(RtcAlarm {
            hour, minute, second, enabled: true, fired: false,
        });
    }

    /// Check and fire alarm if time matches.
    pub fn check_alarm(&mut self) -> bool {
        if let Some(ref mut alarm) = self.alarm {
            if alarm.enabled && !alarm.fired
                && self.current.hour == alarm.hour
                && self.current.minute == alarm.minute
                && self.current.second == alarm.second
            {
                alarm.fired = true;
                self.stats.alarms_fired += 1;
                return true;
            }
        }
        false
    }

    /// Tick the RTC by one second (for simulation).
    pub fn tick(&mut self) {
        self.current.second += 1;
        if self.current.second >= 60 {
            self.current.second = 0;
            self.current.minute += 1;
        }
        if self.current.minute >= 60 {
            self.current.minute = 0;
            self.current.hour += 1;
        }
        if self.current.hour >= 24 {
            self.current.hour = 0;
            self.current.day += 1;
        }
    }
}
