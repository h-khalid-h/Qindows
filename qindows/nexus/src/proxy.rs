//! # Nexus Network Proxy
//!
//! SOCKS5 and HTTP CONNECT proxy for per-Silo network isolation.
//! Each Silo's traffic can be routed through proxy rules,
//! providing domain-level access control, bandwidth metering,
//! and transparent tunneling to the Global Mesh.
//!
//! Integrates with `firewall.rs` for ACL enforcement and
//! `shaper.rs` for per-Silo bandwidth limiting.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

// ─── Proxy Types ────────────────────────────────────────────────────────────

/// Proxy protocol type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyType {
    /// SOCKS5 (RFC 1928) — supports TCP and UDP relay
    Socks5,
    /// HTTP CONNECT — TLS tunneling through HTTP proxy
    HttpConnect,
    /// Transparent proxy — intercepts at IP level
    Transparent,
}

/// SOCKS5 authentication method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// No authentication required
    NoAuth,
    /// Username/password (RFC 1929)
    UsernamePassword,
    /// Capability-based (Qindows native — Silo presents a capability token)
    CapabilityToken,
}

/// SOCKS5 address type.
#[derive(Debug, Clone)]
pub enum ProxyAddress {
    /// IPv4 address
    Ipv4([u8; 4], u16),
    /// Domain name (resolved by proxy)
    Domain(String, u16),
    /// IPv6 address
    Ipv6([u8; 16], u16),
}

impl ProxyAddress {
    /// Get the port.
    pub fn port(&self) -> u16 {
        match self {
            ProxyAddress::Ipv4(_, p) => *p,
            ProxyAddress::Domain(_, p) => *p,
            ProxyAddress::Ipv6(_, p) => *p,
        }
    }

    /// Get a display string.
    pub fn display(&self) -> String {
        match self {
            ProxyAddress::Ipv4(ip, port) => {
                alloc::format!("{}.{}.{}.{}:{}", ip[0], ip[1], ip[2], ip[3], port)
            }
            ProxyAddress::Domain(name, port) => {
                alloc::format!("{}:{}", name, port)
            }
            ProxyAddress::Ipv6(_, port) => {
                alloc::format!("[::ipv6]:{}", port)
            }
        }
    }
}

// ─── Proxy Rules ────────────────────────────────────────────────────────────

/// Action to take for a matching rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleAction {
    /// Allow the connection
    Allow,
    /// Block the connection
    Block,
    /// Redirect to a different destination
    Redirect,
    /// Allow but log all traffic
    AllowAndLog,
}

/// A domain match pattern.
#[derive(Debug, Clone)]
pub enum DomainMatch {
    /// Exact domain match
    Exact(String),
    /// Suffix match (e.g., *.example.com)
    Suffix(String),
    /// All domains
    Any,
}

impl DomainMatch {
    pub fn matches(&self, domain: &str) -> bool {
        match self {
            DomainMatch::Exact(d) => domain == d,
            DomainMatch::Suffix(suffix) => domain.ends_with(suffix.as_str()),
            DomainMatch::Any => true,
        }
    }
}

/// A proxy rule.
#[derive(Debug, Clone)]
pub struct ProxyRule {
    /// Rule ID
    pub id: u64,
    /// Which Silo this rule applies to (None = all Silos)
    pub silo_id: Option<u64>,
    /// Domain pattern to match
    pub domain: DomainMatch,
    /// Port to match (None = any port)
    pub port: Option<u16>,
    /// Action to take
    pub action: RuleAction,
    /// Redirect target (only used with RuleAction::Redirect)
    pub redirect_to: Option<ProxyAddress>,
    /// Priority (lower = higher priority)
    pub priority: u32,
}

// ─── Proxy Sessions ─────────────────────────────────────────────────────────

/// State of a proxy session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// SOCKS5 handshake in progress
    Handshaking,
    /// Authenticating
    Authenticating,
    /// Connecting to destination
    Connecting,
    /// Tunnel established, relaying data
    Relaying,
    /// Closing
    Closing,
    /// Closed
    Closed,
}

