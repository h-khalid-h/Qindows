//! # Nexus DHCP Client
//!
//! DHCP (RFC 2131) client for automatic IPv4 configuration.
//! Implements DISCOVER → OFFER → REQUEST → ACK state machine
//! with lease tracking and renewal timers.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// DHCP message types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpMessageType {
    Discover  = 1,
    Offer     = 2,
    Request   = 3,
    Decline   = 4,
    Ack       = 5,
    Nak       = 6,
    Release   = 7,
    Inform    = 8,
}

impl DhcpMessageType {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(DhcpMessageType::Discover),
            2 => Some(DhcpMessageType::Offer),
            3 => Some(DhcpMessageType::Request),
            4 => Some(DhcpMessageType::Decline),
            5 => Some(DhcpMessageType::Ack),
            6 => Some(DhcpMessageType::Nak),
            7 => Some(DhcpMessageType::Release),
            8 => Some(DhcpMessageType::Inform),
            _ => None,
        }
    }
}

/// DHCP client state machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DhcpState {
    /// Initial state
    Init,
    /// DISCOVER sent, waiting for OFFER
    Selecting,
    /// REQUEST sent, waiting for ACK
    Requesting,
    /// Lease acquired
    Bound,
    /// Lease renewal (T1 expired, unicast REQUEST)
    Renewing,
    /// Lease rebind (T2 expired, broadcast REQUEST)
    Rebinding,
}

/// IPv4 address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ipv4Addr(pub [u8; 4]);

impl Ipv4Addr {
    pub const ZERO: Ipv4Addr = Ipv4Addr([0, 0, 0, 0]);
    pub const BROADCAST: Ipv4Addr = Ipv4Addr([255, 255, 255, 255]);

    pub fn new(a: u8, b: u8, c: u8, d: u8) -> Self {
        Ipv4Addr([a, b, c, d])
    }

    pub fn is_zero(&self) -> bool {
        self.0 == [0, 0, 0, 0]
    }

    pub fn to_u32(&self) -> u32 {
        u32::from_be_bytes(self.0)
    }
}

/// DHCP option codes.
pub mod options {
    pub const SUBNET_MASK: u8    = 1;
    pub const ROUTER: u8         = 3;
    pub const DNS_SERVER: u8     = 6;
    pub const HOSTNAME: u8       = 12;
    pub const DOMAIN_NAME: u8    = 15;
    pub const BROADCAST_ADDR: u8 = 28;
    pub const REQUESTED_IP: u8   = 50;
    pub const LEASE_TIME: u8     = 51;
    pub const MSG_TYPE: u8       = 53;
    pub const SERVER_ID: u8      = 54;
    pub const RENEWAL_T1: u8     = 58;
    pub const REBIND_T2: u8      = 59;
    pub const CLIENT_ID: u8      = 61;
    pub const END: u8            = 255;
    pub const PAD: u8            = 0;
}

/// A DHCP option (type-length-value).
#[derive(Debug, Clone)]
pub struct DhcpOption {
    pub code: u8,
    pub data: Vec<u8>,
}

impl DhcpOption {
    pub fn new_u8(code: u8, val: u8) -> Self {
        DhcpOption { code, data: alloc::vec![val] }
    }

    pub fn new_ip(code: u8, addr: Ipv4Addr) -> Self {
        DhcpOption { code, data: addr.0.to_vec() }
    }

    pub fn new_u32(code: u8, val: u32) -> Self {
        DhcpOption { code, data: val.to_be_bytes().to_vec() }
    }

