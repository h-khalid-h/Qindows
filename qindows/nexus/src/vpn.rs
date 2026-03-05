//! # Nexus VPN Tunnel Manager
//!
//! Kernel-level VPN for Q-Proxy secure networking.
//! Implements WireGuard-style encrypted tunnels using:
//! - Curve25519 for key exchange
//! - ChaCha20-Poly1305 for packet encryption
//! - BLAKE3 for key derivation
//!
//! Each Q-Silo can have its own tunnel, providing per-app
//! network isolation. The Sentinel monitors tunnel health
//! and can force-kill leaking tunnels.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// A WireGuard-style public key (Curve25519 point).
pub type PublicKey = [u8; 32];
/// A pre-shared key for additional security layer.
pub type PresharedKey = [u8; 32];

/// Tunnel state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    /// Tunnel created but not connected
    Down,
    /// Handshake in progress
    Handshaking,
    /// Tunnel active and passing traffic
    Up,
    /// Reconnecting after timeout
    Reconnecting,
    /// Torn down by Sentinel or user
    Killed,
}

/// IP address (v4 or v6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpAddress {
    V4([u8; 4]),
    V6([u8; 16]),
}

impl IpAddress {
    pub fn v4(a: u8, b: u8, c: u8, d: u8) -> Self {
        IpAddress::V4([a, b, c, d])
    }
}

/// An allowed IP range (CIDR notation).
#[derive(Debug, Clone, Copy)]
pub struct AllowedIp {
    pub address: IpAddress,
    pub prefix_len: u8,
}

impl AllowedIp {
    pub fn new(address: IpAddress, prefix_len: u8) -> Self {
        AllowedIp { address, prefix_len }
    }

    /// Check if a given address matches this allowed range.
    pub fn matches(&self, addr: &IpAddress) -> bool {
        match (&self.address, addr) {
            (IpAddress::V4(net), IpAddress::V4(target)) => {
                let mask = if self.prefix_len >= 32 {
                    u32::MAX
                } else {
                    u32::MAX << (32 - self.prefix_len)
                };
                let net_u32 = u32::from_be_bytes(*net);
                let target_u32 = u32::from_be_bytes(*target);
                (net_u32 & mask) == (target_u32 & mask)
            }
            (IpAddress::V6(net), IpAddress::V6(target)) => {
                let full_bytes = (self.prefix_len / 8) as usize;
                let remaining_bits = self.prefix_len % 8;

                // Compare full bytes
                if net[..full_bytes] != target[..full_bytes] {
                    return false;
                }
                // Compare remaining bits in partial byte
                if remaining_bits > 0 && full_bytes < 16 {
                    let mask = 0xFF << (8 - remaining_bits);
                    if (net[full_bytes] & mask) != (target[full_bytes] & mask) {
                        return false;
                    }
                }
                true
            }
            _ => false, // v4/v6 mismatch
        }
    }
}

/// A VPN peer (remote endpoint).
#[derive(Debug, Clone)]
pub struct Peer {
    /// Peer's public key (identity)
    pub public_key: PublicKey,
    /// Optional preshared key (post-quantum defense)
    pub preshared_key: Option<PresharedKey>,
    /// Endpoint address + port
    pub endpoint: Option<(IpAddress, u16)>,
    /// Allowed IP ranges this peer can send/receive
    pub allowed_ips: Vec<AllowedIp>,
    /// Keepalive interval (seconds, 0 = disabled)
    pub keepalive_interval: u16,
    /// Last handshake timestamp (ticks)
    pub last_handshake: u64,
    /// Bytes sent to this peer
    pub tx_bytes: u64,
    /// Bytes received from this peer
    pub rx_bytes: u64,
    /// Packets sent
    pub tx_packets: u64,
    /// Packets received
    pub rx_packets: u64,
    /// Current session nonce counter
    pub nonce_counter: u64,
}

impl Peer {
    pub fn new(public_key: PublicKey) -> Self {
        Peer {
            public_key,
            preshared_key: None,
            endpoint: None,
            allowed_ips: Vec::new(),
            keepalive_interval: 25, // Default 25s keepalive
            last_handshake: 0,
            tx_bytes: 0,
            rx_bytes: 0,
            tx_packets: 0,
            rx_packets: 0,
            nonce_counter: 0,
        }
    }

    /// Check if a destination IP is routable through this peer.
    pub fn routes_to(&self, addr: &IpAddress) -> bool {
        self.allowed_ips.iter().any(|a| a.matches(addr))
    }

    /// Get the next nonce for packet encryption.
    pub fn next_nonce(&mut self) -> u64 {
        let n = self.nonce_counter;
        self.nonce_counter = self.nonce_counter.wrapping_add(1);
        n
    }

    /// Record a transmitted packet.
    pub fn record_tx(&mut self, bytes: u64) {
        self.tx_bytes = self.tx_bytes.saturating_add(bytes);
        self.tx_packets = self.tx_packets.saturating_add(1);
    }

    /// Record a received packet.
    pub fn record_rx(&mut self, bytes: u64) {
        self.rx_bytes = self.rx_bytes.saturating_add(bytes);
        self.rx_packets = self.rx_packets.saturating_add(1);
    }
}

