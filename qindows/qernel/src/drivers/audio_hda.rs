//! # Intel HD Audio (HDA) Driver
//!
//! Drives the Intel High Definition Audio controller.
//! Found via PCI (class 0x04, subclass 0x03).
//! Provides PCM playback and capture for the Aether sound system.

use alloc::vec::Vec;

/// Audio sample format.
#[derive(Debug, Clone, Copy)]
pub enum SampleFormat {
    /// Signed 16-bit PCM (CD quality)
    S16Le,
    /// Signed 24-bit PCM (studio quality)
    S24Le,
    /// 32-bit float
    F32Le,
}

/// Audio stream parameters.
#[derive(Debug, Clone, Copy)]
pub struct AudioParams {
    /// Sample rate (e.g., 44100, 48000, 96000)
    pub sample_rate: u32,
    /// Number of channels (1=mono, 2=stereo, 6=5.1, 8=7.1)
    pub channels: u8,
    /// Sample format
    pub format: SampleFormat,
    /// Buffer size in frames
    pub buffer_frames: u32,
    /// Period size (interrupt interval in frames)
    pub period_frames: u32,
}

impl AudioParams {
    /// Bytes per sample.
    pub fn bytes_per_sample(&self) -> usize {
        match self.format {
            SampleFormat::S16Le => 2,
            SampleFormat::S24Le => 3,
            SampleFormat::F32Le => 4,
        }
    }

    /// Bytes per frame (all channels).
    pub fn bytes_per_frame(&self) -> usize {
        self.bytes_per_sample() * self.channels as usize
    }

    /// Buffer size in bytes.
    pub fn buffer_bytes(&self) -> usize {
        self.bytes_per_frame() * self.buffer_frames as usize
    }
}

impl Default for AudioParams {
    fn default() -> Self {
        AudioParams {
            sample_rate: 48000,
            channels: 2,
            format: SampleFormat::S16Le,
            buffer_frames: 4096,
            period_frames: 1024,
        }
    }
}

/// HDA Buffer Descriptor List Entry (BDLE).
#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct BdlEntry {
    /// Physical address of the buffer
    pub address: u64,
    /// Length in bytes
    pub length: u32,
    /// Interrupt on completion flag
    pub ioc: u32,
}

/// HDA stream direction.
#[derive(Debug, Clone, Copy)]
pub enum StreamDirection {
    Playback,
    Capture,
}

/// An audio stream (playback or capture).
pub struct AudioStream {
    /// Stream index
    pub index: u8,
    /// Direction
    pub direction: StreamDirection,
    /// Parameters
    pub params: AudioParams,
    /// Buffer Descriptor List
    pub bdl: Vec<BdlEntry>,
    /// Audio buffer (ring buffer)
    pub buffer: Vec<u8>,
    /// Current write position
    pub write_pos: usize,
    /// Current read position (hardware position)
    pub read_pos: usize,
    /// Is this stream running?
    pub running: bool,
    /// Total frames played
    pub frames_played: u64,
}

impl AudioStream {
    pub fn new(index: u8, direction: StreamDirection, params: AudioParams) -> Self {
        let buffer_size = params.buffer_bytes();
        AudioStream {
            index,
            direction,
            params,
            bdl: Vec::new(),
            buffer: alloc::vec![0u8; buffer_size],
            write_pos: 0,
            read_pos: 0,
            running: false,
            frames_played: 0,
        }
    }

    /// Write PCM samples to the stream buffer.
    pub fn write(&mut self, samples: &[u8]) -> usize {
        let available = if self.write_pos >= self.read_pos {
            self.buffer.len() - (self.write_pos - self.read_pos) - 1
        } else {
            self.read_pos - self.write_pos - 1
        };

        let to_write = samples.len().min(available);
        for i in 0..to_write {
            self.buffer[self.write_pos] = samples[i];
            self.write_pos = (self.write_pos + 1) % self.buffer.len();
        }

        to_write
    }

    /// Get available space in the buffer (bytes).
    pub fn available(&self) -> usize {
        if self.write_pos >= self.read_pos {
            self.buffer.len() - (self.write_pos - self.read_pos) - 1
        } else {
            self.read_pos - self.write_pos - 1
        }
    }
}

/// The HDA controller.
pub struct HdaController {
    /// MMIO base address
    pub mmio_base: u64,
    /// Playback streams
    pub playback_streams: Vec<AudioStream>,
    /// Capture streams
    pub capture_streams: Vec<AudioStream>,
    /// Master volume (0-100)
    pub master_volume: u8,
    /// Is the controller initialized?
    pub initialized: bool,
    /// Codec count
    pub codec_count: u8,
}

impl HdaController {
    /// Initialize the HDA controller.
    pub fn init(bar0: u64) -> Self {
        let mut ctrl = HdaController {
            mmio_base: bar0,
            playback_streams: Vec::new(),
            capture_streams: Vec::new(),
            master_volume: 75,
            initialized: false,
            codec_count: 0,
        };

        unsafe {
            // Read global capabilities
            let gcap = core::ptr::read_volatile(bar0 as *const u16);
            let num_output = ((gcap >> 12) & 0xF) as u8;
            let num_input = ((gcap >> 8) & 0xF) as u8;

            // Reset controller
            let gctl = (bar0 + 0x08) as *mut u32;
            core::ptr::write_volatile(gctl, 0); // Assert reset
            while core::ptr::read_volatile(gctl) & 1 != 0 {
                core::hint::spin_loop();
            }
            core::ptr::write_volatile(gctl, 1); // Deassert reset
            while core::ptr::read_volatile(gctl) & 1 == 0 {
                core::hint::spin_loop();
            }

            // Check codec presence
            let statests = (bar0 + 0x0E) as *const u16;
            let codecs = core::ptr::read_volatile(statests);
            ctrl.codec_count = codecs.count_ones() as u8;

            // Create default streams
            for i in 0..num_output.min(2) {
                ctrl.playback_streams.push(
                    AudioStream::new(i, StreamDirection::Playback, AudioParams::default())
                );
            }
            for i in 0..num_input.min(1) {
                ctrl.capture_streams.push(
                    AudioStream::new(i, StreamDirection::Capture, AudioParams::default())
                );
            }

            ctrl.initialized = true;
        }

        crate::serial_println!(
            "[OK] HDA audio: {} codecs, {} output, {} input streams",
            ctrl.codec_count,
            ctrl.playback_streams.len(),
            ctrl.capture_streams.len()
        );

        ctrl
    }

    /// Set master volume (0-100).
    pub fn set_volume(&mut self, volume: u8) {
        self.master_volume = volume.min(100);
    }

    /// Start a playback stream.
    pub fn start_playback(&mut self, stream_idx: usize) {
        if let Some(stream) = self.playback_streams.get_mut(stream_idx) {
            stream.running = true;
        }
    }

    /// Stop a playback stream.
    pub fn stop_playback(&mut self, stream_idx: usize) {
        if let Some(stream) = self.playback_streams.get_mut(stream_idx) {
            stream.running = false;
        }
    }
}
