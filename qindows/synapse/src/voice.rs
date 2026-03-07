//! # Synapse Voice Recognition Pipeline
//!
//! Processes audio input into text for Q-Shell and Q-Synapse.
//! The pipeline:
//! 1. **Capture**: Raw PCM audio from the audio driver
//! 2. **VAD**: Voice Activity Detection (skip silence)
//! 3. **Feature Extraction**: Mel-frequency cepstral coefficients
//! 4. **Decode**: NPU inference → text tokens
//! 5. **Intent**: Feed recognized text into the Intent Pipeline

extern crate alloc;

use alloc::string::String;
use crate::math_ext::{F32Ext, F64Ext};
use alloc::vec::Vec;

/// Audio format for voice input.
#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
    /// Sample rate (typically 16000 Hz for speech)
    pub sample_rate: u32,
    /// Bits per sample (16 or 32)
    pub bits_per_sample: u16,
    /// Number of channels (1 = mono, 2 = stereo)
    pub channels: u16,
}

impl AudioFormat {
    pub fn speech_mono() -> Self {
        AudioFormat {
            sample_rate: 16000,
            bits_per_sample: 16,
            channels: 1,
        }
    }

    /// Bytes per second for this format.
    pub fn bytes_per_second(&self) -> u32 {
        self.sample_rate * (self.bits_per_sample as u32 / 8) * self.channels as u32
    }
}

/// Voice Activity Detection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadResult {
    /// Silence detected
    Silence,
    /// Speech detected
    Speech,
    /// Uncertain (noise or partial speech)
    Uncertain,
}

/// A detected speech segment.
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    /// Start time in milliseconds
    pub start_ms: u64,
    /// End time in milliseconds
    pub end_ms: u64,
    /// Audio samples (16-bit PCM)
    pub samples: Vec<i16>,
    /// Energy level (dB)
    pub energy_db: f32,
}

/// Mel-frequency cepstral coefficients for a frame.
#[derive(Debug, Clone)]
pub struct MfccFrame {
    /// 13-dimensional MFCC coefficients
    pub coefficients: [f32; 13],
    /// Frame timestamp (ms)
    pub timestamp_ms: u64,
}

/// A recognition result.
#[derive(Debug, Clone)]
pub struct RecognitionResult {
    /// Recognized text
    pub text: String,
    /// Confidence (0.0 – 1.0)
    pub confidence: f32,
    /// Alternative hypotheses
    pub alternatives: Vec<(String, f32)>,
    /// Time range in the audio
    pub start_ms: u64,
    pub end_ms: u64,
    /// Was this a final or interim result?
    pub is_final: bool,
}

/// Voice recognition states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceState {
    /// Not listening
    Idle,
    /// Listening for wake word
    WakeWordListening,
    /// Actively transcribing speech
    Transcribing,
    /// Processing (VAD detected end of speech)
    Processing,
    /// Error
    Error,
}

/// Wake word configuration.
#[derive(Debug, Clone)]
pub struct WakeWordConfig {
    /// The wake phrase (e.g., "Hey Qindows")
    pub phrase: String,
    /// Detection sensitivity (0.0–1.0, higher = more sensitive)
    pub sensitivity: f32,
    /// Require exact match or fuzzy?
    pub fuzzy_match: bool,
}

/// Voice recognition engine.
pub struct VoiceEngine {
    /// Current state
    pub state: VoiceState,
    /// Audio format
    pub format: AudioFormat,
    /// Wake word config
    pub wake_word: WakeWordConfig,
    /// VAD energy threshold (dB)
    pub vad_threshold: f32,
    /// Minimum speech duration (ms)
    pub min_speech_ms: u64,
    /// Maximum speech duration (ms)
    pub max_speech_ms: u64,
    /// Silence timeout after speech (ms)
    pub silence_timeout_ms: u64,
    /// Current audio buffer
    audio_buffer: Vec<i16>,
    /// Detected segments pending processing
    pending_segments: Vec<SpeechSegment>,
    /// Recognition results
    pub results: Vec<RecognitionResult>,
    /// Stats
    pub total_audio_ms: u64,
    pub total_recognitions: u64,
    pub total_wake_detections: u64,
}

impl VoiceEngine {
    pub fn new() -> Self {
        VoiceEngine {
            state: VoiceState::Idle,
            format: AudioFormat::speech_mono(),
            wake_word: WakeWordConfig {
                phrase: String::from("hey qindows"),
                sensitivity: 0.7,
                fuzzy_match: true,
            },
            vad_threshold: -30.0,
            min_speech_ms: 300,
            max_speech_ms: 30_000,
            silence_timeout_ms: 1500,
            audio_buffer: Vec::new(),
            pending_segments: Vec::new(),
            results: Vec::new(),
            total_audio_ms: 0,
            total_recognitions: 0,
            total_wake_detections: 0,
        }
    }

    /// Start listening for the wake word.
    pub fn start_listening(&mut self) {
        self.state = VoiceState::WakeWordListening;
        self.audio_buffer.clear();
    }

