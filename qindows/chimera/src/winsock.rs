//! # Chimera WinSock Emulation
//!
//! Emulates the Windows Sockets API (WS2_32) for legacy
//! Win32 networking. Maps socket calls to Nexus primitives.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Socket types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketType {
    Stream,   // SOCK_STREAM (TCP)
    Dgram,    // SOCK_DGRAM (UDP)
    Raw,      // SOCK_RAW
}

/// Address family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddrFamily {
    Inet,     // AF_INET (IPv4)
    Inet6,    // AF_INET6 (IPv6)
}

/// Socket state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketState {
    Created,
    Bound,
    Listening,
    Connected,
    Closed,
}

/// Socket address (IPv4).
#[derive(Debug, Clone, Copy)]
pub struct SockAddr {
    pub family: AddrFamily,
    pub addr: [u8; 4],
    pub port: u16,
}

impl SockAddr {
    pub fn any(port: u16) -> Self {
        SockAddr { family: AddrFamily::Inet, addr: [0, 0, 0, 0], port }
    }

    pub fn loopback(port: u16) -> Self {
        SockAddr { family: AddrFamily::Inet, addr: [127, 0, 0, 1], port }
    }
}

/// Socket options.
#[derive(Debug, Clone)]
pub struct SocketOptions {
    pub reuse_addr: bool,
    pub no_delay: bool,     // TCP_NODELAY
    pub keep_alive: bool,
    pub recv_buf_size: u32,
    pub send_buf_size: u32,
    pub linger: Option<u16>, // Linger timeout in seconds
    pub broadcast: bool,
    pub recv_timeout_ms: u32,
    pub send_timeout_ms: u32,
}

impl Default for SocketOptions {
    fn default() -> Self {
        SocketOptions {
            reuse_addr: false,
            no_delay: false,
            keep_alive: false,
            recv_buf_size: 8192,
            send_buf_size: 8192,
            linger: None,
            broadcast: false,
            recv_timeout_ms: 0,
            send_timeout_ms: 0,
        }
    }
}

/// A WinSock socket.
#[derive(Debug, Clone)]
pub struct WinSocket {
    /// Socket handle
    pub handle: u32,
    /// Socket type
    pub sock_type: SocketType,
    /// Address family
    pub family: AddrFamily,
    /// State
    pub state: SocketState,
    /// Local address
    pub local_addr: Option<SockAddr>,
    /// Remote address
    pub remote_addr: Option<SockAddr>,
    /// Options
    pub options: SocketOptions,
    /// Receive buffer
    pub recv_buf: Vec<u8>,
    /// Send buffer
    pub send_buf: Vec<u8>,
    /// Silo ID
    pub silo_id: u64,
    /// Backlog (for listening sockets)
    pub backlog: u32,
    /// Non-blocking mode
    pub non_blocking: bool,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_recv: u64,
}

/// WinSock error codes.
pub mod wsa_error {
    pub const WSAENOTSOCK: i32 = 10038;
    pub const WSAEAFNOSUPPORT: i32 = 10047;
    pub const WSAECONNREFUSED: i32 = 10061;
    pub const WSAEISCONN: i32 = 10056;
    pub const WSAENOTCONN: i32 = 10057;
    pub const WSAETIMEDOUT: i32 = 10060;
    pub const WSAEADDRINUSE: i32 = 10048;
    pub const WSAEINVAL: i32 = 10022;
    pub const WSAEWOULDBLOCK: i32 = 10035;
    pub const WSANOTINITIALISED: i32 = 10093;
}

/// WinSock statistics.
#[derive(Debug, Clone, Default)]
pub struct WsaStats {
    pub sockets_created: u64,
    pub sockets_closed: u64,
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub connections: u64,
    pub errors: u64,
}

/// The WinSock Emulator.
pub struct WinSockEmulator {
    /// Active sockets
    pub sockets: BTreeMap<u32, WinSocket>,
    /// Next socket handle
    next_handle: u32,
    /// Is WSAStartup called?
    pub initialized: bool,
    /// WSA version
    pub version: (u8, u8),
    /// Stats
    pub stats: WsaStats,
}

impl WinSockEmulator {
    pub fn new() -> Self {
        WinSockEmulator {
            sockets: BTreeMap::new(),
            next_handle: 100,
            initialized: false,
            version: (2, 2),
            stats: WsaStats::default(),
        }
    }

    /// WSAStartup
    pub fn wsa_startup(&mut self, version: (u8, u8)) -> i32 {
        self.initialized = true;
        self.version = version;
        0 // Success
    }

