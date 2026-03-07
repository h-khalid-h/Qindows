//! # Nexus Bandwidth Shaper
//!
//! Traffic shaping and rate limiting for Nexus network connections.
//! Implements token-bucket rate limiting, per-Silo bandwidth quotas,
//! priority queues, and QoS classification.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// QoS traffic class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TrafficClass {
    /// Real-time (VoIP, video calls) — highest priority
    Realtime = 0,
    /// Interactive (gaming, remote desktop)
    Interactive = 1,
    /// Bulk transfer (file sync, backup)
    Bulk = 2,
    /// Background (updates, telemetry)
    Background = 3,
    /// Best-effort (default)
    BestEffort = 4,
}

/// A token bucket rate limiter.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    /// Maximum tokens (burst capacity)
    pub capacity: u64,
    /// Current tokens available
    pub tokens: u64,
    /// Tokens added per second (sustained rate)
    pub rate: u64,
    /// Last refill timestamp (ns)
    pub last_refill: u64,
}

impl TokenBucket {
    pub fn new(rate_bytes_per_sec: u64, burst_bytes: u64) -> Self {
        TokenBucket {
            capacity: burst_bytes,
            tokens: burst_bytes, // Start full
            rate: rate_bytes_per_sec,
            last_refill: 0,
        }
    }

    /// Refill tokens based on elapsed time.
    pub fn refill(&mut self, now_ns: u64) {
        if now_ns <= self.last_refill { return; }

        let elapsed_ns = now_ns - self.last_refill;
        // tokens_to_add = rate * elapsed_seconds
        let tokens_to_add = (self.rate as u128 * elapsed_ns as u128 / 1_000_000_000) as u64;

        self.tokens = (self.tokens + tokens_to_add).min(self.capacity);
        self.last_refill = now_ns;
    }

    /// Try to consume `bytes` tokens. Returns true if allowed.
    pub fn try_consume(&mut self, bytes: u64, now_ns: u64) -> bool {
        self.refill(now_ns);
        if self.tokens >= bytes {
            self.tokens -= bytes;
            true
        } else {
            false
        }
    }

    /// How long until `bytes` tokens are available? (ns)
    pub fn wait_time(&self, bytes: u64) -> u64 {
        if self.tokens >= bytes { return 0; }
        let deficit = bytes - self.tokens;
        if self.rate == 0 { return u64::MAX; }
        (deficit as u128 * 1_000_000_000 / self.rate as u128) as u64
    }
}

/// Per-Silo bandwidth quota.
#[derive(Debug, Clone)]
pub struct BandwidthQuota {
    /// Silo ID
    pub silo_id: u64,
    /// Upload rate limiter
    pub upload: TokenBucket,
    /// Download rate limiter
    pub download: TokenBucket,
    /// Traffic class
    pub traffic_class: TrafficClass,
    /// Total bytes uploaded
    pub total_uploaded: u64,
    /// Total bytes downloaded
    pub total_downloaded: u64,
    /// Packets dropped due to rate limit
    pub dropped_packets: u64,
}

/// A queued packet waiting for bandwidth.
#[derive(Debug, Clone)]
pub struct QueuedPacket {
    /// Packet data size
    pub size: u64,
    /// Source Silo
    pub silo_id: u64,
    /// Traffic class
    pub class: TrafficClass,
    /// Enqueue timestamp
    pub queued_at: u64,
    /// Destination identifier
    pub dest: u64,
}

/// The Bandwidth Shaper.
pub struct BandwidthShaper {
    /// Per-Silo quotas
    pub quotas: BTreeMap<u64, BandwidthQuota>,
    /// Global rate limiter (total system bandwidth)
    pub global_upload: TokenBucket,
    pub global_download: TokenBucket,
    /// Priority queues (one per traffic class)
    pub queues: BTreeMap<TrafficClass, Vec<QueuedPacket>>,
    /// Default rate for new Silos (bytes/sec)
    pub default_upload_rate: u64,
    pub default_download_rate: u64,
    /// Stats
    pub stats: ShaperStats,
}

/// Shaper statistics.
#[derive(Debug, Clone, Default)]
pub struct ShaperStats {
    pub total_packets_shaped: u64,
    pub total_packets_dropped: u64,
    pub total_bytes_shaped: u64,
    pub total_queued: u64,
    pub peak_queue_depth: usize,
}

