//! # Nexus NAT Traversal
//!
//! Enables peer-to-peer connectivity across NATs and firewalls.
//! Uses STUN-like hole-punching and relay fallback to ensure
//! any two Qindows nodes can communicate on the Global Mesh.

#![allow(dead_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// NAT type detected by probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NatType {
    /// No NAT (public IP)
    None,
    /// Full cone NAT (easiest to traverse)
    FullCone,
    /// Address-restricted cone
    AddressRestricted,
    /// Port-restricted cone
    PortRestricted,
    /// Symmetric NAT (hardest)
    Symmetric,
    /// Unknown (not yet probed)
    Unknown,
}

/// Connection method used to reach a peer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnMethod {
    /// Direct (both peers on public IPs)
    Direct,
    /// UDP hole-punching succeeded
    HolePunch,
    /// TCP simultaneous open
    TcpSimOpen,
    /// Using a relay node
    Relayed,
    /// Using UPnP port mapping
    Upnp,
}

/// A STUN server endpoint.
#[derive(Debug, Clone)]
pub struct StunServer {
    pub addr: [u8; 4],
    pub port: u16,
    pub name: String,
}

/// Result of a STUN binding request.
#[derive(Debug, Clone)]
pub struct StunResult {
    /// Our mapped (external) IP
    pub external_ip: [u8; 4],
    /// Our mapped (external) port
    pub external_port: u16,
    /// Detected NAT type
    pub nat_type: NatType,
    /// Server-reflexive address
    pub server_reflexive: bool,
}

/// A candidate address for connectivity checks.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// IP address
    pub addr: [u8; 4],
    /// Port
    pub port: u16,
    /// Candidate type
    pub ctype: CandidateType,
    /// Priority (higher = preferred)
    pub priority: u32,
}

/// Candidate types (ICE-style).
#[derive(Debug, Clone, Copy)]
pub enum CandidateType {
    /// Host (local interface)
    Host,
    /// Server reflexive (from STUN)
    ServerReflexive,
    /// Peer reflexive (discovered during checks)
    PeerReflexive,
    /// Relay (from TURN-like server)
    Relay,
}

/// State of a connectivity check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Waiting,
    InProgress,
    Succeeded,
    Failed,
}

/// A candidate pair being tested.
#[derive(Debug, Clone)]
pub struct CandidatePair {
    pub local: Candidate,
    pub remote: Candidate,
    pub state: CheckState,
    pub priority: u64,
    pub rtt_ms: Option<u32>,
}

/// The NAT Traversal Engine.
pub struct NatTraversal {
    /// Our local candidates
    pub local_candidates: Vec<Candidate>,
    /// Remote peer candidates
    pub remote_candidates: Vec<Candidate>,
    /// Candidate pairs (sorted by priority)
    pub pairs: Vec<CandidatePair>,
    /// Detected NAT type
    pub nat_type: NatType,
    /// STUN servers
    pub stun_servers: Vec<StunServer>,
    /// Selected pair (the winner)
    pub selected_pair: Option<usize>,
    /// Connection method that succeeded
    pub method: Option<ConnMethod>,
    /// Stats
    pub stats: NatStats,
}

/// NAT traversal statistics.
#[derive(Debug, Clone, Default)]
pub struct NatStats {
    pub stun_requests: u64,
    pub stun_responses: u64,
    pub hole_punch_attempts: u64,
    pub hole_punch_successes: u64,
    pub relay_fallbacks: u64,
    pub total_connections: u64,
}

impl NatTraversal {
    pub fn new() -> Self {
        NatTraversal {
            local_candidates: Vec::new(),
            remote_candidates: Vec::new(),
            pairs: Vec::new(),
            nat_type: NatType::Unknown,
            stun_servers: alloc::vec![
                StunServer { addr: [74, 125, 250, 129], port: 19302, name: String::from("stun.l.google.com") },
                StunServer { addr: [64, 233, 163, 127], port: 3478, name: String::from("stun.services.mozilla.com") },
            ],
            selected_pair: None,
            method: None,
            stats: NatStats::default(),
        }
    }

    /// Gather local candidates (host addresses).
    pub fn gather_local(&mut self, local_ip: [u8; 4], local_port: u16) {
        self.local_candidates.push(Candidate {
            addr: local_ip,
            port: local_port,
            ctype: CandidateType::Host,
            priority: 65535,
        });

        // Loopback
        self.local_candidates.push(Candidate {
            addr: [127, 0, 0, 1],
            port: local_port,
            ctype: CandidateType::Host,
            priority: 32768,
        });
    }

    /// Process STUN response and add server-reflexive candidate.
    pub fn process_stun_response(&mut self, result: StunResult) {
        self.stats.stun_responses += 1;
        self.nat_type = result.nat_type;

        self.local_candidates.push(Candidate {
            addr: result.external_ip,
            port: result.external_port,
            ctype: CandidateType::ServerReflexive,
            priority: 50000,
        });
    }

    /// Add remote peer candidates (received via signaling).
    pub fn add_remote_candidates(&mut self, candidates: Vec<Candidate>) {
        self.remote_candidates.extend(candidates);
    }

    /// Form candidate pairs for connectivity checks.
    pub fn form_pairs(&mut self) {
        self.pairs.clear();

        for local in &self.local_candidates {
            for remote in &self.remote_candidates {
                let priority = (local.priority as u64) * (remote.priority as u64);
                self.pairs.push(CandidatePair {
                    local: local.clone(),
                    remote: remote.clone(),
                    state: CheckState::Waiting,
                    priority,
                    rtt_ms: None,
                });
            }
        }

        // Sort by priority (highest first)
        self.pairs.sort_by(|a, b| b.priority.cmp(&a.priority));
    }

    /// Run connectivity checks on all pairs.
    pub fn run_checks(&mut self) {
        for (i, pair) in self.pairs.iter_mut().enumerate() {
            pair.state = CheckState::InProgress;
            self.stats.hole_punch_attempts += 1;

            // In production: send STUN binding request to pair.remote
            // and wait for response. For now, simulate:
            let can_connect = match (pair.local.ctype, pair.remote.ctype) {
                (CandidateType::Host, CandidateType::Host) => true,
                (CandidateType::ServerReflexive, CandidateType::ServerReflexive) => {
                    // Depends on NAT type
                    self.nat_type != NatType::Symmetric
                }
                (_, CandidateType::Relay) | (CandidateType::Relay, _) => true,
                _ => false,
            };

            if can_connect {
                pair.state = CheckState::Succeeded;
                pair.rtt_ms = Some(50); // Simulated
                self.stats.hole_punch_successes += 1;

                if self.selected_pair.is_none() {
                    self.selected_pair = Some(i);
                    self.method = Some(ConnMethod::HolePunch);
                }
            } else {
                pair.state = CheckState::Failed;
            }
        }

        // If no pair succeeded, fall back to relay
        if self.selected_pair.is_none() && !self.pairs.is_empty() {
            self.stats.relay_fallbacks += 1;
            self.method = Some(ConnMethod::Relayed);
        }

        self.stats.total_connections += 1;
    }

    /// Get the selected connection endpoint.
    pub fn selected_endpoint(&self) -> Option<([u8; 4], u16)> {
        self.selected_pair
            .and_then(|i| self.pairs.get(i))
            .map(|p| (p.remote.addr, p.remote.port))
    }

    /// Is direct connectivity possible?
    pub fn is_direct(&self) -> bool {
        matches!(self.method, Some(ConnMethod::Direct) | Some(ConnMethod::HolePunch))
    }
}
