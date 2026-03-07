//! # Synapse Neural Models
//!
//! AI inference engine for the Neural Processing Unit (NPU).
//! Provides models for:
//! - Intent classification (natural language → system command)
//! - Semantic embedding (text → vector for Prism search)
//! - Anomaly detection (for Sentinel threat analysis)
//! - Gesture recognition (from Synapse BCI data)

#![allow(dead_code)]

extern crate alloc;

use crate::math_ext::{F32Ext, F64Ext};
use alloc::vec::Vec;

/// A tensor — the fundamental data type for neural computation.
#[derive(Debug, Clone)]
pub struct Tensor {
    /// Flat data buffer
    pub data: Vec<f32>,
    /// Shape (dimensions)
    pub shape: Vec<usize>,
    /// Total number of elements
    pub size: usize,
}

impl Tensor {
    /// Create a new tensor with the given shape, filled with zeros.
    pub fn zeros(shape: &[usize]) -> Self {
        let size = shape.iter().product();
        Tensor {
            data: alloc::vec![0.0; size],
            shape: shape.to_vec(),
            size,
        }
    }

    /// Create a tensor from raw data.
    pub fn from_data(data: Vec<f32>, shape: Vec<usize>) -> Self {
        let size = shape.iter().product();
        assert_eq!(data.len(), size, "Data length must match shape");
        Tensor { data, shape, size }
    }

    /// Element-wise addition.
    pub fn add(&self, other: &Tensor) -> Tensor {
        assert_eq!(self.size, other.size);
        let data: Vec<f32> = self.data.iter().zip(&other.data).map(|(a, b)| a + b).collect();
        Tensor::from_data(data, self.shape.clone())
    }

    /// Dot product (for 1D tensors / vectors).
    pub fn dot(&self, other: &Tensor) -> f32 {
        assert_eq!(self.size, other.size);
        self.data.iter().zip(&other.data).map(|(a, b)| a * b).sum()
    }

    /// Apply ReLU activation (max(0, x)).
    pub fn relu(&self) -> Tensor {
        let data: Vec<f32> = self.data.iter().map(|&x| x.max(0.0)).collect();
        Tensor::from_data(data, self.shape.clone())
    }

    /// Apply softmax (for classification output).
    pub fn softmax(&self) -> Tensor {
        let max_val = self.data.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let exp_sum: f32 = self.data.iter().map(|&x| (x - max_val).exp()).sum();
        let data: Vec<f32> = self.data.iter().map(|&x| (x - max_val).exp() / exp_sum).collect();
        Tensor::from_data(data, self.shape.clone())
    }

    /// L2 normalize (for embedding similarity).
    pub fn normalize(&self) -> Tensor {
        let norm: f32 = self.data.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm == 0.0 {
            return self.clone();
        }
        let data: Vec<f32> = self.data.iter().map(|x| x / norm).collect();
        Tensor::from_data(data, self.shape.clone())
    }
}

/// A linear layer: y = Wx + b
#[derive(Debug, Clone)]
pub struct LinearLayer {
    /// Weight matrix (flattened: out_features × in_features)
    pub weights: Tensor,
    /// Bias vector
    pub bias: Tensor,
    pub in_features: usize,
    pub out_features: usize,
}

impl LinearLayer {
    /// Create a new linear layer with random-like initialization.
    pub fn new(in_features: usize, out_features: usize) -> Self {
        // Xavier initialization (simplified)
        let scale = 1.0 / (in_features as f32).sqrt();
        let mut weights = Vec::with_capacity(out_features * in_features);
        for i in 0..(out_features * in_features) {
            // Deterministic pseudo-random for reproducibility
            let val = ((i as f32 * 0.7127 + 0.3183).sin() * scale).abs() - scale / 2.0;
            weights.push(val);
        }

        LinearLayer {
            weights: Tensor::from_data(weights, alloc::vec![out_features, in_features]),
            bias: Tensor::zeros(&[out_features]),
            in_features,
            out_features,
        }
    }

    /// Forward pass: y = Wx + b
    pub fn forward(&self, input: &Tensor) -> Tensor {
        assert_eq!(input.size, self.in_features);
        let mut output = Vec::with_capacity(self.out_features);

        for row in 0..self.out_features {
            let start = row * self.in_features;
            let end = start + self.in_features;
            let mut sum = self.bias.data[row];
            for (i, &w) in self.weights.data[start..end].iter().enumerate() {
                sum += w * input.data[i];
            }
            output.push(sum);
        }

        Tensor::from_data(output, alloc::vec![self.out_features])
    }
}