impl BandwidthShaper {
    pub fn new(global_upload_bps: u64, global_download_bps: u64) -> Self {
        let mut queues = BTreeMap::new();
        queues.insert(TrafficClass::Realtime, Vec::new());
        queues.insert(TrafficClass::Interactive, Vec::new());
        queues.insert(TrafficClass::Bulk, Vec::new());
        queues.insert(TrafficClass::Background, Vec::new());
        queues.insert(TrafficClass::BestEffort, Vec::new());

        BandwidthShaper {
            quotas: BTreeMap::new(),
            global_upload: TokenBucket::new(global_upload_bps, global_upload_bps * 2),
            global_download: TokenBucket::new(global_download_bps, global_download_bps * 2),
            queues,
            default_upload_rate: 10 * 1024 * 1024, // 10 MB/s default
            default_download_rate: 50 * 1024 * 1024, // 50 MB/s default
            stats: ShaperStats::default(),
        }
    }

    /// Register a Silo with bandwidth quota.
    pub fn register_silo(&mut self, silo_id: u64, upload_bps: u64, download_bps: u64, class: TrafficClass) {
        self.quotas.insert(silo_id, BandwidthQuota {
            silo_id,
            upload: TokenBucket::new(upload_bps, upload_bps * 2),
            download: TokenBucket::new(download_bps, download_bps * 2),
            traffic_class: class,
            total_uploaded: 0,
            total_downloaded: 0,
            dropped_packets: 0,
        });
    }

    /// Try to send a packet (upload). Returns true if allowed.
    pub fn try_send(&mut self, silo_id: u64, size: u64, now_ns: u64) -> bool {
        // Check global limit first
        if !self.global_upload.try_consume(size, now_ns) {
            self.stats.total_packets_dropped += 1;
            return false;
        }

        // Check per-Silo limit
        let quota = self.quotas.entry(silo_id).or_insert_with(|| {
            BandwidthQuota {
                silo_id,
                upload: TokenBucket::new(self.default_upload_rate, self.default_upload_rate * 2),
                download: TokenBucket::new(self.default_download_rate, self.default_download_rate * 2),
                traffic_class: TrafficClass::BestEffort,
                total_uploaded: 0,
                total_downloaded: 0,
                dropped_packets: 0,
            }
        });

        if quota.upload.try_consume(size, now_ns) {
            quota.total_uploaded += size;
            self.stats.total_packets_shaped += 1;
            self.stats.total_bytes_shaped += size;
            true
        } else {
            quota.dropped_packets += 1;
            self.stats.total_packets_dropped += 1;
            false
        }
    }

    /// Try to receive a packet (download). Returns true if allowed.
    pub fn try_recv(&mut self, silo_id: u64, size: u64, now_ns: u64) -> bool {
        if !self.global_download.try_consume(size, now_ns) {
            self.stats.total_packets_dropped += 1;
            return false;
        }

        if let Some(quota) = self.quotas.get_mut(&silo_id) {
            if quota.download.try_consume(size, now_ns) {
                quota.total_downloaded += size;
                self.stats.total_packets_shaped += 1;
                self.stats.total_bytes_shaped += size;
                true
            } else {
                quota.dropped_packets += 1;
                self.stats.total_packets_dropped += 1;
                false
            }
        } else {
            true // No quota registered = unlimited
        }
    }

    /// Enqueue a packet that was rate-limited.
    pub fn enqueue(&mut self, packet: QueuedPacket) {
        let class = packet.class;
        if let Some(queue) = self.queues.get_mut(&class) {
            queue.push(packet);
            self.stats.total_queued += 1;
            let depth: usize = self.queues.values().map(|q| q.len()).sum();
            if depth > self.stats.peak_queue_depth {
                self.stats.peak_queue_depth = depth;
            }
        }
    }

    /// Drain queued packets (highest priority first).
    pub fn drain_queues(&mut self, now_ns: u64) -> Vec<QueuedPacket> {
        let mut drained = Vec::new();
        let classes = [
            TrafficClass::Realtime,
            TrafficClass::Interactive,
            TrafficClass::BestEffort,
            TrafficClass::Bulk,
            TrafficClass::Background,
        ];

        for class in &classes {
            if let Some(queue) = self.queues.get_mut(class) {
                let mut remaining = Vec::new();
                for pkt in queue.drain(..) {
                    if self.global_upload.try_consume(pkt.size, now_ns) {
                        drained.push(pkt);
                    } else {
                        remaining.push(pkt);
                    }
                }
                *queue = remaining;
            }
        }

        drained
    }

    /// Remove a Silo's quota.
    pub fn remove_silo(&mut self, silo_id: u64) {
        self.quotas.remove(&silo_id);
    }
}
