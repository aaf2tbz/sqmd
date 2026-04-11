pub trait EmbedProvider: Send {
    fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>>;
    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>>;
    fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>>;
    fn is_available(&mut self) -> bool;
    fn model_name(&self) -> &str;
}

pub fn make_provider() -> Result<Box<dyn EmbedProvider>, Box<dyn std::error::Error>> {
    let rt = crate::native::NativeRuntime::new()?;
    Ok(Box::new(rt))
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
