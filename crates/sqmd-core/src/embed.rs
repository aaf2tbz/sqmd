use ort::session::Session;
use ort::value::Value;
use std::io::{Read, Write};
use std::path::PathBuf;

const DIMS: usize = 768;
const MODEL_NAME: &str = "nomic-embed-text-v1.5";
const MODEL_FILE: &str = "nomic-embed-text-v1.5-q8.onnx";
const TOKENIZER_FILE: &str = "nomic-embed-text-v1.5-tokenizer.json";
const MODEL_URL: &str = "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main/nomic-embed-text-v1.5-q8.onnx";
const TOKENIZER_URL: &str =
    "https://huggingface.co/nomic-ai/nomic-embed-text-v1.5/resolve/main/tokenizer.json";
const MAX_SEQ_LEN: usize = 512;

const QUERY_PREFIX: &str = "search_query: ";
const DOCUMENT_PREFIX: &str = "search_document: ";

pub struct Embedder {
    session: Option<Session>,
    tokenizer: Option<tokenizers::Tokenizer>,
    model_name: String,
}

impl Embedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            session: None,
            tokenizer: None,
            model_name: MODEL_NAME.to_string(),
        })
    }

    pub fn model_dir() -> Result<PathBuf, Box<dyn std::error::Error>> {
        let home = std::env::var("HOME").map_err(|_| "HOME env var not set")?;
        let dir = PathBuf::from(home).join(".sqmd").join("models");
        Ok(dir)
    }

    pub fn model_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(Self::model_dir()?.join(MODEL_FILE))
    }

    fn tokenizer_path() -> Result<PathBuf, Box<dyn std::error::Error>> {
        Ok(Self::model_dir()?.join(TOKENIZER_FILE))
    }

    fn download_file(
        url: &str,
        dest: &PathBuf,
        label: &str,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if dest.exists() {
            return Ok(());
        }

        let dir = dest.parent().unwrap();
        std::fs::create_dir_all(dir)?;

        eprintln!("Downloading {label}...");
        eprintln!("  URL: {url}");
        eprintln!("  Destination: {:?}", dest);

        let response = ureq::Agent::new_with_defaults().get(url).call()?;

        let total_bytes: usize = response
            .headers()
            .get("content-length")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);

        let mut reader = response.into_body().into_reader();
        let mut file = std::fs::File::create(dest)?;
        let mut downloaded: usize = 0;

        let mut buf = [0u8; 8192];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n])?;
            downloaded += n;
            if total_bytes > 0 {
                let pct = (downloaded as f64 / total_bytes as f64) * 100.0;
                eprint!("\r  Downloading: {:.0}%", pct);
            }
        }

        eprintln!("\n  Saved {} KB", downloaded / 1024);
        Ok(())
    }

    pub fn ensure_model_exists() -> Result<(), Box<dyn std::error::Error>> {
        let model_path = Self::model_path()?;
        Self::download_file(MODEL_URL, &model_path, "embedding model")?;
        let tokenizer_path = Self::tokenizer_path()?;
        Self::download_file(TOKENIZER_URL, &tokenizer_path, "tokenizer")?;
        Ok(())
    }

    pub fn ensure_loaded(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.session.is_some() && self.tokenizer.is_some() {
            return Ok(());
        }

        if !Self::model_path()?.exists() || !Self::tokenizer_path()?.exists() {
            Self::ensure_model_exists()?;
        }

        let session = Session::builder()?.commit_from_file(&Self::model_path()?)?;
        self.session = Some(session);

        let tokenizer = tokenizers::Tokenizer::from_file(&Self::tokenizer_path()?)
            .map_err(|e| format!("Failed to load tokenizer: {e}"))?;
        let mut with_padding = tokenizer.clone();
        let padding = tokenizers::PaddingParams {
            strategy: tokenizers::PaddingStrategy::BatchLongest,
            ..Default::default()
        };
        with_padding.with_padding(Some(padding));
        self.tokenizer = Some(with_padding);

        Ok(())
    }

    pub fn is_available(&mut self) -> bool {
        self.ensure_loaded().is_ok()
    }

    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.embed_with_prefix(text, "")
    }

    pub fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.embed_with_prefix(text, QUERY_PREFIX)
    }

    pub fn embed_document(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.embed_with_prefix(text, DOCUMENT_PREFIX)
    }

    fn embed_with_prefix(
        &mut self,
        text: &str,
        prefix: &str,
    ) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.ensure_loaded()?;
        let session = self.session.as_mut().unwrap();
        let tokenizer = self.tokenizer.as_ref().unwrap();

        let full_text = if prefix.is_empty() {
            text.to_string()
        } else {
            format!("{}{}", prefix, text)
        };
        let encodings = tokenizer
            .encode(full_text.as_str(), true)
            .map_err(|e| format!("Tokenization failed: {e}"))?;
        let ids = encodings.get_ids();

        let seq_len = ids.len().min(MAX_SEQ_LEN);
        let padded_len = ceil_to_multiple(seq_len, 64).max(8);

        let mut input_ids = vec![0i64; padded_len];
        let mut attention_mask = vec![0i64; padded_len];
        let token_types = vec![0i64; padded_len];

        for (i, &t) in ids.iter().take(seq_len).enumerate() {
            input_ids[i] = t as i64;
            attention_mask[i] = 1;
        }

        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|i| i.name().to_string())
            .collect();

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

    pub fn embed_batch_documents(
        &mut self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        self.embed_batch_with_prefix(texts, DOCUMENT_PREFIX)
    }

    pub fn embed_batch_queries(
        &mut self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        self.embed_batch_with_prefix(texts, QUERY_PREFIX)
    }

    pub fn embed_batch(
        &mut self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        self.embed_batch_with_prefix(texts, "")
    }

    fn embed_batch_with_prefix(
        &mut self,
        texts: &[&str],
        prefix: &str,
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        self.ensure_loaded()?;
        let session = self.session.as_mut().unwrap();
        let tokenizer = self.tokenizer.as_ref().unwrap();

        let batch_size = texts.len();

        let mut encoded: Vec<(Vec<i64>, usize)> = Vec::with_capacity(batch_size);
        for text in texts.iter() {
            let full_text = if prefix.is_empty() {
                text.to_string()
            } else {
                format!("{}{}", prefix, text)
            };
            let encodings = tokenizer
                .encode(full_text.as_str(), true)
                .map_err(|e| format!("Tokenization failed: {e}"))?;
            let seq_len = encodings.get_ids().len().min(MAX_SEQ_LEN);
            let ids: Vec<i64> = encodings
                .get_ids()
                .iter()
                .take(seq_len)
                .map(|&t| t as i64)
                .collect();
            encoded.push((ids, seq_len));
        }

        let padded_len = encoded
            .iter()
            .map(|(_, len)| ceil_to_multiple(*len, 64).max(8))
            .max()
            .unwrap_or(8);

        let mut all_input_ids = vec![0i64; batch_size * padded_len];
        let mut all_attention = vec![0i64; batch_size * padded_len];
        let all_token_types = vec![0i64; batch_size * padded_len];

        for (i, (ids, _seq_len)) in encoded.iter().enumerate() {
            let offset = i * padded_len;
            for (j, &t) in ids.iter().enumerate() {
                all_input_ids[offset + j] = t;
                all_attention[offset + j] = 1;
            }
        }

        let input_names: Vec<String> = session
            .inputs()
            .iter()
            .map(|inp| inp.name().to_string())
            .collect();

        let input_ids_val = Value::from_array(([batch_size, padded_len], all_input_ids))?;
        let attention_val = Value::from_array(([batch_size, padded_len], all_attention.clone()))?;
        let token_types_val = Value::from_array(([batch_size, padded_len], all_token_types))?;

        let inputs: Vec<(&str, Value)> = vec![
            (input_names[0].as_str(), input_ids_val.into()),
            (input_names[1].as_str(), attention_val.into()),
            (input_names[2].as_str(), token_types_val.into()),
        ];

        let outputs = session.run(inputs)?;
        let (_, data) = outputs[0].try_extract_tensor::<f32>()?;

        let mut results = Vec::with_capacity(batch_size);
        for i in 0..batch_size {
            let mask_start = i * padded_len;
            let mask: Vec<i64> = all_attention[mask_start..mask_start + padded_len].to_vec();
            let hidden_start = i * padded_len * DIMS;
            let hidden_end = hidden_start + padded_len * DIMS;
            let hidden = &data[hidden_start..hidden_end];
            let pooled = mean_pool(hidden, &mask, DIMS);
            let norm: f32 = pooled.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm > 0.0 {
                let normalized: Vec<f32> = pooled.iter().map(|v| v / norm).collect();
                results.push(normalized);
            } else {
                results.push(pooled);
            }
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

fn ceil_to_multiple(val: usize, multiple: usize) -> usize {
    ((val + multiple - 1) / multiple) * multiple
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
        assert!(
            (norm - 1.0).abs() < 0.01,
            "Should be unit normalized, got {}",
            norm
        );
        println!("Single embed: {:?} ({} dims)", elapsed, vec.len());
    }

    #[test]
    fn test_embed_batch() {
        let mut embedder = Embedder::new().unwrap();
        if !embedder.is_available() {
            println!("Skipping: model not found");
            return;
        }

        let texts = vec![
            "fn main() {}",
            "struct User { name: String }",
            "import React from 'react'",
        ];
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
