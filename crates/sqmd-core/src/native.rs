use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::embed::EmbedProvider;

const DEFAULT_EMBED_MODEL_NAME: &str = "mxbai-embed-large";
const EMBED_DIMS: usize = 1024;

static BACKEND: OnceLock<Mutex<llama_cpp_2::llama_backend::LlamaBackend>> = OnceLock::new();
static INIT_LOCK: Mutex<()> = Mutex::new(());

fn get_backend(
) -> Result<&'static Mutex<llama_cpp_2::llama_backend::LlamaBackend>, Box<dyn std::error::Error>> {
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let _guard = INIT_LOCK.lock().unwrap();
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let backend = llama_cpp_2::llama_backend::LlamaBackend::init()
        .map_err(|e| format!("backend init failed: {}", e))?;
    let _ = BACKEND.set(Mutex::new(backend));
    BACKEND
        .get()
        .ok_or_else(|| "backend not initialized".into())
}

pub struct NativeRuntime {
    model: llama_cpp_2::model::LlamaModel,
    model_path: PathBuf,
}

unsafe impl Send for NativeRuntime {}

impl NativeRuntime {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let model_path = Self::find_model()?;
        eprintln!("[native] loading model from {:?}", model_path);
        Self::from_path(&model_path)
    }

    pub fn from_path(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let backend = get_backend()?;
        let backend_guard = backend.lock().unwrap();

        let model_params =
            llama_cpp_2::model::params::LlamaModelParams::default().with_n_gpu_layers(99);

        let model =
            llama_cpp_2::model::LlamaModel::load_from_file(&backend_guard, path, &model_params)
                .map_err(|e| format!("failed to load model from {:?}: {}", path, e))?;

        Ok(Self {
            model,
            model_path: path.to_path_buf(),
        })
    }

    fn find_model() -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Ok(path) = std::env::var("SQMD_NATIVE_MODEL") {
            let p = PathBuf::from(&path);
            if p.exists() {
                return Ok(p);
            }
            eprintln!(
                "[native] SQMD_NATIVE_MODEL={} not found, searching ollama store",
                path
            );
        }

        let ollama_home = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| {
            format!(
                "{}/.ollama/models",
                std::env::var("HOME").unwrap_or_default()
            )
        });

        let manifest_path = format!(
            "{}/manifests/registry.ollama.ai/library/{}/latest",
            ollama_home, DEFAULT_EMBED_MODEL_NAME
        );

        let manifest_str = std::fs::read_to_string(&manifest_path)
            .map_err(|_| format!("model manifest not found at {}", manifest_path))?;

        let manifest: serde_json::Value = serde_json::from_str(&manifest_str)?;
        let layers = manifest["layers"]
            .as_array()
            .ok_or("manifest has no layers")?;

        for layer in layers {
            let media_type = layer["mediaType"].as_str().unwrap_or("");
            if media_type.contains("gguf") {
                let digest = layer["digest"].as_str().ok_or("layer missing digest")?;
                let blob_path = format!("{}/blobs/{}", ollama_home, digest.replace(":", "-"));
                if std::path::Path::new(&blob_path).exists() {
                    return Ok(PathBuf::from(blob_path));
                }
            }
        }

        for layer in layers {
            let digest = layer["digest"].as_str().unwrap_or("");
            if digest.is_empty() {
                continue;
            }
            let size = layer["size"].as_u64().unwrap_or(0);
            if size > 100_000_000 {
                let blob_path = format!("{}/blobs/{}", ollama_home, digest.replace(":", "-"));
                if std::path::Path::new(&blob_path).exists() {
                    return Ok(PathBuf::from(blob_path));
                }
            }
        }

        Err(format!(
            "no GGUF blob found for {} — set SQMD_NATIVE_MODEL to a GGUF path or install via ollama",
            DEFAULT_EMBED_MODEL_NAME
        )
        .into())
    }

    pub fn embed_batch(
        &mut self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let backend = get_backend()?;
        let backend_guard = backend.lock().unwrap();

        let ctx_params = llama_cpp_2::context::params::LlamaContextParams::default()
            .with_n_ctx(std::num::NonZero::new(512u32))
            .with_n_batch(512)
            .with_embeddings(true);

        let mut ctx = self
            .model
            .new_context(&backend_guard, ctx_params)
            .map_err(|e| format!("failed to create context: {}", e))?;

        let mut all_embeddings = Vec::with_capacity(texts.len());

        for text in texts {
            let tokens = self
                .model
                .str_to_token(text, llama_cpp_2::model::AddBos::Always)
                .unwrap_or_else(|_| vec![self.model.token_bos()]);

            if tokens.is_empty() {
                all_embeddings.push(vec![0.0f32; EMBED_DIMS]);
                continue;
            }

            let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(tokens.len(), 1);
            for (i, token) in tokens.iter().enumerate() {
                batch.add(*token, i as i32, &[0], false)?;
            }

            ctx.encode(&mut batch)
                .map_err(|e| format!("encode failed: {}", e))?;

            let embedding = ctx
                .embeddings_seq_ith(0)
                .map_err(|e| format!("embedding extraction failed: {}", e))?;

            all_embeddings.push(embedding.to_vec());
        }

        Ok(all_embeddings)
    }

    pub fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let results = self.embed_batch(&[text])?;
        results
            .into_iter()
            .next()
            .ok_or_else(|| "no embedding returned".into())
    }

    pub fn model_path(&self) -> &std::path::Path {
        &self.model_path
    }
}

impl EmbedProvider for NativeRuntime {
    fn embed_one(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        NativeRuntime::embed_one(self, text)
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        self.embed_one(text)
    }

    fn embed_batch(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        NativeRuntime::embed_batch(self, texts)
    }

    fn is_available(&mut self) -> bool {
        true
    }

    fn model_name(&self) -> &str {
        DEFAULT_EMBED_MODEL_NAME
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_model() {
        let path = NativeRuntime::find_model();
        if let Ok(p) = &path {
            assert!(p.exists(), "model file should exist: {:?}", p);
            eprintln!("[test] found model at {:?}", p);
        } else {
            eprintln!("[test] model not found (skipping): {:?}", path.err());
        }
    }

    #[test]
    fn test_embed_one() {
        let mut rt = match NativeRuntime::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[test] skipping: {}", e);
                return;
            }
        };
        let vec = rt
            .embed_one("fn authenticate(user: &str, token: &str) -> Result<bool>")
            .unwrap();
        assert_eq!(vec.len(), EMBED_DIMS);
        let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!(norm > 0.0, "embedding should not be zero");
        eprintln!("[test] embedding norm = {:.4}", norm);
    }

    #[test]
    fn test_embed_batch() {
        let mut rt = match NativeRuntime::new() {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[test] skipping: {}", e);
                return;
            }
        };
        let texts = vec!["fn login()", "struct User", "enum Color { Red, Blue }"];
        let refs: Vec<&str> = texts.iter().copied().collect();
        let vecs = rt.embed_batch(&refs).unwrap();
        assert_eq!(vecs.len(), 3);
        for (i, v) in vecs.iter().enumerate() {
            assert_eq!(v.len(), EMBED_DIMS, "embedding {} wrong dim", i);
        }
        eprintln!("[test] batch of 3 embeddings OK");
    }
}
