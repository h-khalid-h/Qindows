//! # Nexus WebSocket Handler
//!
//! WebSocket frame parser and builder for the Nexus networking
//! stack. Implements RFC 6455 framing, masking, and control
//! frame handling (ping/pong/close).

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

/// WebSocket opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WsOpcode {
    Continuation = 0x0,
    Text = 0x1,
    Binary = 0x2,
    Close = 0x8,
    Ping = 0x9,
    Pong = 0xA,
}

impl WsOpcode {
    pub fn from_u8(v: u8) -> Option<Self> {
        match v {
            0x0 => Some(WsOpcode::Continuation),
            0x1 => Some(WsOpcode::Text),
            0x2 => Some(WsOpcode::Binary),
            0x8 => Some(WsOpcode::Close),
            0x9 => Some(WsOpcode::Ping),
            0xA => Some(WsOpcode::Pong),
            _ => None,
        }
    }

    pub fn is_control(&self) -> bool {
        matches!(self, WsOpcode::Close | WsOpcode::Ping | WsOpcode::Pong)
    }
}

/// WebSocket close status code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseCode {
    Normal = 1000,
    GoingAway = 1001,
    ProtocolError = 1002,
    UnsupportedData = 1003,
    InvalidPayload = 1007,
    PolicyViolation = 1008,
    MessageTooBig = 1009,
    InternalError = 1011,
}

impl CloseCode {
    pub fn from_u16(v: u16) -> Self {
        match v {
            1000 => CloseCode::Normal,
            1001 => CloseCode::GoingAway,
            1002 => CloseCode::ProtocolError,
            1003 => CloseCode::UnsupportedData,
            1007 => CloseCode::InvalidPayload,
            1008 => CloseCode::PolicyViolation,
            1009 => CloseCode::MessageTooBig,
            _ => CloseCode::InternalError,
        }
    }
}

/// A WebSocket frame.
#[derive(Debug, Clone)]
pub struct WsFrame {
    /// Is this the final fragment?
    pub fin: bool,
    /// Frame opcode
    pub opcode: WsOpcode,
    /// Is the payload masked?
    pub masked: bool,
    /// Masking key (if masked)
    pub mask_key: [u8; 4],
    /// Payload data (unmasked)
    pub payload: Vec<u8>,
}

impl WsFrame {
    /// Create a text frame.
    pub fn text(data: &str) -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Text,
            masked: false,
            mask_key: [0; 4],
            payload: data.as_bytes().to_vec(),
        }
    }

    /// Create a binary frame.
    pub fn binary(data: &[u8]) -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Binary,
            masked: false,
            mask_key: [0; 4],
            payload: data.to_vec(),
        }
    }

    /// Create a ping frame.
    pub fn ping(data: &[u8]) -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Ping,
            masked: false,
            mask_key: [0; 4],
            payload: data.to_vec(),
        }
    }

    /// Create a pong frame (response to ping).
    pub fn pong(data: &[u8]) -> Self {
        WsFrame {
            fin: true,
            opcode: WsOpcode::Pong,
            masked: false,
            mask_key: [0; 4],
            payload: data.to_vec(),
        }
    }

    /// Create a close frame.
    pub fn close(code: CloseCode, reason: &str) -> Self {
        let mut payload = Vec::new();
        let code_val = code as u16;
        payload.push((code_val >> 8) as u8);
        payload.push((code_val & 0xFF) as u8);
        payload.extend_from_slice(reason.as_bytes());

        WsFrame {
            fin: true,
            opcode: WsOpcode::Close,
            masked: false,
            mask_key: [0; 4],
            payload,
        }
    }

    /// Serialize the frame to bytes.
    pub fn serialize(&self) -> Vec<u8> {
        let mut buf = Vec::new();

        // First byte: FIN + opcode
        let byte0 = if self.fin { 0x80 } else { 0x00 } | (self.opcode as u8);
        buf.push(byte0);

        // Second byte: MASK + payload length
        let payload_len = self.payload.len();
        let mask_bit = if self.masked { 0x80 } else { 0x00 };

        if payload_len < 126 {
            buf.push(mask_bit | payload_len as u8);
        } else if payload_len <= 0xFFFF {
            buf.push(mask_bit | 126);
            buf.push((payload_len >> 8) as u8);
            buf.push((payload_len & 0xFF) as u8);
        } else {
            buf.push(mask_bit | 127);
            for i in (0..8).rev() {
                buf.push((payload_len >> (i * 8)) as u8);
            }
        }

        // Masking key
        if self.masked {
            buf.extend_from_slice(&self.mask_key);
        }

        // Payload (apply mask if needed)
        if self.masked {
            for (i, &byte) in self.payload.iter().enumerate() {
                buf.push(byte ^ self.mask_key[i % 4]);
            }
        } else {
            buf.extend_from_slice(&self.payload);
        }

        buf
    }

    /// Get payload as UTF-8 text (for text frames).
    pub fn text_payload(&self) -> Option<&str> {
        if self.opcode == WsOpcode::Text {
            core::str::from_utf8(&self.payload).ok()
        } else { None }
    }
}

/// Parse error.
#[derive(Debug, Clone)]
pub enum WsParseError {
    Incomplete,
    InvalidOpcode,
    ControlFrameTooLarge,
    FragmentedControl,
}

/// Parse a single WebSocket frame from raw bytes.
pub fn parse_frame(data: &[u8]) -> Result<(WsFrame, usize), WsParseError> {
    if data.len() < 2 { return Err(WsParseError::Incomplete); }

    let fin = data[0] & 0x80 != 0;
    let opcode = WsOpcode::from_u8(data[0] & 0x0F)
        .ok_or(WsParseError::InvalidOpcode)?;

    let masked = data[1] & 0x80 != 0;
    let len_byte = data[1] & 0x7F;

    let (payload_len, mut offset) = if len_byte < 126 {
        (len_byte as usize, 2)
    } else if len_byte == 126 {
        if data.len() < 4 { return Err(WsParseError::Incomplete); }
        let len = ((data[2] as usize) << 8) | data[3] as usize;
        (len, 4)
    } else {
        if data.len() < 10 { return Err(WsParseError::Incomplete); }
        let mut len = 0usize;
        for i in 0..8 {
            len = (len << 8) | data[2 + i] as usize;
        }
        (len, 10)
    };

    // Control frames can't be >125 bytes
    if opcode.is_control() && payload_len > 125 {
        return Err(WsParseError::ControlFrameTooLarge);
    }
    if opcode.is_control() && !fin {
        return Err(WsParseError::FragmentedControl);
    }

    let mut mask_key = [0u8; 4];
    if masked {
        if data.len() < offset + 4 { return Err(WsParseError::Incomplete); }
        mask_key.copy_from_slice(&data[offset..offset + 4]);
        offset += 4;
    }

    if data.len() < offset + payload_len { return Err(WsParseError::Incomplete); }

    let mut payload = data[offset..offset + payload_len].to_vec();
    if masked {
        for (i, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask_key[i % 4];
        }
    }

    let total_consumed = offset + payload_len;

    Ok((WsFrame {
        fin, opcode, masked, mask_key, payload,
    }, total_consumed))
}

/// WebSocket statistics.
#[derive(Debug, Clone, Default)]
pub struct WsStats {
    pub frames_sent: u64,
    pub frames_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub pings_sent: u64,
    pub pongs_received: u64,
}
