use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginManifest {
    #[serde(default)]
    pub plugin: Vec<PluginConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub plugin_type: PluginType,
    pub command: Vec<String>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub priority: i32,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    10
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PluginType {
    #[serde(rename = "chunker")]
    Chunker,
    #[serde(rename = "search-layer")]
    SearchLayer,
    #[serde(rename = "post-index")]
    PostIndex,
    #[serde(rename = "pre-search")]
    PreSearch,
}

impl std::fmt::Display for PluginType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginType::Chunker => write!(f, "chunker"),
            PluginType::SearchLayer => write!(f, "search-layer"),
            PluginType::PostIndex => write!(f, "post-index"),
            PluginType::PreSearch => write!(f, "pre-search"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkerRequest {
    pub file_path: String,
    pub content: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkerResponse {
    pub chunks: Vec<PluginChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginChunk {
    pub name: String,
    pub chunk_type: String,
    pub line_start: usize,
    pub line_end: usize,
    pub content_raw: String,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub imports: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLayerRequest {
    pub query: String,
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResultItem {
    pub chunk_id: i64,
    pub file_path: String,
    pub name: Option<String>,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchLayerResponse {
    pub results: Vec<SearchResultItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreSearchRequest {
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreSearchResponse {
    pub query: String,
}

impl PluginManifest {
    pub fn load(project_root: &Path) -> Self {
        let manifest_path = project_root.join(".sqmd").join("plugins.toml");
        if !manifest_path.exists() {
            return Self::default();
        }
        match std::fs::read_to_string(&manifest_path) {
            Ok(content) => match toml::from_str(&content) {
                Ok(manifest) => manifest,
                Err(e) => {
                    eprintln!("[plugin] failed to parse {:?}: {}", manifest_path, e);
                    Self::default()
                }
            },
            Err(e) => {
                eprintln!("[plugin] failed to read {:?}: {}", manifest_path, e);
                Self::default()
            }
        }
    }

    pub fn find_chunker(&self, extension: &str, language: &str) -> Option<&PluginConfig> {
        let mut matches: Vec<&PluginConfig> = self
            .plugin
            .iter()
            .filter(|p| {
                p.plugin_type == PluginType::Chunker
                    && (p.extensions.iter().any(|e| extension.ends_with(e.as_str()))
                        || p.languages.iter().any(|l| l.eq_ignore_ascii_case(language)))
            })
            .collect();
        matches.sort_by_key(|p| -p.priority);
        matches.into_iter().next()
    }

    pub fn search_layer_plugins(&self) -> Vec<&PluginConfig> {
        let mut plugins: Vec<&PluginConfig> = self
            .plugin
            .iter()
            .filter(|p| p.plugin_type == PluginType::SearchLayer)
            .collect();
        plugins.sort_by_key(|p| -p.priority);
        plugins
    }

    pub fn pre_search_plugins(&self) -> Vec<&PluginConfig> {
        let mut plugins: Vec<&PluginConfig> = self
            .plugin
            .iter()
            .filter(|p| p.plugin_type == PluginType::PreSearch)
            .collect();
        plugins.sort_by_key(|p| p.priority);
        plugins
    }

    pub fn post_index_plugins(&self) -> Vec<&PluginConfig> {
        let mut plugins: Vec<&PluginConfig> = self
            .plugin
            .iter()
            .filter(|p| p.plugin_type == PluginType::PostIndex)
            .collect();
        plugins.sort_by_key(|p| p.priority);
        plugins
    }
}

pub fn invoke_chunker(
    plugin: &PluginConfig,
    request: &ChunkerRequest,
) -> Result<ChunkerResponse, PluginError> {
    let input =
        serde_json::to_string(request).map_err(|e| PluginError::Serialization(e.to_string()))?;
    let output = run_plugin(plugin, &input)?;
    serde_json::from_str(&output).map_err(|e| PluginError::Deserialization(e.to_string()))
}

pub fn invoke_search_layer(
    plugin: &PluginConfig,
    request: &SearchLayerRequest,
) -> Result<SearchLayerResponse, PluginError> {
    let input =
        serde_json::to_string(request).map_err(|e| PluginError::Serialization(e.to_string()))?;
    let output = run_plugin(plugin, &input)?;
    serde_json::from_str(&output).map_err(|e| PluginError::Deserialization(e.to_string()))
}

pub fn invoke_pre_search(
    plugin: &PluginConfig,
    request: &PreSearchRequest,
) -> Result<PreSearchResponse, PluginError> {
    let input =
        serde_json::to_string(request).map_err(|e| PluginError::Serialization(e.to_string()))?;
    let output = run_plugin(plugin, &input)?;
    serde_json::from_str(&output).map_err(|e| PluginError::Deserialization(e.to_string()))
}

pub fn run_post_index(plugin: &PluginConfig, file_path: &str) -> Result<(), PluginError> {
    let input = serde_json::json!({"file_path": file_path}).to_string();
    run_plugin(plugin, &input)?;
    Ok(())
}

#[derive(Debug)]
pub enum PluginError {
    Execution(String),
    Timeout(String),
    Serialization(String),
    Deserialization(String),
    NotFound(String),
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginError::Execution(s) => write!(f, "plugin execution failed: {}", s),
            PluginError::Timeout(s) => write!(f, "plugin timed out: {}", s),
            PluginError::Serialization(s) => write!(f, "plugin input serialization failed: {}", s),
            PluginError::Deserialization(s) => write!(f, "plugin output parse failed: {}", s),
            PluginError::NotFound(s) => write!(f, "plugin not found: {}", s),
        }
    }
}

impl std::error::Error for PluginError {}

fn run_plugin(plugin: &PluginConfig, input: &str) -> Result<String, PluginError> {
    if plugin.command.is_empty() {
        return Err(PluginError::NotFound("empty command".to_string()));
    }

    let mut child = Command::new(&plugin.command[0])
        .args(&plugin.command[1..])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PluginError::Execution(e.to_string()))?;

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }

    let timeout = Duration::from_secs(plugin.timeout_secs);
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                if let Some(mut out) = child.stdout.take() {
                    let _ = out.read_to_string(&mut stdout);
                }
                let mut stderr = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut stderr);
                }
                if !status.success() {
                    if !stderr.is_empty() {
                        eprintln!("[plugin:{}] stderr: {}", plugin.name, stderr);
                    }
                    return Err(PluginError::Execution(format!(
                        "exit code {}",
                        status.code().unwrap_or(-1)
                    )));
                }
                return Ok(stdout);
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(PluginError::Timeout(format!(
                        "{} timed out after {}s",
                        plugin.name, plugin.timeout_secs
                    )));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(PluginError::Execution(e.to_string()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_load_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = PluginManifest::load(tmp.path());
        assert!(manifest.plugin.is_empty());
    }

    #[test]
    fn manifest_parse_valid() {
        let toml_str = r#"
[[plugin]]
name = "sql-parser"
type = "chunker"
command = ["node", "/path/to/sql.mjs"]
extensions = [".sql", ".pgsql"]
priority = 10

[[plugin]]
name = "redactor"
type = "post-index"
command = ["python3", "/path/to/redact.py"]
"#;
        let manifest: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(manifest.plugin.len(), 2);
        assert_eq!(manifest.plugin[0].name, "sql-parser");
        assert_eq!(manifest.plugin[0].plugin_type, PluginType::Chunker);
        assert_eq!(manifest.plugin[0].extensions, vec![".sql", ".pgsql"]);
        assert_eq!(manifest.plugin[1].plugin_type, PluginType::PostIndex);
    }

    #[test]
    fn find_chunker_by_extension() {
        let manifest = PluginManifest {
            plugin: vec![
                PluginConfig {
                    name: "sql".to_string(),
                    plugin_type: PluginType::Chunker,
                    command: vec!["cat".to_string()],
                    extensions: vec![".sql".to_string()],
                    languages: vec![],
                    priority: 5,
                    timeout_secs: 10,
                },
                PluginConfig {
                    name: "jsx".to_string(),
                    plugin_type: PluginType::Chunker,
                    command: vec!["cat".to_string()],
                    extensions: vec![".jsx".to_string()],
                    languages: vec![],
                    priority: 10,
                    timeout_secs: 10,
                },
            ],
        };
        let found = manifest.find_chunker(".sql", "SQL");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "sql");
        assert!(manifest.find_chunker(".rs", "rust").is_none());
    }

    #[test]
    fn plugin_type_display() {
        assert_eq!(PluginType::Chunker.to_string(), "chunker");
        assert_eq!(PluginType::SearchLayer.to_string(), "search-layer");
        assert_eq!(PluginType::PostIndex.to_string(), "post-index");
        assert_eq!(PluginType::PreSearch.to_string(), "pre-search");
    }

    #[test]
    fn run_plugin_empty_command() {
        let plugin = PluginConfig {
            name: "empty".to_string(),
            plugin_type: PluginType::Chunker,
            command: vec![],
            extensions: vec![],
            languages: vec![],
            priority: 0,
            timeout_secs: 10,
        };
        let result = run_plugin(&plugin, "{}");
        assert!(result.is_err());
    }
}