/// A proxy tunneling session.
#[derive(Debug, Clone)]
pub struct ProxySession {
    /// Session ID
    pub id: u64,
    /// Silo ID that initiated this connection
    pub silo_id: u64,
    /// Proxy protocol
    pub proxy_type: ProxyType,
    /// Session state
    pub state: SessionState,
    /// Destination address
    pub destination: ProxyAddress,
    /// Auth method used
    pub auth: AuthMethod,
    /// Bytes sent (client → destination)
    pub bytes_sent: u64,
    /// Bytes received (destination → client)
    pub bytes_recv: u64,
    /// Session start time (ns)
    pub started_at: u64,
    /// Last activity (ns)
    pub last_activity: u64,
    /// Was the connection logged?
    pub logged: bool,
}

// ─── Proxy Server ───────────────────────────────────────────────────────────

/// Proxy statistics.
#[derive(Debug, Clone, Default)]
pub struct ProxyStats {
    pub sessions_created: u64,
    pub sessions_completed: u64,
    pub sessions_blocked: u64,
    pub bytes_relayed: u64,
    pub auth_failures: u64,
    pub rule_matches: u64,
}

/// The Proxy Server.
pub struct ProxyServer {
    /// Active sessions
    pub sessions: BTreeMap<u64, ProxySession>,
    /// Access control rules (sorted by priority)
    pub rules: Vec<ProxyRule>,
    /// Default action when no rule matches
    pub default_action: RuleAction,
    /// Supported auth methods
    pub auth_methods: Vec<AuthMethod>,
    /// Session idle timeout (ns)
    pub idle_timeout_ns: u64,
    /// Max sessions per Silo
    pub max_sessions_per_silo: usize,
    /// Next session ID
    next_session_id: u64,
    /// Next rule ID
    next_rule_id: u64,
    /// Statistics
    pub stats: ProxyStats,
}

impl ProxyServer {
    pub fn new() -> Self {
        ProxyServer {
            sessions: BTreeMap::new(),
            rules: Vec::new(),
            default_action: RuleAction::Allow,
            auth_methods: alloc::vec![AuthMethod::CapabilityToken, AuthMethod::NoAuth],
            idle_timeout_ns: 120_000_000_000, // 2 minutes
            max_sessions_per_silo: 64,
            next_session_id: 1,
            next_rule_id: 1,
            stats: ProxyStats::default(),
        }
    }

    /// Add an access control rule.
    pub fn add_rule(
        &mut self,
        silo_id: Option<u64>,
        domain: DomainMatch,
        port: Option<u16>,
        action: RuleAction,
        priority: u32,
    ) -> u64 {
        let id = self.next_rule_id;
        self.next_rule_id += 1;

        let rule = ProxyRule {
            id,
            silo_id,
            domain,
            port,
            action,
            redirect_to: None,
            priority,
        };

        self.rules.push(rule);
        // Keep sorted by priority (ascending = higher priority first)
        self.rules.sort_by_key(|r| r.priority);

        id
    }

    /// Remove a rule by ID.
    pub fn remove_rule(&mut self, rule_id: u64) {
        self.rules.retain(|r| r.id != rule_id);
    }

