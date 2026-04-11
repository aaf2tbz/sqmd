use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use crate::embed::EmbedProvider;

const DEFAULT_EMBED_MODEL_NAME: &str = "mxbai-embed-large";
const DEFAULT_HINT_MODEL_NAME: &str = "phi4-mini";
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
        Self::find_named_model(DEFAULT_EMBED_MODEL_NAME, "SQMD_NATIVE_MODEL")
    }

    fn find_named_model(
        model_name: &str,
        env_var: &str,
    ) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Ok(path) = std::env::var(env_var) {
            let p = PathBuf::from(&path);
            if p.exists() {
                return Ok(p);
            }
            eprintln!(
                "[native] {}={} not found, searching model store",
                env_var, path
            );
        }

        let store_home = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| {
            format!(
                "{}/.ollama/models",
                std::env::var("HOME").unwrap_or_default()
            )
        });

        let manifest_path = format!(
            "{}/manifests/registry.ollama.ai/library/{}/latest",
            store_home, model_name
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
                let blob_path = format!("{}/blobs/{}", store_home, digest.replace(":", "-"));
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
                let blob_path = format!("{}/blobs/{}", store_home, digest.replace(":", "-"));
                if std::path::Path::new(&blob_path).exists() {
                    return Ok(PathBuf::from(blob_path));
                }
            }
        }

        Err(format!(
            "no GGUF blob found for {} — set {} to a GGUF path",
            model_name, env_var
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

pub struct NativeGenerator {
    model: llama_cpp_2::model::LlamaModel,
    model_path: PathBuf,
}

unsafe impl Send for NativeGenerator {}

impl NativeGenerator {
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let model_name = std::env::var("SQMD_HINT_MODEL")
            .unwrap_or_else(|_| DEFAULT_HINT_MODEL_NAME.to_string());
        let model_path = Self::find_named_model(&model_name)?;
        eprintln!("[native] loading generator model from {:?}", model_path);
        Self::from_path(&model_path)
    }

    pub fn from_path(path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let backend = get_backend()?;
        let backend_guard = backend.lock().unwrap();

        let model_params =
            llama_cpp_2::model::params::LlamaModelParams::default().with_n_gpu_layers(99);

        let model =
            llama_cpp_2::model::LlamaModel::load_from_file(&backend_guard, path, &model_params)
                .map_err(|e| format!("failed to load generator model from {:?}: {}", path, e))?;

        Ok(Self {
            model,
            model_path: path.to_path_buf(),
        })
    }

    fn find_named_model(model_name: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
        if let Ok(path) = std::env::var("SQMD_HINT_MODEL_PATH") {
            let p = PathBuf::from(&path);
            if p.exists() {
                return Ok(p);
            }
        }

        let name = model_name.split(':').next().unwrap_or(model_name);

        let store_home = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| {
            format!(
                "{}/.ollama/models",
                std::env::var("HOME").unwrap_or_default()
            )
        });

        let manifest_path = format!(
            "{}/manifests/registry.ollama.ai/library/{}/latest",
            store_home, name
        );

        if let Ok(manifest_str) = std::fs::read_to_string(&manifest_path) {
            if let Ok(manifest) = serde_json::from_str::<serde_json::Value>(&manifest_str) {
                if let Some(layers) = manifest["layers"].as_array() {
                    for layer in layers {
                        let media_type = layer["mediaType"].as_str().unwrap_or("");
                        if media_type.contains("gguf") {
                            if let Some(digest) = layer["digest"].as_str() {
                                let blob_path =
                                    format!("{}/blobs/{}", store_home, digest.replace(":", "-"));
                                if std::path::Path::new(&blob_path).exists() {
                                    return Ok(PathBuf::from(blob_path));
                                }
                            }
                        }
                    }
                    for layer in layers {
                        if let Some(digest) = layer["digest"].as_str() {
                            if digest.is_empty() {
                                continue;
                            }
                            let size = layer["size"].as_u64().unwrap_or(0);
                            if size > 100_000_000 {
                                let blob_path =
                                    format!("{}/blobs/{}", store_home, digest.replace(":", "-"));
                                if std::path::Path::new(&blob_path).exists() {
                                    return Ok(PathBuf::from(blob_path));
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(format!(
            "no GGUF found for hint model '{}' — set SQMD_HINT_MODEL_PATH to a GGUF file path",
            model_name
        )
        .into())
    }

    pub fn generate(
        &mut self,
        prompt: &str,
        max_tokens: u32,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let backend = get_backend()?;
        let backend_guard = backend.lock().unwrap();

        let n_ctx = std::num::NonZero::new(2048u32).unwrap();
        let ctx_params = llama_cpp_2::context::params::LlamaContextParams::default()
            .with_n_ctx(Some(n_ctx))
            .with_n_batch(512);

        let mut ctx = self
            .model
            .new_context(&backend_guard, ctx_params)
            .map_err(|e| format!("generator context creation failed: {}", e))?;

        let tokens = self
            .model
            .str_to_token(prompt, llama_cpp_2::model::AddBos::Always)
            .unwrap_or_else(|_| vec![self.model.token_bos()]);

        if tokens.is_empty() {
            return Ok(String::new());
        }

        let n_ctx_val = n_ctx.get() as usize;
        let prompt_tokens = tokens.len().min(n_ctx_val);
        let tokens = &tokens[..prompt_tokens];

        let mut batch = llama_cpp_2::llama_batch::LlamaBatch::new(prompt_tokens, 1);
        for (i, token) in tokens.iter().enumerate() {
            let is_last = i == prompt_tokens - 1;
            batch.add(*token, i as i32, &[0], is_last)?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| format!("generator decode failed: {}", e))?;

        let eos = self.model.token_eos();
        let nl = self
            .model
            .str_to_token("\n", llama_cpp_2::model::AddBos::Never)
            .ok()
            .and_then(|v| v.first().copied());

        let mut sampler = llama_cpp_2::sampling::LlamaSampler::chain_simple([
            llama_cpp_2::sampling::LlamaSampler::top_k(40),
            llama_cpp_2::sampling::LlamaSampler::top_p(0.9, 1),
            llama_cpp_2::sampling::LlamaSampler::temp(0.8),
            llama_cpp_2::sampling::LlamaSampler::dist(42),
        ]);

        let mut generated_tokens = Vec::new();
        let mut n_past = prompt_tokens as i32;

        for _ in 0..max_tokens {
            let new_token = sampler.sample(&ctx, -1);
            sampler.accept(new_token);

            if new_token == eos {
                break;
            }
            if Some(new_token) == nl && !generated_tokens.is_empty() {
                break;
            }

            generated_tokens.push(new_token);

            let mut cont_batch = llama_cpp_2::llama_batch::LlamaBatch::new(1, 1);
            cont_batch.add(new_token, n_past, &[0], true)?;

            ctx.decode(&mut cont_batch)
                .map_err(|e| format!("generator continuation decode failed: {}", e))?;
            n_past += 1;

            if n_past as u32 >= n_ctx.get() - 1 {
                break;
            }
        }

        let text = generated_tokens
            .iter()
            .filter_map(|t| {
                self.model
                    .token_to_piece_bytes(*t, 32, true, None)
                    .ok()
                    .and_then(|bytes| String::from_utf8(bytes).ok())
            })
            .collect::<String>();

        Ok(text)
    }

    pub fn generate_prospective_hints(
        &mut self,
        content: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let truncated = if content.len() > 1500 {
            &content[..1500]
        } else {
            content
        };

        let prompt = format!(
            "3 search queries for this code, one per line, no explanation:\n{}",
            truncated
        );

        let output = self.generate(&prompt, 128)?;

        let hints: Vec<String> = output
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .take(3)
            .collect();

        Ok(hints)
    }

    pub fn model_path_val(&self) -> &std::path::Path {
        &self.model_path
    }
}
