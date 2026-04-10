use std::io::Read;

const DEFAULT_MODEL: &str = "nomic-embed-text:latest";
const DIMS: usize = 768;

const QUERY_PREFIX: &str = "search_query: ";
const DOCUMENT_PREFIX: &str = "search_document: ";

pub struct Embedder {
    base_url: String,
    model: String,
    available: Option<bool>,
}

impl Embedder {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let base_url =
            std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://localhost:11434".to_string());
        let model = std::env::var("SQMD_EMBED_MODEL").unwrap_or_else(|_| DEFAULT_MODEL.to_string());
        Ok(Self {
            base_url,
            model,
            available: None,
        })
    }

    pub fn is_available(&mut self) -> bool {
        if let Some(a) = self.available {
            return a;
        }
        let url = format!("{}/api/tags", self.base_url);
        let result = ureq::Agent::new_with_defaults()
            .get(&url)
            .call()
            .and_then(|resp| {
                let mut body = String::new();
                resp.into_body().into_reader().read_to_string(&mut body)?;
                Ok(body)
            });
        let ok = result.is_ok();
        self.available = Some(ok);
        ok
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
        let full_text = if prefix.is_empty() {
            text.to_string()
        } else {
            format!("{}{}", prefix, text)
        };
        let results = self.call_ollama_embed(&[full_text])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| "No embedding returned".into())
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

        let prefixed: Vec<String> = texts
            .iter()
            .map(|t| {
                if prefix.is_empty() {
                    t.to_string()
                } else {
                    format!("{}{}", prefix, t)
                }
            })
            .collect();

        let batch_size = 64;
        let mut all_results = Vec::with_capacity(texts.len());

        for chunk in prefixed.chunks(batch_size) {
            match self.call_ollama_embed(chunk) {
                Ok(results) => all_results.extend(results),
                Err(e) => {
                    eprintln!(
                        "[embed] batch failed ({} texts), falling back to single: {e}",
                        chunk.len()
                    );
                    for text in chunk {
                        match self.call_ollama_embed(std::slice::from_ref(text)) {
                            Ok(r) => all_results.extend(r),
                            Err(_) => {
                                all_results.push(vec![0.0f32; DIMS]);
                            }
                        }
                    }
                }
            }
        }

        Ok(all_results)
    }

    fn call_ollama_embed(
        &mut self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        let body = serde_json::json!({
            "model": self.model,
            "input": texts,
        });

        let url = format!("{}/api/embed", self.base_url);
        let response = ureq::Agent::new_with_defaults()
            .post(&url)
            .send_json(&body)?;

        let mut body_str = String::new();
        response
            .into_body()
            .into_reader()
            .read_to_string(&mut body_str)?;
        let parsed: serde_json::Value = serde_json::from_str(&body_str)?;

        let embeddings = parsed["embeddings"]
            .as_array()
            .ok_or_else(|| "Unexpected embed response: no embeddings array".to_string())?;

        let mut results = Vec::with_capacity(embeddings.len());
        for emb in embeddings {
            let vec: Vec<f32> = emb
                .as_array()
                .ok_or("Embedding is not an array")?
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            results.push(vec);
        }

        Ok(results)
    }

    pub fn model_name(&self) -> &str {
        &self.model
    }

    pub fn dims() -> usize {
        DIMS
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embedder_create() {
        let embedder = Embedder::new().unwrap();
        assert_eq!(embedder.model, "nomic-embed-text:latest");
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
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.001);

        let c = vec![0.0, 1.0, 0.0];
        assert!(cosine_similarity(&a, &c).abs() < 0.001);
    }
}
