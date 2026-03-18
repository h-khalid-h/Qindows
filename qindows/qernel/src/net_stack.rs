//! # Qindows Network Stack — Ethernet Demultiplexer (Phase 24)
//!
//! Receives raw Ethernet frames from VirtIO-net `receive()` and routes them
//! by EtherType to the appropriate protocol handler.
//!
//! ## EtherType routing table
//! - 0x0806 → ARP  (address resolution)
//! - 0x0800 → IPv4 (TCP/UDP/ICMP)
//! - 0x86DD → IPv6 (reserved)
//! - other  → drop (log to serial)
//!
//! Called from kstate_ext::tick_hook via the VirtIO-net RX path.

extern crate alloc;
use alloc::vec::Vec;

/// EtherType constants
pub mod ether_type {
    pub const ARP:  u16 = 0x0806;
    pub const IPV4: u16 = 0x0800;
    pub const IPV6: u16 = 0x86DD;
    pub const VLAN: u16 = 0x8100;
}

/// A parsed Ethernet frame header.
#[derive(Debug, Clone)]
pub struct EtherFrame<'a> {
    pub dst_mac:    [u8; 6],
    pub src_mac:    [u8; 6],
    pub ether_type: u16,
    pub payload:    &'a [u8],
}

impl<'a> EtherFrame<'a> {
    /// Parse a raw Ethernet frame (minimum 14 bytes).
    pub fn parse(raw: &'a [u8]) -> Option<Self> {
        if raw.len() < 14 {
            return None;
        }
        let dst_mac = [raw[0],raw[1],raw[2],raw[3],raw[4],raw[5]];
        let src_mac = [raw[6],raw[7],raw[8],raw[9],raw[10],raw[11]];
        let ether_type = u16::from_be_bytes([raw[12], raw[13]]);
        Some(EtherFrame { dst_mac, src_mac, ether_type, payload: &raw[14..] })
    }

    /// True if this frame is addressed to us or broadcast.
    pub fn is_for_us(&self, our_mac: &[u8; 6]) -> bool {
        &self.dst_mac == our_mac || self.dst_mac == [0xFF; 6]
    }
}

/// Gap 24.3 — Ethernet demultiplexer.
///
/// Routes received frames by EtherType. Returns accepted frame count.
/// `reply_buf`: if ARP generates a reply frame, caller sends it via `send_fn`.
pub fn demux(frames: &[Vec<u8>], our_mac: &[u8; 6]) -> (usize, Option<[u8; 42]>) {
    let mut accepted = 0;
    let mut arp_reply: Option<[u8; 42]> = None;

    for raw in frames {
        let frame = match EtherFrame::parse(raw) {
            Some(f) => f,
            None => {
                crate::serial_println!("[NET] Dropped: frame too short ({} bytes)", raw.len());
                continue;
            }
        };

        if !frame.is_for_us(our_mac) { continue; }

        accepted += 1;

        match frame.ether_type {
            ether_type::ARP => {
                // Logic fix 3 — handle_arp returns an optional reply frame;
                // caller (tick_hook) sends it while already holding the net lock.
                if let Some(reply) = handle_arp(frame.payload, our_mac) {
                    arp_reply = Some(reply);
                }
            }
            ether_type::IPV4 => { handle_ipv4(frame.payload); }
            ether_type::IPV6 => {
                crate::serial_println!("[NET] IPv6 frame ({} bytes) — reserved", frame.payload.len());
            }
            ether_type::VLAN => {
                crate::serial_println!("[NET] VLAN-tagged frame — stripped");
            }
            other => {
                crate::serial_println!("[NET] Unknown EtherType 0x{:04X} — dropped", other);
            }
        }
    }

    (accepted, arp_reply)
}

