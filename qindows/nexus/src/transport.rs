//! # Nexus QUIC Transport
//!
//! QUIC-based transport layer for the Global Mesh.
//! Provides reliable, encrypted, multiplexed streams over UDP.
//! All Nexus communication (data transfer, fiber migration,
//! antibody propagation) flows through QUIC.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

/// Connection ID (randomly generated, 64-bit).
pub type ConnectionId = u64;

/// Stream ID within a QUIC connection.
pub type StreamId = u64;

/// QUIC connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Handshake in progress
    Handshaking,
    /// Connection established
    Connected,
    /// Draining (graceful close)
    Draining,
    /// Closed
    Closed,
    /// Error state
    Failed,
}

/// QUIC packet types.
#[derive(Debug, Clone, Copy)]
pub enum PacketType {
    /// Initial handshake
    Initial,
    /// Handshake continuation
    Handshake,
    /// 0-RTT early data
    ZeroRtt,
    /// Regular data packet (short header)
    Short,
    /// Retry packet
    Retry,
}

/// A QUIC stream — bidirectional byte stream within a connection.
#[derive(Debug)]
pub struct QuicStream {
    /// Stream ID
    pub id: StreamId,
    /// Is this a unidirectional stream?
    pub unidirectional: bool,
    /// Send buffer
    pub send_buffer: Vec<u8>,
    /// Receive buffer
    pub recv_buffer: Vec<u8>,
    /// Bytes sent
    pub bytes_sent: u64,
    /// Bytes received
    pub bytes_received: u64,
    /// Is the send side closed?
    pub send_closed: bool,
    /// Is the receive side closed?
    pub recv_closed: bool,
    /// Flow control: max bytes the peer will accept
    pub max_stream_data: u64,
}

impl QuicStream {
    pub fn new(id: StreamId) -> Self {
        QuicStream {
            id,
            unidirectional: false,
            send_buffer: Vec::new(),
            recv_buffer: Vec::new(),
            bytes_sent: 0,
            bytes_received: 0,
            send_closed: false,
            recv_closed: false,
            max_stream_data: 1024 * 1024, // 1 MiB default
        }
    }

    /// Write data to the stream.
    pub fn write(&mut self, data: &[u8]) -> Result<usize, &'static str> {
        if self.send_closed {
            return Err("Stream send side closed");
        }

        let available = (self.max_stream_data - self.bytes_sent) as usize;
        let to_write = data.len().min(available);

        self.send_buffer.extend_from_slice(&data[..to_write]);
        self.bytes_sent += to_write as u64;

        Ok(to_write)
    }

    /// Read data from the stream.
    pub fn read(&mut self, buf: &mut [u8]) -> usize {
        let to_read = buf.len().min(self.recv_buffer.len());
        buf[..to_read].copy_from_slice(&self.recv_buffer[..to_read]);
        self.recv_buffer.drain(..to_read);
        self.bytes_received += to_read as u64;
        to_read
    }

    /// Close the send side.
    pub fn close_send(&mut self) {
        self.send_closed = true;
    }
}

/// Congestion control state.
#[derive(Debug, Clone)]
pub struct CongestionState {
    /// Congestion window (bytes)
    pub cwnd: u64,
    /// Slow start threshold
    pub ssthresh: u64,
    /// Smoothed RTT (microseconds)
    pub srtt: u64,
    /// RTT variance
    pub rttvar: u64,
    /// Bytes in flight (sent but unacked)
    pub bytes_in_flight: u64,
    /// Packets lost
    pub packets_lost: u64,
}

impl CongestionState {
    pub fn new() -> Self {
        CongestionState {
            cwnd: 14720,      // ~10 packets × 1472 bytes
            ssthresh: u64::MAX,
            srtt: 100_000,    // 100ms initial estimate
            rttvar: 50_000,
            bytes_in_flight: 0,
            packets_lost: 0,
        }
    }

    /// Update RTT estimate (RFC 6298 algorithm).
    pub fn update_rtt(&mut self, sample_rtt: u64) {
        if self.srtt == 100_000 {
            // First measurement
            self.srtt = sample_rtt;
            self.rttvar = sample_rtt / 2;
        } else {
            let diff = if sample_rtt > self.srtt {
                sample_rtt - self.srtt
            } else {
                self.srtt - sample_rtt
            };
            self.rttvar = (3 * self.rttvar + diff) / 4;
            self.srtt = (7 * self.srtt + sample_rtt) / 8;
        }
    }