/// Intent classification model — maps text to system actions.
pub struct IntentClassifier {
    /// Embedding layer
    pub embedding: LinearLayer,
    /// Hidden layer
    pub hidden: LinearLayer,
    /// Output layer (one per intent category)
    pub output: LinearLayer,
}

/// Recognized intents
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Open an application
    OpenApp,
    /// Search for something
    Search,
    /// File operation (open, save, copy, etc.)
    FileOp,
    /// System setting change
    SystemSetting,
    /// Navigation (switch window, desktop, etc.)
    Navigate,
    /// Communication (send message, email, etc.)
    Communicate,
    /// Media control (play, pause, volume)
    MediaControl,
    /// Unknown / not recognized
    Unknown,
}

impl IntentClassifier {
    /// Create a new intent classifier.
    pub fn new() -> Self {
        IntentClassifier {
            embedding: LinearLayer::new(128, 64),
            hidden: LinearLayer::new(64, 32),
            output: LinearLayer::new(32, 8), // 8 intent categories
        }
    }

    /// Classify a text input into an intent.
    pub fn classify(&self, input_embedding: &Tensor) -> (Intent, f32) {
        let h1 = self.embedding.forward(input_embedding).relu();
        let h2 = self.hidden.forward(&h1).relu();
        let logits = self.output.forward(&h2);
        let probs = logits.softmax();

        // Find highest probability intent
        let (idx, &confidence) = probs.data.iter().enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(core::cmp::Ordering::Equal))
            .unwrap_or((7, &0.0));

        let intent = match idx {
            0 => Intent::OpenApp,
            1 => Intent::Search,
            2 => Intent::FileOp,
            3 => Intent::SystemSetting,
            4 => Intent::Navigate,
            5 => Intent::Communicate,
            6 => Intent::MediaControl,
            _ => Intent::Unknown,
        };

        (intent, confidence)
    }
}

/// Semantic embedding model — converts text to a vector for Prism search.
pub struct SemanticEmbedder {
    pub layer1: LinearLayer,
    pub layer2: LinearLayer,
    /// Output dimension (embedding size)
    pub embed_dim: usize,
}

impl SemanticEmbedder {
    pub fn new(embed_dim: usize) -> Self {
        SemanticEmbedder {
            layer1: LinearLayer::new(128, 64),
            layer2: LinearLayer::new(64, embed_dim),
            embed_dim,
        }
    }

    /// Generate a semantic embedding vector for a text input.
    pub fn embed(&self, input: &Tensor) -> Tensor {
        let h1 = self.layer1.forward(input).relu();
        let h2 = self.layer2.forward(&h1);
        h2.normalize() // L2 normalize for cosine similarity
    }

    /// Compute cosine similarity between two embeddings.
    pub fn similarity(&self, a: &Tensor, b: &Tensor) -> f32 {
        a.dot(b) // Already L2-normalized, so dot = cosine similarity
    }
}

/// Anomaly detector — monitors Silo behavior for the Sentinel.
pub struct AnomalyDetector {
    pub encoder: LinearLayer,
    pub decoder: LinearLayer,
    /// Anomaly threshold (reconstruction error above this = anomaly)
    pub threshold: f32,
}

impl AnomalyDetector {
    pub fn new() -> Self {
        AnomalyDetector {
            encoder: LinearLayer::new(16, 4),  // Compress 16 features → 4
            decoder: LinearLayer::new(4, 16),  // Reconstruct 4 → 16
            threshold: 0.5,
        }
    }

    /// Check if a behavior vector is anomalous.
    ///
    /// Uses an autoencoder: if the reconstruction error is high,
    /// the behavior doesn't fit the learned "normal" distribution.
    pub fn is_anomalous(&self, features: &Tensor) -> (bool, f32) {
        let encoded = self.encoder.forward(features).relu();
        let decoded = self.decoder.forward(&encoded);

        // Compute reconstruction error (MSE)
        let error: f32 = features.data.iter().zip(&decoded.data)
            .map(|(a, b)| (a - b) * (a - b))
            .sum::<f32>() / features.size as f32;

        (error > self.threshold, error)
    }
}