/// Handle an ARP packet (EtherType 0x0806).
/// Logic fix 3 — returns Option<[u8;42]> reply frame instead of calling virtio_net directly.
/// If this node is the ARP target (10.0.2.15), builds a 42-byte ARP reply and returns it;
/// the caller (demux → tick_hook) sends it while holding the existing net lock.
fn handle_arp(payload: &[u8], our_mac: &[u8; 6]) -> Option<[u8; 42]> {
    if payload.len() < 28 { return None; }
    let oper = u16::from_be_bytes([payload[6], payload[7]]);
    let sender_mac = &payload[8..14];
    let sender_ip  = &payload[14..18];
    let target_ip  = &payload[24..28];
    const OUR_IP: [u8; 4] = [10, 0, 2, 15];

    match oper {
        1 => {
            crate::serial_println!(
                "[ARP] WHO HAS {}.{}.{}.{}? TELL {}.{}.{}.{}",
                target_ip[0], target_ip[1], target_ip[2], target_ip[3],
                sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3],
            );
            if target_ip != &OUR_IP { return None; }
            // Build 42-byte ARP reply (14 Eth header + 28 ARP payload)
            let mut f = [0u8; 42];
            f[0..6].copy_from_slice(sender_mac);            // dst = requester
            f[6..12].copy_from_slice(our_mac);              // src = us
            f[12..14].copy_from_slice(&[0x08, 0x06]);       // EtherType = ARP
            f[14..16].copy_from_slice(&[0, 1]);             // HTYPE = Ethernet
            f[16..18].copy_from_slice(&[0x08, 0x00]);       // PTYPE = IPv4
            f[18] = 6; f[19] = 4;                           // HLEN=6 PLEN=4
            f[20..22].copy_from_slice(&[0, 2]);             // OPER = reply
            f[22..28].copy_from_slice(our_mac);             // sender MAC = us
            f[28..32].copy_from_slice(&OUR_IP);             // sender IP = us
            f[32..38].copy_from_slice(sender_mac);          // target MAC = requester
            f[38..42].copy_from_slice(sender_ip);           // target IP = requester
            crate::serial_println!(
                "[ARP] Sending reply: {}.{}.{}.{} is at {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                OUR_IP[0], OUR_IP[1], OUR_IP[2], OUR_IP[3],
                our_mac[0], our_mac[1], our_mac[2], our_mac[3], our_mac[4], our_mac[5],
            );
            Some(f)
        }
        2 => {
            crate::serial_println!(
                "[ARP] REPLY: {}.{}.{}.{} IS AT {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}",
                sender_ip[0], sender_ip[1], sender_ip[2], sender_ip[3],
                sender_mac[0], sender_mac[1], sender_mac[2], sender_mac[3], sender_mac[4], sender_mac[5],
            );
            None
        }
        _ => { crate::serial_println!("[ARP] Unknown oper: {}", oper); None }
    }
}

/// Handle an IPv4 packet (EtherType 0x0800).
fn handle_ipv4(payload: &[u8]) {
    if payload.len() < 20 {
        crate::serial_println!("[IPv4] Too short: {} bytes", payload.len());
        return;
    }
    let protocol = payload[9];
    let src = &payload[12..16];
    let dst = &payload[16..20];
    match protocol {
        1  => crate::serial_println!("[ICMP] {}.{}.{}.{} → {}.{}.{}.{}", src[0],src[1],src[2],src[3], dst[0],dst[1],dst[2],dst[3]),
        6  => crate::serial_println!("[TCP]  {}.{}.{}.{} → {}.{}.{}.{} len={}", src[0],src[1],src[2],src[3], dst[0],dst[1],dst[2],dst[3], payload.len()),
        17 => crate::serial_println!("[UDP]  {}.{}.{}.{} → {}.{}.{}.{} len={}", src[0],src[1],src[2],src[3], dst[0],dst[1],dst[2],dst[3], payload.len()),
        _  => crate::serial_println!("[IPv4] proto={} src={}.{}.{}.{}", protocol, src[0],src[1],src[2],src[3]),
    }
}