    /// Open a new proxy session (CONNECT request).
    pub fn connect(
        &mut self,
        silo_id: u64,
        destination: ProxyAddress,
        proxy_type: ProxyType,
        now: u64,
    ) -> Result<u64, ProxyError> {
        // Check per-Silo session limit
        let silo_sessions = self.sessions.values()
            .filter(|s| s.silo_id == silo_id && s.state != SessionState::Closed)
            .count();
        if silo_sessions >= self.max_sessions_per_silo {
            return Err(ProxyError::TooManySessions);
        }

        // Evaluate rules
        let action = self.evaluate_rules(silo_id, &destination);
        self.stats.rule_matches += 1;

        match action {
            RuleAction::Block => {
                self.stats.sessions_blocked += 1;
                return Err(ProxyError::Blocked);
            }
            RuleAction::Redirect => {
                // Would redirect to alternate destination
            }
            _ => {}
        }

        // Create session
        let id = self.next_session_id;
        self.next_session_id += 1;

        let session = ProxySession {
            id,
            silo_id,
            proxy_type,
            state: SessionState::Relaying,
            destination,
            auth: AuthMethod::CapabilityToken,
            bytes_sent: 0,
            bytes_recv: 0,
            started_at: now,
            last_activity: now,
            logged: action == RuleAction::AllowAndLog,
        };

        self.sessions.insert(id, session);
        self.stats.sessions_created += 1;

        Ok(id)
    }

    /// Relay data through a session.
    pub fn relay(&mut self, session_id: u64, sent: u64, recv: u64, now: u64) -> Result<(), ProxyError> {
        let session = self.sessions.get_mut(&session_id)
            .ok_or(ProxyError::SessionNotFound)?;

        if session.state != SessionState::Relaying {
            return Err(ProxyError::InvalidState);
        }

        session.bytes_sent += sent;
        session.bytes_recv += recv;
        session.last_activity = now;
        self.stats.bytes_relayed += sent + recv;

        Ok(())
    }

    /// Close a proxy session.
    pub fn close_session(&mut self, session_id: u64) {
        if let Some(session) = self.sessions.get_mut(&session_id) {
            session.state = SessionState::Closed;
            self.stats.sessions_completed += 1;
        }
    }

    /// Evaluate rules for a destination, returning the action.
    fn evaluate_rules(&self, silo_id: u64, dest: &ProxyAddress) -> RuleAction {
        let domain = match dest {
            ProxyAddress::Domain(d, _) => d.as_str(),
            _ => "",
        };
        let port = dest.port();

        for rule in &self.rules {
            // Check Silo match
            if let Some(rule_silo) = rule.silo_id {
                if rule_silo != silo_id { continue; }
            }

            // Check domain match
            if !rule.domain.matches(domain) { continue; }

            // Check port match
            if let Some(rule_port) = rule.port {
                if rule_port != port { continue; }
            }

            return rule.action;
        }

        self.default_action
    }

    /// Clean up idle and closed sessions.
    pub fn maintenance(&mut self, now: u64) {
        let timeout = self.idle_timeout_ns;
        let timed_out: Vec<u64> = self.sessions.iter()
            .filter(|(_, s)| {
                s.state == SessionState::Relaying
                    && now.saturating_sub(s.last_activity) > timeout
            })
            .map(|(&id, _)| id)
            .collect();

        for id in timed_out {
            if let Some(session) = self.sessions.get_mut(&id) {
                session.state = SessionState::Closed;
                self.stats.sessions_completed += 1;
            }
        }

        // Remove closed sessions
        self.sessions.retain(|_, s| s.state != SessionState::Closed);
    }

    /// Get active session count for a Silo.
    pub fn silo_session_count(&self, silo_id: u64) -> usize {
        self.sessions.values()
            .filter(|s| s.silo_id == silo_id && s.state != SessionState::Closed)
            .count()
    }

    /// Get total bytes relayed for a Silo.
    pub fn silo_bytes(&self, silo_id: u64) -> (u64, u64) {
        let mut sent = 0u64;
        let mut recv = 0u64;
        for s in self.sessions.values() {
            if s.silo_id == silo_id {
                sent += s.bytes_sent;
                recv += s.bytes_recv;
            }
        }
        (sent, recv)
    }
}

/// Proxy errors.
#[derive(Debug, Clone)]
pub enum ProxyError {
    /// Connection blocked by rule
    Blocked,
    /// Too many sessions for this Silo
    TooManySessions,
    /// Session not found
    SessionNotFound,
    /// Session in wrong state
    InvalidState,
    /// Authentication failed
    AuthFailed,
    /// Destination unreachable
    Unreachable,
}