    /// Stop listening.
    pub fn stop_listening(&mut self) {
        self.state = VoiceState::Idle;
        self.audio_buffer.clear();
        self.pending_segments.clear();
    }

    /// Feed raw audio samples into the engine.
    pub fn feed_audio(&mut self, samples: &[i16]) {
        if self.state == VoiceState::Idle {
            return;
        }

        self.audio_buffer.extend_from_slice(samples);

        let duration_ms = (samples.len() as u64 * 1000)
            / self.format.sample_rate as u64;
        self.total_audio_ms = self.total_audio_ms.saturating_add(duration_ms);

        // Run VAD on the new samples
        let energy = self.compute_energy(samples);

        match self.state {
            VoiceState::WakeWordListening => {
                if energy > self.vad_threshold {
                    // Check for wake word in accumulated buffer
                    if self.detect_wake_word() {
                        self.total_wake_detections += 1;
                        self.state = VoiceState::Transcribing;
                        self.audio_buffer.clear();
                    }
                }
            }
            VoiceState::Transcribing => {
                if energy < self.vad_threshold {
                    // Silence detected — check if we should end transcription
                    let buffer_ms = (self.audio_buffer.len() as u64 * 1000)
                        / self.format.sample_rate as u64;

                    if buffer_ms >= self.min_speech_ms {
                        self.state = VoiceState::Processing;
                        self.process_speech();
                    }
                } else {
                    // Check max duration
                    let buffer_ms = (self.audio_buffer.len() as u64 * 1000)
                        / self.format.sample_rate as u64;
                    if buffer_ms >= self.max_speech_ms {
                        self.state = VoiceState::Processing;
                        self.process_speech();
                    }
                }
            }
            _ => {}
        }
    }

    /// Compute energy (dB) of audio samples.
    fn compute_energy(&self, samples: &[i16]) -> f32 {
        if samples.is_empty() { return -100.0; }

        let sum_sq: f64 = samples.iter()
            .map(|&s| {
                let f = s as f64 / 32768.0;
                f * f
            })
            .sum();

        let rms = (sum_sq / samples.len() as f64).sqrt();
        if rms <= 0.0 { return -100.0; }
        (20.0 * rms.log10()) as f32
    }

    /// Detect wake word in the audio buffer (simplified keyword spotting).
    fn detect_wake_word(&self) -> bool {
        // In production: run a small CNN/RNN on MFCC features
        // For now: check if buffer has enough energy for speech
        let energy = self.compute_energy(&self.audio_buffer);
        energy > self.vad_threshold + 10.0
    }

    /// Process accumulated speech through the recognition pipeline.
    fn process_speech(&mut self) {
        let samples = core::mem::take(&mut self.audio_buffer);

        if samples.is_empty() {
            self.state = VoiceState::WakeWordListening;
            return;
        }

        let duration_ms = (samples.len() as u64 * 1000)
            / self.format.sample_rate as u64;

        // Step 1: Extract MFCC features
        let _features = self.extract_mfcc(&samples);

        // Step 2: In production, run NPU inference on features
        // For now: generate a placeholder result
        let result = RecognitionResult {
            text: String::from("[speech detected]"),
            confidence: 0.85,
            alternatives: Vec::new(),
            start_ms: self.total_audio_ms.saturating_sub(duration_ms),
            end_ms: self.total_audio_ms,
            is_final: true,
        };

        self.results.push(result);
        self.total_recognitions += 1;
        self.state = VoiceState::WakeWordListening;
    }

    /// Extract MFCC features from audio samples.
    fn extract_mfcc(&self, samples: &[i16]) -> Vec<MfccFrame> {
        let mut frames = Vec::new();
        let frame_size = (self.format.sample_rate as usize) / 100; // 10ms frames
        let hop_size = frame_size / 2; // 50% overlap

        let mut offset = 0;
        let mut frame_idx = 0u64;

        while offset + frame_size <= samples.len() {
            let frame_samples = &samples[offset..offset + frame_size];

            // Simplified MFCC: compute energy in 13 frequency bands
            let mut coefficients = [0.0f32; 13];
            for (band, coeff) in coefficients.iter_mut().enumerate() {
                let start = band * frame_size / 13;
                let end = ((band + 1) * frame_size / 13).min(frame_size);
                let band_energy: f64 = frame_samples[start..end].iter()
                    .map(|&s| (s as f64 / 32768.0).powi(2))
                    .sum();
                *coeff = (band_energy / (end - start) as f64).sqrt() as f32;
            }

            frames.push(MfccFrame {
                coefficients,
                timestamp_ms: frame_idx * 10,
            });

            offset += hop_size;
            frame_idx += 1;
        }

        frames
    }

    /// Get the latest recognition result.
    pub fn latest_result(&self) -> Option<&RecognitionResult> {
        self.results.last()
    }

    /// Drain all pending results.
    pub fn drain_results(&mut self) -> Vec<RecognitionResult> {
        core::mem::take(&mut self.results)
    }
}