    /// WSACleanup
    pub fn wsa_cleanup(&mut self) -> i32 {
        self.initialized = false;
        self.sockets.clear();
        0
    }

    /// socket()
    pub fn socket(&mut self, family: AddrFamily, sock_type: SocketType, silo_id: u64) -> Result<u32, i32> {
        if !self.initialized { return Err(wsa_error::WSANOTINITIALISED); }

        let handle = self.next_handle;
        self.next_handle += 1;

        let sock = WinSocket {
            handle,
            sock_type,
            family,
            state: SocketState::Created,
            local_addr: None,
            remote_addr: None,
            options: SocketOptions::default(),
            recv_buf: Vec::new(),
            send_buf: Vec::new(),
            silo_id,
            backlog: 0,
            non_blocking: false,
            bytes_sent: 0,
            bytes_recv: 0,
        };

        self.sockets.insert(handle, sock);
        self.stats.sockets_created += 1;
        Ok(handle)
    }

    /// bind()
    pub fn bind(&mut self, handle: u32, addr: SockAddr) -> Result<(), i32> {
        // Validate state with immutable borrow first
        let sock = self.sockets.get(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        if sock.state != SocketState::Created { return Err(wsa_error::WSAEINVAL); }

        // Check for address conflicts (immutable borrow of sockets)
        for other in self.sockets.values() {
            if other.handle != handle {
                if let Some(ref la) = other.local_addr {
                    if la.port == addr.port { return Err(wsa_error::WSAEADDRINUSE); }
                }
            }
        }

        // Now take mutable borrow for mutation
        let sock = self.sockets.get_mut(&handle).unwrap();
        sock.local_addr = Some(addr);
        sock.state = SocketState::Bound;
        Ok(())
    }

    /// listen()
    pub fn listen(&mut self, handle: u32, backlog: u32) -> Result<(), i32> {
        let sock = self.sockets.get_mut(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        if sock.state != SocketState::Bound { return Err(wsa_error::WSAEINVAL); }
        if sock.sock_type != SocketType::Stream { return Err(wsa_error::WSAEINVAL); }

        sock.backlog = backlog;
        sock.state = SocketState::Listening;
        Ok(())
    }

    /// connect()
    pub fn connect(&mut self, handle: u32, addr: SockAddr) -> Result<(), i32> {
        let sock = self.sockets.get_mut(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        if sock.state == SocketState::Connected { return Err(wsa_error::WSAEISCONN); }

        sock.remote_addr = Some(addr);
        sock.state = SocketState::Connected;
        self.stats.connections += 1;
        Ok(())
    }

    /// send()
    pub fn send(&mut self, handle: u32, data: &[u8]) -> Result<usize, i32> {
        let sock = self.sockets.get_mut(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        if sock.state != SocketState::Connected { return Err(wsa_error::WSAENOTCONN); }

        let len = data.len().min(sock.options.send_buf_size as usize);
        sock.send_buf.extend_from_slice(&data[..len]);
        sock.bytes_sent += len as u64;
        self.stats.bytes_sent += len as u64;
        Ok(len)
    }

    /// recv()
    pub fn recv(&mut self, handle: u32, buf_size: usize) -> Result<Vec<u8>, i32> {
        let sock = self.sockets.get_mut(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        if sock.state != SocketState::Connected { return Err(wsa_error::WSAENOTCONN); }

        let len = buf_size.min(sock.recv_buf.len());
        let data: Vec<u8> = sock.recv_buf.drain(..len).collect();
        sock.bytes_recv += data.len() as u64;
        self.stats.bytes_recv += data.len() as u64;
        Ok(data)
    }

    /// closesocket()
    pub fn closesocket(&mut self, handle: u32) -> Result<(), i32> {
        self.sockets.remove(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        self.stats.sockets_closed += 1;
        Ok(())
    }

    /// setsockopt()
    pub fn setsockopt(&mut self, handle: u32, opt: &str, value: u32) -> Result<(), i32> {
        let sock = self.sockets.get_mut(&handle).ok_or(wsa_error::WSAENOTSOCK)?;
        match opt {
            "SO_REUSEADDR" => sock.options.reuse_addr = value != 0,
            "TCP_NODELAY" => sock.options.no_delay = value != 0,
            "SO_KEEPALIVE" => sock.options.keep_alive = value != 0,
            "SO_RCVBUF" => sock.options.recv_buf_size = value,
            "SO_SNDBUF" => sock.options.send_buf_size = value,
            "SO_BROADCAST" => sock.options.broadcast = value != 0,
            _ => return Err(wsa_error::WSAEINVAL),
        }
        Ok(())
    }
}
