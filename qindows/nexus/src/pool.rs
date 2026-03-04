//! # Nexus Connection Pool
//!
//! Manages reusable network connections to reduce handshake
//! overhead. Supports idle timeout, health checks, and
//! per-destination connection limits.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    /// Ready for use
    Idle,
    /// Currently in use
    Active,
    /// Being established
    Connecting,
    /// Marked for removal
    Closing,
    /// Dead (failed health check)
    Dead,
}

/// A pooled connection.
#[derive(Debug, Clone)]
pub struct PooledConn {
    /// Connection ID
    pub id: u64,
    /// Destination key (host:port or peer ID)
    pub destination: String,
    /// Connection state
    pub state: ConnState,
    /// Last activity timestamp (ns)
    pub last_used: u64,
    /// Creation timestamp
    pub created_at: u64,
    /// Bytes sent over this connection
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_recv: u64,
    /// Number of times reused
    pub reuse_count: u32,
    /// Health check failures
    pub health_failures: u32,
}

/// Pool configuration.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Max connections per destination
    pub max_per_dest: usize,
    /// Max total connections
    pub max_total: usize,
    /// Idle timeout (ns) — close idle connections after this
    pub idle_timeout_ns: u64,
    /// Max lifetime of a connection (ns)
    pub max_lifetime_ns: u64,
    /// Health check interval (ns)
    pub health_check_interval_ns: u64,
    /// Max health check failures before removal
    pub max_health_failures: u32,
}

impl Default for PoolConfig {
    fn default() -> Self {
        PoolConfig {
            max_per_dest: 8,
            max_total: 128,
            idle_timeout_ns: 60_000_000_000, // 60s
            max_lifetime_ns: 300_000_000_000, // 5min
            health_check_interval_ns: 30_000_000_000, // 30s
            max_health_failures: 3,
        }
    }
}

/// Pool statistics.
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    pub connections_created: u64,
    pub connections_reused: u64,
    pub connections_closed: u64,
    pub connections_evicted: u64,
    pub health_checks_passed: u64,
    pub health_checks_failed: u64,
    pub pool_exhausted_count: u64,
}

/// The Connection Pool.
pub struct ConnectionPool {
    /// All connections
    pub connections: Vec<PooledConn>,
    /// Per-destination counts
    pub dest_counts: BTreeMap<String, usize>,
    /// Configuration
    pub config: PoolConfig,
    /// Next connection ID
    next_id: u64,
    /// Stats
    pub stats: PoolStats,
}

impl ConnectionPool {
    pub fn new(config: PoolConfig) -> Self {
        ConnectionPool {
            connections: Vec::new(),
            dest_counts: BTreeMap::new(),
            config,
            next_id: 1,
            stats: PoolStats::default(),
        }
    }

    /// Acquire a connection to a destination.
    /// Returns an existing idle connection or creates a new one.
    pub fn acquire(&mut self, destination: &str, now_ns: u64) -> Option<u64> {
        // Try to reuse an idle connection
        if let Some(conn) = self.connections.iter_mut()
            .find(|c| c.destination == destination && c.state == ConnState::Idle)
        {
            conn.state = ConnState::Active;
            conn.last_used = now_ns;
            conn.reuse_count += 1;
            self.stats.connections_reused += 1;
            return Some(conn.id);
        }

        // Check per-destination limit
        let dest_count = self.dest_counts.get(destination).copied().unwrap_or(0);
        if dest_count >= self.config.max_per_dest {
            self.stats.pool_exhausted_count += 1;
            return None;
        }

        // Check total limit
        let active_count = self.connections.iter()
            .filter(|c| c.state != ConnState::Dead && c.state != ConnState::Closing)
            .count();
        if active_count >= self.config.max_total {
            // Try to evict an idle connection to a different destination
            if !self.evict_one(now_ns) {
                self.stats.pool_exhausted_count += 1;
                return None;
            }
        }

        // Create new connection
        let id = self.next_id;
        self.next_id += 1;

        self.connections.push(PooledConn {
            id,
            destination: String::from(destination),
            state: ConnState::Active,
            last_used: now_ns,
            created_at: now_ns,
            bytes_sent: 0,
            bytes_recv: 0,
            reuse_count: 0,
            health_failures: 0,
        });

        *self.dest_counts.entry(String::from(destination)).or_insert(0) += 1;
        self.stats.connections_created += 1;

        Some(id)
    }

    /// Release a connection back to the pool.
    pub fn release(&mut self, conn_id: u64, now_ns: u64) {
        if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
            conn.state = ConnState::Idle;
            conn.last_used = now_ns;
        }
    }

    /// Report bytes sent/received on a connection.
    pub fn report_io(&mut self, conn_id: u64, sent: u64, recv: u64) {
        if let Some(conn) = self.connections.iter_mut().find(|c| c.id == conn_id) {
            conn.bytes_sent += sent;
            conn.bytes_recv += recv;
        }
    }

    /// Run idle timeout and lifetime checks.
    pub fn maintenance(&mut self, now_ns: u64) {
        for conn in &mut self.connections {
            if conn.state == ConnState::Dead || conn.state == ConnState::Closing {
                continue;
            }

            // Idle timeout
            if conn.state == ConnState::Idle && now_ns.saturating_sub(conn.last_used) > self.config.idle_timeout_ns {
                conn.state = ConnState::Closing;
                self.stats.connections_evicted += 1;
            }

            // Max lifetime
            if now_ns.saturating_sub(conn.created_at) > self.config.max_lifetime_ns {
                conn.state = ConnState::Closing;
                self.stats.connections_evicted += 1;
            }
        }

        // Remove closed/dead connections
        let before = self.connections.len();
        self.connections.retain(|c| c.state != ConnState::Closing && c.state != ConnState::Dead);

        // Update dest counts
        self.dest_counts.clear();
        for conn in &self.connections {
            *self.dest_counts.entry(conn.destination.clone()).or_insert(0) += 1;
        }

        self.stats.connections_closed += (before - self.connections.len()) as u64;
    }

    /// Evict the oldest idle connection. Returns true if one was evicted.
    fn evict_one(&mut self, _now_ns: u64) -> bool {
        // Find the oldest idle connection
        let candidate = self.connections.iter()
            .enumerate()
            .filter(|(_, c)| c.state == ConnState::Idle)
            .min_by_key(|(_, c)| c.last_used)
            .map(|(i, _)| i);

        if let Some(idx) = candidate {
            self.connections[idx].state = ConnState::Closing;
            self.stats.connections_evicted += 1;
            true
        } else {
            false
        }
    }

    /// Get pool summary.
    pub fn summary(&self) -> (usize, usize, usize) {
        let idle = self.connections.iter().filter(|c| c.state == ConnState::Idle).count();
        let active = self.connections.iter().filter(|c| c.state == ConnState::Active).count();
        let total = self.connections.len();
        (idle, active, total)
    }
}
