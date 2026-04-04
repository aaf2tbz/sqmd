use ort::session::Session;
use ort::value::Value;
use std::path::PathBuf;

const DIMS: usize = 768;
const MODEL_NAME: &str = "nomic-embed-text-v1.5";
const MAX_SEQ_LEN: usize = 512;

pub struct Embedder {
    session: Option<Session>,
    model_name: String,
}

impl Embedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            session: None,
            model_name: MODEL_NAME.to_string(),
        })
    }

    pub fn ensure_loaded(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.session.is_some() {
            return Ok(());
        }

        let home = std::env::var("HOME").map_err(|_| "HOME env var not set")?;
        let path = PathBuf::from(home)
            .join(".sqmd")
            .join("models")
            .join("nomic-embed-text-v1.5-q8.onnx");

        if !path.exists() {
            return Err(format!(
                "Model not found at {:?}. Download from https://huggingface.co/nomic-ai/nomic-embed-text-v1.5",
                path
            )
            .into());
        }

        let session = Session::builder()?.commit_from_file(&path)?;
        self.session = Some(session);
        Ok(())
    }

    pub fn is_available(&mut self) -> bool {
        self.ensure_loaded().is_ok()
    }

    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.ensure_loaded()?;
        let session = self.session.as_mut().unwrap();

        let tokens = tokenize(text);
        let seq_len = tokens.len().min(MAX_SEQ_LEN);
        let padded_len = seq_len.next_power_of_two().max(8);

        let mut input_ids = vec![0i64; padded_len];
        let mut attention_mask = vec![0i64; padded_len];
        let token_types = vec![0i64; padded_len];

        for (i, &t) in tokens.iter().take(seq_len).enumerate() {
            input_ids[i] = t;
            attention_mask[i] = 1;
        }

        let input_names: Vec<String> = session.inputs().iter().map(|i| i.name().to_string()).collect();

        let input_ids_val = Value::from_array(([1usize, padded_len], input_ids.clone()))?;
        let attention_val = Value::from_array(([1usize, padded_len], attention_mask.clone()))?;
        let token_types_val = Value::from_array(([1usize, padded_len], token_types.clone()))?;

        let inputs: Vec<(&str, Value)> = vec![
            (input_names[0].as_str(), input_ids_val.into()),
            (input_names[1].as_str(), attention_val.into()),
            (input_names[2].as_str(), token_types_val.into()),
        ];

        let outputs = session.run(inputs)?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>()?;
        let pooled = mean_pool(data, &attention_mask, DIMS);

        let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
        if norm > 0.0 {
            let normalized: Vec<f32> = pooled.iter().map(|v| v / norm).collect();
            Ok(normalized)
        } else {
            Ok(pooled)
        }
    }

    pub fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        self.ensure_loaded()?;

        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed_one(text)?);
        }
        Ok(results)
    }

    pub fn model_name(&self) -> &str {
        &self.model_name
    }

    pub fn dims() -> usize {
        DIMS
    }
}

pub fn mean_pool(last_hidden: &[f32], attention_mask: &[i64], dims: usize) -> Vec<f32> {
    let seq_len = last_hidden.len() / dims;
    let mut pooled = vec![0.0f32; dims];
    let mut mask_sum = 0.0f32;

    for i in 0..seq_len {
        let mask_val = if i < attention_mask.len() && attention_mask[i] == 1 {
            1.0f32
        } else {
            0.0f32
        };
        mask_sum += mask_val;
        for j in 0..dims {
            pooled[j] += mask_val * last_hidden[i * dims + j];
        }
    }

    if mask_sum > 0.0 {
        for v in pooled.iter_mut() {
            *v /= mask_sum;
        }
    }

    pooled
}

pub fn vector_to_blob(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn blob_to_vector(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

fn tokenize(text: &str) -> Vec<i64> {
    let mut tokens = Vec::new();
    for word in text.split_whitespace() {
        let mut chars = word.chars().peekable();
        tokens.push(101); // [CLS]
        while chars.peek().is_some() {
            tokens.push(chars.next().unwrap() as i64 + 256);
        }
        if tokens.len() >= MAX_SEQ_LEN {
            tokens.truncate(MAX_SEQ_LEN);
            tokens[MAX_SEQ_LEN - 1] = 102; // [SEP]
            return tokens;
        }
    }
    if tokens.is_empty() {
        tokens.push(101);
    }
    tokens.push(102); // [SEP]
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn test_embedder_load_and_embed() {
        let mut embedder = Embedder::new().unwrap();
        if !embedder.is_available() {
            println!("Skipping: model not found");
            return;
        }

        let start = Instant::now();
        let vec = embedder.embed_one("hello world").unwrap();
        let elapsed = start.elapsed();

        assert_eq!(vec.len(), DIMS);
        let norm: f32 = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 0.01, "Should be unit normalized, got {}", norm);
        println!("Single embed: {:?} ({} dims)", elapsed, vec.len());
    }

    #[test]
    fn test_embed_batch() {
        let mut embedder = Embedder::new().unwrap();
        if !embedder.is_available() {
            println!("Skipping: model not found");
            return;
        }

        let texts = vec!["fn main() {}", "struct User { name: String }", "import React from 'react'"];
        let start = Instant::now();
        let results = embedder.embed_batch(&texts).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 3);
        for r in &results {
            assert_eq!(r.len(), DIMS);
        }
        println!("Batch embed ({}): {:?}", texts.len(), elapsed);
    }

    #[test]
    fn test_vector_blob_roundtrip() {
        let original = vec![0.1f32, -0.2, 0.3, 1.0, 0.0];
        let blob = vector_to_blob(&original);
        assert_eq!(blob.len(), original.len() * 4);
        let restored = blob_to_vector(&blob);
        assert_eq!(original.len(), restored.len());
        for (a, b) in original.iter().zip(restored.iter()) {
            assert!((a - b).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn test_mean_pool() {
        let hidden = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]; // 2 tokens, 3 dims
        let mask = vec![1, 1];
        let pooled = mean_pool(&hidden, &mask, 3);
        assert_eq!(pooled.len(), 3);
        assert_eq!(pooled[0], 2.5); // (1+4)/2
        assert_eq!(pooled[1], 3.5); // (2+5)/2
        assert_eq!(pooled[2], 4.5); // (3+6)/2
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|v| v * v).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}