    /// Handle packet acknowledgment.
    pub fn on_ack(&mut self, acked_bytes: u64) {
        self.bytes_in_flight = self.bytes_in_flight.saturating_sub(acked_bytes);

        if self.cwnd < self.ssthresh {
            // Slow start
            self.cwnd += acked_bytes;
        } else {
            // Congestion avoidance
            self.cwnd += 1472 * acked_bytes / self.cwnd;
        }
    }

    /// Handle packet loss.
    pub fn on_loss(&mut self) {
        self.packets_lost += 1;
        self.ssthresh = self.cwnd / 2;
        self.cwnd = self.ssthresh.max(14720);
    }

    /// Can we send more data?
    pub fn can_send(&self) -> bool {
        self.bytes_in_flight < self.cwnd
    }
}

/// A QUIC connection.
pub struct QuicConnection {
    /// Connection ID
    pub id: ConnectionId,
    /// Connection state
    pub state: ConnectionState,
    /// Peer address (simplified as u64)
    pub peer_addr: u64,
    /// Active streams
    pub streams: BTreeMap<StreamId, QuicStream>,
    /// Next stream ID
    next_stream_id: StreamId,
    /// Congestion control
    pub congestion: CongestionState,
    /// Total bytes sent across all streams
    pub total_sent: u64,
    /// Total bytes received
    pub total_received: u64,
    /// Is this a server-side connection?
    pub is_server: bool,
}

impl QuicConnection {
    pub fn new(id: ConnectionId, peer_addr: u64, is_server: bool) -> Self {
        QuicConnection {
            id,
            state: ConnectionState::Handshaking,
            peer_addr,
            streams: BTreeMap::new(),
            next_stream_id: if is_server { 1 } else { 0 },
            congestion: CongestionState::new(),
            total_sent: 0,
            total_received: 0,
            is_server,
        }
    }

    /// Open a new stream.
    pub fn open_stream(&mut self) -> StreamId {
        let id = self.next_stream_id;
        self.next_stream_id += 4; // QUIC stream ID spacing
        self.streams.insert(id, QuicStream::new(id));
        id
    }

    /// Get a stream by ID.
    pub fn stream(&mut self, id: StreamId) -> Option<&mut QuicStream> {
        self.streams.get_mut(&id)
    }

    /// Close the connection gracefully.
    pub fn close(&mut self) {
        self.state = ConnectionState::Draining;
        for stream in self.streams.values_mut() {
            stream.close_send();
        }
    }

    /// Get number of active streams.
    pub fn active_streams(&self) -> usize {
        self.streams.values().filter(|s| !s.send_closed || !s.recv_closed).count()
    }
}

/// QUIC endpoint — manages multiple connections.
pub struct QuicEndpoint {
    /// Active connections
    pub connections: BTreeMap<ConnectionId, QuicConnection>,
    /// Next connection ID
    next_conn_id: ConnectionId,
    /// Is this a server endpoint?
    pub is_server: bool,
    /// Total connections handled
    pub total_connections: u64,
}

impl QuicEndpoint {
    pub fn new(is_server: bool) -> Self {
        QuicEndpoint {
            connections: BTreeMap::new(),
            next_conn_id: 1,
            is_server,
            total_connections: 0,
        }
    }

    /// Accept or create a new connection.
    pub fn connect(&mut self, peer_addr: u64) -> ConnectionId {
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        self.total_connections += 1;

        self.connections.insert(id, QuicConnection::new(id, peer_addr, self.is_server));
        id
    }

    /// Get a connection by ID.
    pub fn connection(&mut self, id: ConnectionId) -> Option<&mut QuicConnection> {
        self.connections.get_mut(&id)
    }

    /// Remove closed connections.
    pub fn gc(&mut self) {
        self.connections.retain(|_, conn| conn.state != ConnectionState::Closed);
    }

    /// Get count of active connections.
    pub fn active_count(&self) -> usize {
        self.connections.values().filter(|c| c.state == ConnectionState::Connected).count()
    }
}
