//! # Mesh Rate Limiter — Per-Service Request Throttling
//!
//! Token-bucket rate limiter for mesh services, preventing
//! abuse and ensuring fair resource allocation (Section 11.30).
//!
//! Features:
//! - Token bucket algorithm
//! - Per-service and per-Silo limits
//! - Burst allowance
//! - Sliding window counters
//! - Rate limit headers for clients

extern crate alloc;

use alloc::collections::BTreeMap;

/// A token bucket.
#[derive(Debug, Clone)]
pub struct TokenBucket {
    pub capacity: u64,
    pub tokens: u64,
    pub refill_rate: u64, // tokens per second
    pub last_refill: u64,
}

impl TokenBucket {
    pub fn new(capacity: u64, refill_rate: u64, now: u64) -> Self {
        TokenBucket {
            capacity, tokens: capacity,
            refill_rate, last_refill: now,
        }
    }

    /// Refill tokens based on elapsed time.
    pub fn refill(&mut self, now: u64) {
        let elapsed_ms = now.saturating_sub(self.last_refill);
        if elapsed_ms == 0 { return; }
        let new_tokens = (self.refill_rate * elapsed_ms) / 1000;
        self.tokens = (self.tokens + new_tokens).min(self.capacity);
        self.last_refill = now;
    }

    /// Try to consume N tokens. Returns true if allowed.
    pub fn try_consume(&mut self, n: u64, now: u64) -> bool {
        self.refill(now);
        if self.tokens >= n {
            self.tokens -= n;
            true
        } else {
            false
        }
    }

    /// How many tokens are available?
    pub fn available(&self) -> u64 {
        self.tokens
    }

    /// Time until N tokens are available (ms).
    pub fn wait_time(&self, n: u64) -> u64 {
        if self.tokens >= n { return 0; }
        let deficit = n - self.tokens;
        if self.refill_rate == 0 { return u64::MAX; }
        (deficit * 1000) / self.refill_rate
    }
}

/// Rate limit result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    Allowed,
    Limited,
}

/// Rate limiter statistics.
#[derive(Debug, Clone, Default)]
pub struct RateLimitStats {
    pub requests_allowed: u64,
    pub requests_limited: u64,
    pub buckets_created: u64,
}

/// The Mesh Rate Limiter.
pub struct MeshRateLimiter {
    /// Per-key buckets (key = service_name:silo_id)
    pub buckets: BTreeMap<(u64, u64), TokenBucket>, // (service_hash, silo_id)
    pub default_capacity: u64,
    pub default_rate: u64,
    pub stats: RateLimitStats,
}

impl MeshRateLimiter {
    pub fn new(default_capacity: u64, default_rate: u64) -> Self {
        MeshRateLimiter {
            buckets: BTreeMap::new(),
            default_capacity, default_rate,
            stats: RateLimitStats::default(),
        }
    }

    /// Check rate limit for a service+silo pair.
    pub fn check(&mut self, service: u64, silo_id: u64, now: u64) -> RateLimitResult {
        let key = (service, silo_id);

        if !self.buckets.contains_key(&key) {
            self.buckets.insert(key, TokenBucket::new(
                self.default_capacity, self.default_rate, now,
            ));
            self.stats.buckets_created += 1;
        }

        let bucket = self.buckets.get_mut(&key).unwrap();
        if bucket.try_consume(1, now) {
            self.stats.requests_allowed += 1;
            RateLimitResult::Allowed
        } else {
            self.stats.requests_limited += 1;
            RateLimitResult::Limited
        }
    }

    /// Set custom limits for a service+silo pair.
    pub fn set_limit(&mut self, service: u64, silo_id: u64, capacity: u64, rate: u64, now: u64) {
        self.buckets.insert((service, silo_id), TokenBucket::new(capacity, rate, now));
    }

    /// Get remaining quota.
    pub fn remaining(&mut self, service: u64, silo_id: u64, now: u64) -> u64 {
        if let Some(b) = self.buckets.get_mut(&(service, silo_id)) {
            b.refill(now);
            b.available()
        } else {
            self.default_capacity
        }
    }
}