    pub fn as_ip(&self) -> Option<Ipv4Addr> {
        if self.data.len() >= 4 {
            Some(Ipv4Addr([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else { None }
    }

    pub fn as_u32(&self) -> Option<u32> {
        if self.data.len() >= 4 {
            Some(u32::from_be_bytes([self.data[0], self.data[1], self.data[2], self.data[3]]))
        } else { None }
    }
}

/// A DHCP message (simplified — no full 576-byte packet).
#[derive(Debug, Clone)]
pub struct DhcpMessage {
    /// Op: 1=BOOTREQUEST, 2=BOOTREPLY
    pub op: u8,
    /// Transaction ID
    pub xid: u32,
    /// Seconds elapsed
    pub secs: u16,
    /// Flags (0x8000 = broadcast)
    pub flags: u16,
    /// Client IP (if bound)
    pub ciaddr: Ipv4Addr,
    /// 'Your' IP (offered by server)
    pub yiaddr: Ipv4Addr,
    /// Server IP
    pub siaddr: Ipv4Addr,
    /// Gateway IP
    pub giaddr: Ipv4Addr,
    /// Client hardware address
    pub chaddr: [u8; 6],
    /// Options
    pub options: Vec<DhcpOption>,
}

impl DhcpMessage {
    /// Get an option by code.
    pub fn get_option(&self, code: u8) -> Option<&DhcpOption> {
        self.options.iter().find(|o| o.code == code)
    }

    /// Get the message type.
    pub fn msg_type(&self) -> Option<DhcpMessageType> {
        self.get_option(options::MSG_TYPE)
            .and_then(|o| o.data.first().copied())
            .and_then(DhcpMessageType::from_u8)
    }
}

/// DHCP lease information.
#[derive(Debug, Clone)]
pub struct DhcpLease {
    pub ip: Ipv4Addr,
    pub subnet_mask: Ipv4Addr,
    pub gateway: Ipv4Addr,
    pub dns_servers: Vec<Ipv4Addr>,
    pub server_id: Ipv4Addr,
    pub lease_time_secs: u32,
    pub t1_renewal_secs: u32,
    pub t2_rebind_secs: u32,
    pub domain: Option<String>,
    pub hostname: Option<String>,
    /// Timestamp when lease was acquired (kernel ticks)
    pub acquired_at: u64,
}

/// DHCP client statistics.
#[derive(Debug, Clone, Default)]
pub struct DhcpStats {
    pub discovers_sent: u64,
    pub offers_received: u64,
    pub requests_sent: u64,
    pub acks_received: u64,
    pub naks_received: u64,
    pub renewals: u64,
    pub rebinds: u64,
}

/// The DHCP Client.
pub struct DhcpClient {
    /// Current state
    pub state: DhcpState,
    /// Client MAC address
    pub mac: [u8; 6],
    /// Current lease
    pub lease: Option<DhcpLease>,
    /// Transaction ID for current exchange
    pub xid: u32,
    /// Simple XID counter for generating transaction IDs
    xid_counter: u32,
    /// Stats
    pub stats: DhcpStats,
}

impl DhcpClient {
    pub fn new(mac: [u8; 6]) -> Self {
        // Generate initial XID from MAC
        let seed = u32::from_be_bytes([mac[2], mac[3], mac[4], mac[5]]);
        DhcpClient {
            state: DhcpState::Init,
            mac,
            lease: None,
            xid: seed,
            xid_counter: seed,
            stats: DhcpStats::default(),
        }
    }

    /// Generate a DHCPDISCOVER message.
    pub fn discover(&mut self) -> DhcpMessage {
        self.xid_counter = self.xid_counter.wrapping_add(1);
        self.xid = self.xid_counter;
        self.state = DhcpState::Selecting;
        self.stats.discovers_sent += 1;

        DhcpMessage {
            op: 1, // BOOTREQUEST
            xid: self.xid,
            secs: 0,
            flags: 0x8000, // Broadcast
            ciaddr: Ipv4Addr::ZERO,
            yiaddr: Ipv4Addr::ZERO,
            siaddr: Ipv4Addr::ZERO,
            giaddr: Ipv4Addr::ZERO,
            chaddr: self.mac,
            options: alloc::vec![
                DhcpOption::new_u8(options::MSG_TYPE, DhcpMessageType::Discover as u8),
            ],
        }
    }

    /// Process a DHCPOFFER and generate a DHCPREQUEST.
    pub fn handle_offer(&mut self, offer: &DhcpMessage) -> Option<DhcpMessage> {
        if self.state != DhcpState::Selecting { return None; }
        if offer.xid != self.xid { return None; }
        if offer.msg_type() != Some(DhcpMessageType::Offer) { return None; }

        self.stats.offers_received += 1;

        let offered_ip = offer.yiaddr;
        let server_id = offer.get_option(options::SERVER_ID)
            .and_then(|o| o.as_ip())
            .unwrap_or(offer.siaddr);

        self.state = DhcpState::Requesting;
        self.stats.requests_sent += 1;

        Some(DhcpMessage {
            op: 1,
            xid: self.xid,
            secs: 0,
            flags: 0x8000,
            ciaddr: Ipv4Addr::ZERO,
            yiaddr: Ipv4Addr::ZERO,
            siaddr: Ipv4Addr::ZERO,
            giaddr: Ipv4Addr::ZERO,
            chaddr: self.mac,
            options: alloc::vec![
                DhcpOption::new_u8(options::MSG_TYPE, DhcpMessageType::Request as u8),
                DhcpOption::new_ip(options::REQUESTED_IP, offered_ip),
                DhcpOption::new_ip(options::SERVER_ID, server_id),
            ],
        })
    }

    /// Process a DHCPACK and store the lease.
    pub fn handle_ack(&mut self, ack: &DhcpMessage, now: u64) -> bool {
        if self.state != DhcpState::Requesting
            && self.state != DhcpState::Renewing
            && self.state != DhcpState::Rebinding { return false; }
        if ack.xid != self.xid { return false; }
        if ack.msg_type() != Some(DhcpMessageType::Ack) { return false; }

        self.stats.acks_received += 1;

        let subnet = ack.get_option(options::SUBNET_MASK)
            .and_then(|o| o.as_ip())
            .unwrap_or(Ipv4Addr::new(255, 255, 255, 0));

        let gateway = ack.get_option(options::ROUTER)
            .and_then(|o| o.as_ip())
            .unwrap_or(Ipv4Addr::ZERO);

        let mut dns_servers = Vec::new();
        if let Some(dns_opt) = ack.get_option(options::DNS_SERVER) {
            let mut i = 0;
            while i + 4 <= dns_opt.data.len() {
                dns_servers.push(Ipv4Addr([
                    dns_opt.data[i], dns_opt.data[i+1],
                    dns_opt.data[i+2], dns_opt.data[i+3],
                ]));
                i += 4;
            }
        }

        let server_id = ack.get_option(options::SERVER_ID)
            .and_then(|o| o.as_ip())
            .unwrap_or(ack.siaddr);

        let lease_time = ack.get_option(options::LEASE_TIME)
            .and_then(|o| o.as_u32())
            .unwrap_or(86400); // Default 24h

        let t1 = ack.get_option(options::RENEWAL_T1)
            .and_then(|o| o.as_u32())
            .unwrap_or(lease_time / 2);

        let t2 = ack.get_option(options::REBIND_T2)
            .and_then(|o| o.as_u32())
            .unwrap_or(lease_time * 7 / 8);

        let domain = ack.get_option(options::DOMAIN_NAME)
            .and_then(|o| core::str::from_utf8(&o.data).ok())
            .map(String::from);

        let hostname = ack.get_option(options::HOSTNAME)
            .and_then(|o| core::str::from_utf8(&o.data).ok())
            .map(String::from);

        self.lease = Some(DhcpLease {
            ip: ack.yiaddr,
            subnet_mask: subnet,
            gateway,
            dns_servers,
            server_id,
            lease_time_secs: lease_time,
            t1_renewal_secs: t1,
            t2_rebind_secs: t2,
            domain,
            hostname,
            acquired_at: now,
        });

        self.state = DhcpState::Bound;
        true
    }

    /// Handle a DHCPNAK.
    pub fn handle_nak(&mut self, nak: &DhcpMessage) {
        if nak.xid != self.xid { return; }
        if nak.msg_type() != Some(DhcpMessageType::Nak) { return; }

        self.stats.naks_received += 1;
        self.lease = None;
        self.state = DhcpState::Init;
    }

    /// Check if the lease needs renewal (called periodically).
    pub fn check_timers(&mut self, now: u64) -> Option<DhcpMessage> {
        let lease = self.lease.as_ref()?;
        let elapsed = now.saturating_sub(lease.acquired_at);

        if self.state == DhcpState::Bound && elapsed >= lease.t1_renewal_secs as u64 {
            self.state = DhcpState::Renewing;
            self.stats.renewals += 1;
            return Some(self.build_renew_request());
        }

        if self.state == DhcpState::Renewing && elapsed >= lease.t2_rebind_secs as u64 {
            self.state = DhcpState::Rebinding;
            self.stats.rebinds += 1;
            return Some(self.build_renew_request());
        }

        if elapsed >= lease.lease_time_secs as u64 {
            // Lease expired
            self.lease = None;
            self.state = DhcpState::Init;
        }

        None
    }

    /// Build a renewal REQUEST using the current lease IP.
    fn build_renew_request(&mut self) -> DhcpMessage {
        self.xid_counter = self.xid_counter.wrapping_add(1);
        self.xid = self.xid_counter;
        self.stats.requests_sent += 1;

        let ciaddr = self.lease.as_ref()
            .map(|l| l.ip)
            .unwrap_or(Ipv4Addr::ZERO);

        DhcpMessage {
            op: 1,
            xid: self.xid,
            secs: 0,
            flags: if self.state == DhcpState::Rebinding { 0x8000 } else { 0 },
            ciaddr,
            yiaddr: Ipv4Addr::ZERO,
            siaddr: Ipv4Addr::ZERO,
            giaddr: Ipv4Addr::ZERO,
            chaddr: self.mac,
            options: alloc::vec![
                DhcpOption::new_u8(options::MSG_TYPE, DhcpMessageType::Request as u8),
            ],
        }
    }

    /// Generate a DHCPRELEASE message.
    pub fn release(&mut self) -> Option<DhcpMessage> {
        let lease = self.lease.take()?;
        self.state = DhcpState::Init;

        Some(DhcpMessage {
            op: 1,
            xid: self.xid,
            secs: 0,
            flags: 0,
            ciaddr: lease.ip,
            yiaddr: Ipv4Addr::ZERO,
            siaddr: lease.server_id,
            giaddr: Ipv4Addr::ZERO,
            chaddr: self.mac,
            options: alloc::vec![
                DhcpOption::new_u8(options::MSG_TYPE, DhcpMessageType::Release as u8),
                DhcpOption::new_ip(options::SERVER_ID, lease.server_id),
            ],
        })
    }

    /// Is the network configured?
    pub fn is_configured(&self) -> bool {
        self.state == DhcpState::Bound && self.lease.is_some()
    }

    /// Get current IP address.
    pub fn ip(&self) -> Option<Ipv4Addr> {
        self.lease.as_ref().map(|l| l.ip)
    }
}
