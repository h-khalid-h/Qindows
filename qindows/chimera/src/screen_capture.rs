//! # Screen Capture — Per-Silo Screenshot & Recording
//!
//! Captures screen content with strict Silo isolation
//! (Section 7.5). A Silo can only capture its own windows.
//!
//! Features:
//! - Screenshot (single frame capture)
//! - Screen recording (frame sequence)
//! - Per-Silo isolation (cannot capture other Silo's content)
//! - Capability-gated cross-Silo capture (admin/debug)
//! - Output format: raw framebuffer, PNG metadata

extern crate alloc;

use alloc::collections::BTreeMap;

/// Capture type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureType {
    Screenshot,
    Recording,
}

/// Capture state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    Ready,
    Capturing,
    Complete,
    Failed,
}

/// A capture job.
#[derive(Debug, Clone)]
pub struct CaptureJob {
    pub id: u64,
    pub silo_id: u64,
    pub capture_type: CaptureType,
    pub state: CaptureState,
    pub width: u32,
    pub height: u32,
    pub frames: u32,
    pub total_bytes: u64,
    pub started_at: u64,
    pub completed_at: u64,
}

/// Capture statistics.
#[derive(Debug, Clone, Default)]
pub struct CaptureStats {
    pub screenshots_taken: u64,
    pub recordings_started: u64,
    pub recordings_completed: u64,
    pub bytes_captured: u64,
    pub cross_silo_denied: u64,
}

/// The Screen Capture Manager.
pub struct ScreenCapture {
    pub jobs: BTreeMap<u64, CaptureJob>,
    next_id: u64,
    pub max_recording_frames: u32,
    pub stats: CaptureStats,
}

impl ScreenCapture {
    pub fn new() -> Self {
        ScreenCapture {
            jobs: BTreeMap::new(),
            next_id: 1,
            max_recording_frames: 36000, // 10 minutes at 60fps
            stats: CaptureStats::default(),
        }
    }

    /// Take a screenshot.
    pub fn screenshot(&mut self, silo_id: u64, width: u32, height: u32, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let bytes = (width as u64) * (height as u64) * 4; // RGBA

        self.jobs.insert(id, CaptureJob {
            id, silo_id, capture_type: CaptureType::Screenshot,
            state: CaptureState::Complete,
            width, height, frames: 1, total_bytes: bytes,
            started_at: now, completed_at: now,
        });

        self.stats.screenshots_taken += 1;
        self.stats.bytes_captured += bytes;
        id
    }

    /// Start a recording.
    pub fn start_recording(&mut self, silo_id: u64, width: u32, height: u32, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        self.jobs.insert(id, CaptureJob {
            id, silo_id, capture_type: CaptureType::Recording,
            state: CaptureState::Capturing,
            width, height, frames: 0, total_bytes: 0,
            started_at: now, completed_at: 0,
        });

        self.stats.recordings_started += 1;
        id
    }

    /// Add a frame to a recording.
    pub fn add_frame(&mut self, job_id: u64) -> Result<(), &'static str> {
        let job = self.jobs.get_mut(&job_id).ok_or("Job not found")?;
        if job.state != CaptureState::Capturing {
            return Err("Not recording");
        }
        if job.frames >= self.max_recording_frames {
            return Err("Max frames reached");
        }

        let frame_bytes = (job.width as u64) * (job.height as u64) * 4;
        job.frames += 1;
        job.total_bytes += frame_bytes;
        self.stats.bytes_captured += frame_bytes;
        Ok(())
    }

    /// Stop a recording.
    pub fn stop_recording(&mut self, job_id: u64, now: u64) {
        if let Some(job) = self.jobs.get_mut(&job_id) {
            if job.state == CaptureState::Capturing {
                job.state = CaptureState::Complete;
                job.completed_at = now;
                self.stats.recordings_completed += 1;
            }
        }
    }

    /// Check if a Silo can capture (only own windows).
    pub fn can_capture(&mut self, requester_silo: u64, target_silo: u64, has_admin_cap: bool) -> bool {
        if requester_silo == target_silo {
            return true;
        }
        if has_admin_cap {
            return true;
        }
        self.stats.cross_silo_denied += 1;
        false
    }
}
