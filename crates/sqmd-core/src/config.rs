use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ProjectConfig {
    pub sqlite: SqliteConfig,
    pub search: SearchConfig,
    pub chunking: ChunkingConfig,
    #[serde(default)]
    pub importance: HashMap<String, f64>,
    pub hints: HintsConfig,
    pub embed: EmbedConfig,
    pub watch: WatchConfig,
    pub context: ContextConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SqliteConfig {
    pub mmap_size: Option<String>,
    pub cache_size: Option<String>,
    pub wal_autocheckpoint: Option<i64>,
    pub busy_timeout: Option<i64>,
}

impl Default for SqliteConfig {
    fn default() -> Self {
        Self {
            mmap_size: None,
            cache_size: None,
            wal_autocheckpoint: None,
            busy_timeout: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    pub default_top_k: Option<usize>,
    pub default_max_tokens: Option<usize>,
    pub graph_boost_base: Option<f64>,
    pub graph_boost_decay: Option<f64>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            default_top_k: None,
            default_max_tokens: None,
            graph_boost_base: None,
            graph_boost_decay: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ChunkingConfig {
    pub unclaimed_gap: Option<usize>,
    pub min_importance: Option<f64>,
}

impl Default for ChunkingConfig {
    fn default() -> Self {
        Self {
            unclaimed_gap: None,
            min_importance: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HintsConfig {
    pub min_importance: Option<f64>,
    pub max_per_chunk: Option<usize>,
}

impl Default for HintsConfig {
    fn default() -> Self {
        Self {
            min_importance: None,
            max_per_chunk: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct EmbedConfig {
    pub model: Option<String>,
    pub hint_model: Option<String>,
}

impl Default for EmbedConfig {
    fn default() -> Self {
        Self {
            model: None,
            hint_model: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WatchConfig {
    pub debounce_ms: Option<u64>,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self { debounce_ms: None }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    pub max_dep_chunks: Option<usize>,
    pub default_dep_depth: Option<usize>,
    pub community_boost: Option<f64>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_dep_chunks: None,
            default_dep_depth: None,
            community_boost: None,
        }
    }
}

impl ProjectConfig {
    pub fn load(project_root: &Path) -> Self {
        let config_path = project_root.join(".sqmd").join("config.toml");
        if !config_path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&config_path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(config) => config,
                Err(e) => {
                    eprintln!("[config] failed to parse {:?}: {}", config_path, e);
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("[config] failed to read {:?}: {}", config_path, e);
                Self::default()
            }
        }
    }

    pub fn mmap_size_bytes(&self) -> i64 {
        self.sqlite
            .mmap_size
            .as_deref()
            .and_then(parse_size)
            .unwrap_or(268435456)
    }

    pub fn cache_size_pages(&self) -> i64 {
        self.sqlite
            .cache_size
            .as_deref()
            .and_then(|s| s.strip_prefix('-').and_then(|v| v.parse::<i64>().ok()))
            .map(|v| -v)
            .unwrap_or(-8000)
    }

    pub fn wal_autocheckpoint(&self) -> i64 {
        self.sqlite.wal_autocheckpoint.unwrap_or(1000)
    }

    pub fn busy_timeout(&self) -> i64 {
        self.sqlite.busy_timeout.unwrap_or(5000)
    }

    pub fn graph_boost_base(&self) -> f64 {
        self.search.graph_boost_base.unwrap_or(0.20)
    }

    pub fn graph_boost_decay(&self) -> f64 {
        self.search.graph_boost_decay.unwrap_or(0.50)
    }

    pub fn hint_min_importance(&self) -> f64 {
        self.hints.min_importance.unwrap_or(0.5)
    }

    pub fn hint_max_per_chunk(&self) -> usize {
        self.hints.max_per_chunk.unwrap_or(3)
    }

    pub fn embed_model(&self) -> Option<&str> {
        self.embed.model.as_deref()
    }

    pub fn hint_model(&self) -> Option<&str> {
        self.embed.hint_model.as_deref()
    }

    pub fn debounce_duration(&self) -> std::time::Duration {
        std::time::Duration::from_millis(self.watch.debounce_ms.unwrap_or(200))
    }

    pub fn override_importance(&self, chunk_type: &str) -> Option<f64> {
        self.importance.get(chunk_type).copied()
    }

    pub fn max_dep_chunks(&self) -> usize {
        self.context.max_dep_chunks.unwrap_or(50)
    }

    pub fn default_dep_depth(&self) -> usize {
        self.context.default_dep_depth.unwrap_or(2)
    }

    pub fn community_boost(&self) -> f64 {
        self.context.community_boost.unwrap_or(0.1)
    }
}

fn parse_size(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num_str, multiplier): (&str, i64) = if s.ends_with("MB") {
        (s.trim_end_matches("MB"), 1024 * 1024)
    } else if s.ends_with("GB") {
        (s.trim_end_matches("GB"), 1024 * 1024 * 1024)
    } else if s.ends_with("KB") {
        (s.trim_end_matches("KB"), 1024)
    } else if s.ends_with('B') {
        (s.trim_end_matches('B'), 1)
    } else {
        (s, 1)
    };
    num_str.trim().parse::<i64>().ok().map(|n| n * multiplier)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_size_works() {
        assert_eq!(parse_size("256MB"), Some(256 * 1024 * 1024));
        assert_eq!(parse_size("1GB"), Some(1024 * 1024 * 1024));
        assert_eq!(parse_size("512KB"), Some(512 * 1024));
        assert_eq!(parse_size("1024B"), Some(1024));
        assert_eq!(parse_size("268435456"), Some(268435456));
        assert_eq!(parse_size("invalid"), None);
    }

    #[test]
    fn load_missing_config_returns_defaults() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ProjectConfig::load(tmp.path());
        assert_eq!(config.mmap_size_bytes(), 268435456);
        assert_eq!(config.cache_size_pages(), -8000);
        assert_eq!(config.wal_autocheckpoint(), 1000);
        assert_eq!(config.busy_timeout(), 5000);
    }

    #[test]
    fn parse_valid_config() {
        let toml_str = r#"
[sqlite]
mmap_size = "512MB"
busy_timeout = 10000

[search]
default_top_k = 20
graph_boost_base = 0.3

[importance]
function = 0.95
import = 0.05

[hints]
min_importance = 0.7

[watch]
debounce_ms = 500
"#;
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mmap_size_bytes(), 512 * 1024 * 1024);
        assert_eq!(config.busy_timeout(), 10000);
        assert_eq!(config.search.default_top_k, Some(20));
        assert_eq!(config.graph_boost_base(), 0.3);
        assert_eq!(config.override_importance("function"), Some(0.95));
        assert_eq!(config.override_importance("import"), Some(0.05));
        assert_eq!(config.override_importance("class"), None);
        assert_eq!(config.hint_min_importance(), 0.7);
        assert_eq!(
            config.debounce_duration(),
            std::time::Duration::from_millis(500)
        );
    }

    #[test]
    fn cache_size_negative() {
        let toml_str = r#"
[sqlite]
cache_size = "-16000"
"#;
        let config: ProjectConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.cache_size_pages(), -16000);
    }
}
