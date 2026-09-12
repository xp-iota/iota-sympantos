//! Embedding engine: Ollama / OpenAI-compatible API with local trigram fallback.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};

use crate::config::EmbeddingConfig;

/// Dimension of the local trigram fallback embedding.
pub const LOCAL_DIM: usize = 128;

/// Embedding engine that supports API-based or local trigram embeddings.
#[derive(Clone, Default)]
pub struct EmbeddingEngine {
    config: Option<EmbeddingConfig>,
}

impl EmbeddingEngine {
    /// Create from optional config. If config is None or has no base_url, falls back to local.
    pub fn from_config(config: Option<EmbeddingConfig>) -> Self {
        Self { config }
    }

    /// Whether this engine uses an API (Ollama / OpenAI-compatible).
    pub fn is_api(&self) -> bool {
        self.config
            .as_ref()
            .map(|c| c.base_url.is_some())
            .unwrap_or(false)
    }

    /// Compute embedding for content. Uses API if configured, else local trigram.
    pub fn embed(&self, content: &str) -> Vec<f32> {
        let canonical = canonicalize(content);
        if canonical.is_empty() {
            return Vec::new();
        }
        if self.is_api() {
            match self.embed_api(&canonical) {
                Ok(vec) => return vec,
                Err(e) => {
                    tracing::warn!("embedding API failed, using local fallback: {e}");
                }
            }
        }
        local_trigram(&canonical)
    }

    fn embed_api(&self, text: &str) -> anyhow::Result<Vec<f32>> {
        let config = self
            .config
            .as_ref()
            .context("embedding config not initialized")?
            .clone();
        let text = text.to_string();

        std::thread::spawn(move || embed_api_blocking(config, text))
            .join()
            .map_err(|_| anyhow::anyhow!("embedding API worker thread panicked"))?
    }
}

fn embed_api_blocking(config: EmbeddingConfig, text: String) -> anyhow::Result<Vec<f32>> {
    let base_url = config
        .base_url
        .as_deref()
        .context("embedding base_url not configured")?;
    let model = config.model.as_deref().unwrap_or("nomic-embed-text");

    let url = format!("{}/api/embeddings", base_url.trim_end_matches('/'));
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let mut request = client.post(&url).json(&OllamaEmbeddingRequest {
        model: model.to_string(),
        prompt: text,
    });

    if let Some(api_key) = config.api_key.as_deref().filter(|key| !key.is_empty()) {
        request = request.bearer_auth(api_key);
    }

    let response = request.send()?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().unwrap_or_default();
        anyhow::bail!("embedding API returned {status}: {body}");
    }

    let parsed: OllamaEmbeddingResponse = response.json()?;
    if parsed.embedding.is_empty() {
        anyhow::bail!("embedding API returned empty vector");
    }
    Ok(parsed.embedding)
}

/// Ollama native /api/embeddings request shape.
#[derive(Serialize)]
struct OllamaEmbeddingRequest {
    model: String,
    prompt: String,
}

/// Ollama native /api/embeddings response shape.
#[derive(Deserialize)]
struct OllamaEmbeddingResponse {
    embedding: Vec<f32>,
}

/// Local trigram hash-projection embedding (128-dim fallback).
pub fn local_trigram(canonical: &str) -> Vec<f32> {
    let mut vector = vec![0.0f32; LOCAL_DIM];
    for token in canonical.split_whitespace() {
        let chars = token.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            continue;
        }
        add_feature(&mut vector, token);
        if chars.len() > 2 {
            for window in chars.windows(3) {
                let mut gram = String::new();
                for ch in window {
                    gram.push(*ch);
                }
                add_feature(&mut vector, &gram);
            }
        }
    }
    normalize(&mut vector);
    vector
}

fn add_feature(vector: &mut [f32], feature: &str) {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    feature.hash(&mut hasher);
    let hash = hasher.finish();
    let index = (hash as usize) % LOCAL_DIM;
    let sign = if (hash & 1) == 0 { 1.0 } else { -1.0 };
    vector[index] += sign;
}

/// Normalize a vector in-place to unit length.
pub fn normalize(vector: &mut [f32]) {
    let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm <= f32::EPSILON {
        return;
    }
    for v in vector.iter_mut() {
        *v /= norm;
    }
}

/// Cosine similarity between two vectors. Handles dimension mismatch by returning 0.
pub fn cosine(left: &[f32], right: &[f32]) -> f64 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    left.iter()
        .zip(right.iter())
        .map(|(l, r)| (*l as f64) * (*r as f64))
        .sum::<f64>()
}

/// Canonicalize content: lowercase alphanumeric, collapse whitespace.
pub fn canonicalize(content: &str) -> String {
    let mut output = String::with_capacity(content.len());
    let mut in_space = false;
    for ch in content.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                output.push(lower);
            }
            in_space = false;
        } else if ch.is_whitespace() && !in_space && !output.is_empty() {
            output.push(' ');
            in_space = true;
        }
    }
    output.trim().to_string()
}

/// Pack f32 vector to little-endian bytes for BLOB storage.
pub fn to_blob(vec: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vec.len() * 4);
    for &v in vec {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    bytes
}

/// Unpack little-endian bytes to f32 vector.
pub fn from_blob(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

#[cfg(test)]
#[path = "embedding_tests.rs"]
mod embedding_tests;
