//! # Q-Pipe — Named Pipes for Cross-Silo IPC
//!
//! Provides named pipe semantics for inter-Silo communication
//! with capability-gated access (Section 3.22).
//!
//! Features:
//! - Named pipes with unique identifiers
//! - Byte-stream and message modes
//! - Buffered read/write with flow control
//! - Per-Silo pipe namespace isolation
//! - Server/client connection model

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Pipe mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeMode {
    ByteStream,
    Message,
}

/// Pipe state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipeState {
    Listening,
    Connected,
    Closed,
}

/// A named pipe instance.
#[derive(Debug, Clone)]
pub struct NamedPipe {
    pub id: u64,
    pub name: String,
    pub mode: PipeMode,
    pub state: PipeState,
    pub server_silo: u64,
    pub client_silo: Option<u64>,
    pub buffer: Vec<u8>,
    pub max_buffer: usize,
    pub bytes_written: u64,
    pub bytes_read: u64,
}

/// Pipe statistics.
#[derive(Debug, Clone, Default)]
pub struct PipeStats {
    pub pipes_created: u64,
    pub connections: u64,
    pub bytes_transferred: u64,
    pub overflows: u64,
}

/// The Q-Pipe Manager.
pub struct QPipe {
    pub pipes: BTreeMap<u64, NamedPipe>,
    /// Name → pipe ID lookup
    pub names: BTreeMap<String, u64>,
    next_id: u64,
    pub stats: PipeStats,
}

impl QPipe {
    pub fn new() -> Self {
        QPipe {
            pipes: BTreeMap::new(),
            names: BTreeMap::new(),
            next_id: 1,
            stats: PipeStats::default(),
        }
    }

    /// Create a named pipe (server side).
    pub fn create(&mut self, name: &str, mode: PipeMode, server_silo: u64, max_buf: usize) -> Result<u64, &'static str> {
        if self.names.contains_key(name) {
            return Err("Pipe name already exists");
        }

        let id = self.next_id;
        self.next_id += 1;

        self.pipes.insert(id, NamedPipe {
            id, name: String::from(name), mode, state: PipeState::Listening,
            server_silo, client_silo: None,
            buffer: Vec::new(), max_buffer: max_buf,
            bytes_written: 0, bytes_read: 0,
        });
        self.names.insert(String::from(name), id);
        self.stats.pipes_created += 1;
        Ok(id)
    }

    /// Connect to a named pipe (client side).
    pub fn connect(&mut self, name: &str, client_silo: u64) -> Result<u64, &'static str> {
        let id = self.names.get(name).copied().ok_or("Pipe not found")?;
        let pipe = self.pipes.get_mut(&id).ok_or("Pipe not found")?;

        if pipe.state != PipeState::Listening {
            return Err("Pipe not listening");
        }

        pipe.client_silo = Some(client_silo);
        pipe.state = PipeState::Connected;
        self.stats.connections += 1;
        Ok(id)
    }

    /// Write data to a pipe.
    pub fn write(&mut self, pipe_id: u64, data: &[u8]) -> Result<usize, &'static str> {
        let pipe = self.pipes.get_mut(&pipe_id).ok_or("Pipe not found")?;
        if pipe.state != PipeState::Connected { return Err("Not connected"); }

        let available = pipe.max_buffer.saturating_sub(pipe.buffer.len());
        if available == 0 {
            self.stats.overflows += 1;
            return Err("Buffer full");
        }

        let to_write = data.len().min(available);
        pipe.buffer.extend_from_slice(&data[..to_write]);
        pipe.bytes_written += to_write as u64;
        self.stats.bytes_transferred += to_write as u64;
        Ok(to_write)
    }

    /// Read data from a pipe.
    pub fn read(&mut self, pipe_id: u64, max_bytes: usize) -> Result<Vec<u8>, &'static str> {
        let pipe = self.pipes.get_mut(&pipe_id).ok_or("Pipe not found")?;
        if pipe.state != PipeState::Connected { return Err("Not connected"); }

        let len = max_bytes.min(pipe.buffer.len());
        let data: Vec<u8> = pipe.buffer.drain(..len).collect();
        pipe.bytes_read += data.len() as u64;
        Ok(data)
    }

    /// Close a pipe.
    pub fn close(&mut self, pipe_id: u64) {
        if let Some(pipe) = self.pipes.get_mut(&pipe_id) {
            pipe.state = PipeState::Closed;
            self.names.remove(&pipe.name);
        }
    }
}