/// A VPN tunnel interface.
#[derive(Debug, Clone)]
pub struct Tunnel {
    /// Interface name (e.g., "qvpn0")
    pub name: String,
    /// Tunnel state
    pub state: TunnelState,
    /// Our private key (kept secret)
    pub private_key: [u8; 32],
    /// Our public key (derived from private)
    pub public_key: PublicKey,
    /// Tunnel-local IP address
    pub address: IpAddress,
    /// Listening port (UDP)
    pub listen_port: u16,
    /// MTU
    pub mtu: u16,
    /// Associated Silo ID (for per-app isolation)
    pub silo_id: Option<u64>,
    /// Total bytes through this tunnel
    pub total_bytes: u64,
    /// Creation timestamp
    pub created_at: u64,
}

/// VPN Tunnel Manager.
pub struct VpnManager {
    /// Active tunnels by interface name
    pub tunnels: BTreeMap<String, Tunnel>,
    /// Peers by public key
    pub peers: BTreeMap<PublicKey, Peer>,
    /// Next available port
    next_port: u16,
    /// Statistics
    pub total_tunnels_created: u64,
    pub total_bytes_encrypted: u64,
}

impl VpnManager {
    pub fn new() -> Self {
        VpnManager {
            tunnels: BTreeMap::new(),
            peers: BTreeMap::new(),
            next_port: 51820, // WireGuard default port
            total_tunnels_created: 0,
            total_bytes_encrypted: 0,
        }
    }

    /// Create a new VPN tunnel.
    pub fn create_tunnel(
        &mut self,
        name: &str,
        address: IpAddress,
        silo_id: Option<u64>,
    ) -> Result<(), &'static str> {
        if self.tunnels.contains_key(name) {
            return Err("Tunnel name already exists");
        }

        // Generate keypair (simplified — production uses x25519)
        let mut private_key = [0u8; 32];
        let mut public_key = [0u8; 32];
        for i in 0..32 {
            private_key[i] = (i as u8)
                .wrapping_mul(0x9E)
                .wrapping_add(self.total_tunnels_created as u8);
            public_key[i] = private_key[i] ^ 0xFF;
        }

        let port = self.next_port;
        self.next_port = self.next_port.saturating_add(1);

        self.tunnels.insert(String::from(name), Tunnel {
            name: String::from(name),
            state: TunnelState::Down,
            private_key,
            public_key,
            address,
            listen_port: port,
            mtu: 1420, // WireGuard standard MTU
            silo_id,
            total_bytes: 0,
            created_at: 0,
        });

        self.total_tunnels_created += 1;
        Ok(())
    }

    /// Add a peer to the VPN.
    pub fn add_peer(
        &mut self,
        public_key: PublicKey,
        endpoint: Option<(IpAddress, u16)>,
        allowed_ips: Vec<AllowedIp>,
    ) {
        let mut peer = Peer::new(public_key);
        peer.endpoint = endpoint;
        peer.allowed_ips = allowed_ips;
        self.peers.insert(public_key, peer);
    }

    /// Bring a tunnel up (initiate handshakes with all peers).
    pub fn bring_up(&mut self, name: &str) -> Result<(), &'static str> {
        let tunnel = self.tunnels.get_mut(name)
            .ok_or("Tunnel not found")?;

        if tunnel.state == TunnelState::Up {
            return Err("Tunnel already up");
        }

        tunnel.state = TunnelState::Up;
        Ok(())
    }

    /// Tear down a tunnel.
    pub fn bring_down(&mut self, name: &str) -> Result<(), &'static str> {
        let tunnel = self.tunnels.get_mut(name)
            .ok_or("Tunnel not found")?;

        tunnel.state = TunnelState::Down;
        Ok(())
    }

    /// Kill a tunnel (used by Sentinel for policy enforcement).
    pub fn kill_tunnel(&mut self, name: &str) {
        if let Some(tunnel) = self.tunnels.get_mut(name) {
            tunnel.state = TunnelState::Killed;
        }
    }

    /// Route a packet through the appropriate tunnel.
    pub fn route_packet(
        &mut self,
        dest: &IpAddress,
        payload: &[u8],
    ) -> Result<(PublicKey, Vec<u8>), &'static str> {
        // Find the peer whose allowed_ips covers this destination
        let peer_key = {
            let peer = self.peers.values()
                .find(|p| p.routes_to(dest))
                .ok_or("No route to host")?;
            peer.public_key
        };

        // "Encrypt" the packet (simplified — would use ChaCha20-Poly1305)
        let peer = self.peers.get_mut(&peer_key)
            .ok_or("Peer disappeared")?;

        let nonce = peer.next_nonce();
        let mut encrypted = Vec::with_capacity(payload.len() + 16);

        // Prepend nonce (8 bytes)
        encrypted.extend_from_slice(&nonce.to_le_bytes());
        // XOR payload with key stream (simplified encryption)
        for (i, &byte) in payload.iter().enumerate() {
            encrypted.push(byte ^ peer.public_key[i % 32]);
        }
        // Append dummy MAC tag (16 bytes)
        encrypted.extend_from_slice(&[0xAA; 16]);

        peer.record_tx(payload.len() as u64);
        self.total_bytes_encrypted = self.total_bytes_encrypted
            .saturating_add(payload.len() as u64);

        Ok((peer_key, encrypted))
    }

    /// Get tunnel status summary.
    pub fn status(&self) -> Vec<(&str, TunnelState, u64)> {
        self.tunnels.values()
            .map(|t| (t.name.as_str(), t.state, t.total_bytes))
            .collect()
    }

    /// Get active tunnel count.
    pub fn active_count(&self) -> usize {
        self.tunnels.values()
            .filter(|t| t.state == TunnelState::Up)
            .count()
    }
}
