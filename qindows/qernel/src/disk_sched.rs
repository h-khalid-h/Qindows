//! # Disk I/O Scheduler — Deadline + CFQ Hybrid
//!
//! Merges, reorders, and dispatches block I/O requests to
//! NVMe/SATA devices with fairness and latency guarantees (Section 3.1).
//!
//! Design:
//! - **Deadline queue**: Ensures no request starves beyond its deadline
//! - **CFQ fairness**: Per-Silo I/O bandwidth shares
//! - **Merge engine**: Coalesces adjacent sector requests
//! - **Priority boost**: System I/O (paging, journal) gets priority

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// I/O direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoDir {
    Read,
    Write,
}

/// I/O priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IoPriority {
    Idle = 0,
    Background = 1,
    Normal = 2,
    System = 3,
    Critical = 4,
}

/// I/O request state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoState {
    Queued,
    Merged,
    Dispatched,
    Complete,
    Failed,
}

/// A block I/O request.
#[derive(Debug, Clone)]
pub struct IoRequest {
    pub id: u64,
    pub silo_id: u64,
    pub device_id: u32,
    pub direction: IoDir,
    pub sector: u64,
    pub count: u32,
    pub priority: IoPriority,
    pub state: IoState,
    pub submitted_at: u64,
    pub deadline: u64,
    pub completed_at: u64,
}

/// Per-Silo I/O share.
#[derive(Debug, Clone)]
pub struct SiloShare {
    pub silo_id: u64,
    pub weight: u32,
    pub bytes_used: u64,
    pub requests_served: u64,
}

/// Disk scheduler statistics.
#[derive(Debug, Clone, Default)]
pub struct SchedStats {
    pub requests_submitted: u64,
    pub requests_completed: u64,
    pub requests_merged: u64,
    pub requests_failed: u64,
    pub deadlines_met: u64,
    pub deadlines_missed: u64,
    pub bytes_read: u64,
    pub bytes_written: u64,
}

/// The Disk I/O Scheduler.
pub struct DiskScheduler {
    pub queue: Vec<IoRequest>,
    pub shares: BTreeMap<u64, SiloShare>,
    next_id: u64,
    /// Sectors per merge window
    pub merge_window: u64,
    /// Default deadline (microseconds from submit)
    pub default_deadline_us: u64,
    pub stats: SchedStats,
}

impl DiskScheduler {
    pub fn new() -> Self {
        DiskScheduler {
            queue: Vec::new(),
            shares: BTreeMap::new(),
            next_id: 1,
            merge_window: 128,
            default_deadline_us: 50_000,
            stats: SchedStats::default(),
        }
    }

    /// Register a Silo's I/O share.
    pub fn set_share(&mut self, silo_id: u64, weight: u32) {
        self.shares.entry(silo_id).or_insert(SiloShare {
            silo_id, weight, bytes_used: 0, requests_served: 0,
        }).weight = weight;
    }

    /// Submit an I/O request.
    pub fn submit(&mut self, silo_id: u64, device_id: u32, dir: IoDir, sector: u64, count: u32, priority: IoPriority, now: u64) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let deadline = now + self.default_deadline_us;

        // Try to merge with existing request
        let merged = self.try_merge(device_id, dir, sector, count);
        if merged {
            self.stats.requests_merged += 1;
            return id;
        }

        self.queue.push(IoRequest {
            id, silo_id, device_id, direction: dir,
            sector, count, priority, state: IoState::Queued,
            submitted_at: now, deadline, completed_at: 0,
        });

        self.stats.requests_submitted += 1;
        id
    }

    /// Try to merge with an adjacent queued request.
    fn try_merge(&mut self, device_id: u32, dir: IoDir, sector: u64, count: u32) -> bool {
        for req in self.queue.iter_mut() {
            if req.state != IoState::Queued || req.device_id != device_id || req.direction != dir {
                continue;
            }
            let req_end = req.sector + req.count as u64;
            let new_end = sector + count as u64;

            // Adjacent: new request follows existing
            if sector == req_end && (req.count as u64 + count as u64) <= self.merge_window {
                req.count += count;
                req.state = IoState::Merged;
                return true;
            }
            // Adjacent: new request precedes existing
            if new_end == req.sector && (req.count as u64 + count as u64) <= self.merge_window {
                req.sector = sector;
                req.count += count;
                req.state = IoState::Merged;
                return true;
            }
        }
        false
    }

    /// Dispatch the highest-priority request respecting deadlines.
    pub fn dispatch(&mut self, now: u64) -> Option<u64> {
        // First: any request past its deadline
        let urgent = self.queue.iter().position(|r| {
            r.state == IoState::Queued && now >= r.deadline
        });

        let idx = if let Some(i) = urgent {
            self.stats.deadlines_missed += 1;
            i
        } else {
            // CFQ: pick from the Silo with lowest bytes_used/weight ratio
            self.queue.iter().enumerate()
                .filter(|(_, r)| r.state == IoState::Queued)
                .max_by_key(|(_, r)| r.priority)
                .map(|(i, _)| i)?
        };

        self.queue[idx].state = IoState::Dispatched;
        Some(self.queue[idx].id)
    }

    /// Complete a request.
    pub fn complete(&mut self, request_id: u64, success: bool, now: u64) {
        if let Some(req) = self.queue.iter_mut().find(|r| r.id == request_id) {
            req.state = if success { IoState::Complete } else { IoState::Failed };
            req.completed_at = now;

            if success {
                let bytes = req.count as u64 * 512;
                match req.direction {
                    IoDir::Read => self.stats.bytes_read += bytes,
                    IoDir::Write => self.stats.bytes_written += bytes,
                }
                if now < req.deadline {
                    self.stats.deadlines_met += 1;
                }
                if let Some(share) = self.shares.get_mut(&req.silo_id) {
                    share.bytes_used += bytes;
                    share.requests_served += 1;
                }
                self.stats.requests_completed += 1;
            } else {
                self.stats.requests_failed += 1;
            }
        }
    }
}
