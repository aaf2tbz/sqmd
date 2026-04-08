use crate::chunk::{Chunk, ChunkType, SourceType};
use crate::chunker::LanguageChunker;
use tree_sitter::Tree;

pub struct YamlChunker;

impl Default for YamlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl YamlChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for YamlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_yaml::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "yaml"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        walk_yaml_node(tree.root_node(), source, file_path, 0, chunks);
    }

    fn chunk_unclaimed(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let _ = tree;
        let mut claimed_ranges: Vec<(usize, usize)> =
            chunks.iter().map(|c| (c.line_start, c.line_end)).collect();
        claimed_ranges.sort();

        let source_lines: Vec<&str> = source.lines().collect();
        let total_lines = source_lines.len();
        let max_gap = 50;

        let mut gap_start = 0;
        for (start, end) in &claimed_ranges {
            let gap_size = start.saturating_sub(gap_start);
            if gap_size > 0 && *end > gap_start {
                let effective_start = gap_start;
                let effective_end = std::cmp::min(*start, gap_start + max_gap);
                if effective_end > effective_start {
                    let text: String = source_lines[effective_start..effective_end].join("\n");
                    if !text.trim().is_empty() {
                        let hash = crate::files::content_hash(text.as_bytes());
                        chunks.push(Chunk {
                            file_path: file_path.to_string(),
                            language: "yaml".to_string(),
                            chunk_type: ChunkType::Section,
                            source_type: SourceType::Code,
                            name: None,
                            signature: None,
                            line_start: effective_start,
                            line_end: effective_end,
                            content_raw: text,
                            content_hash: hash,
                            importance: ChunkType::Section.importance(),
                            metadata: serde_json::Map::new(),
                            agent_id: None,
                            tags: None,
                            decay_rate: 0.0,
                            created_by: None,
                        });
                    }
                }
            }
            gap_start = end + 1;
        }

        if gap_start < total_lines {
            let effective_end = std::cmp::min(total_lines, gap_start + max_gap);
            let text: String = source_lines[gap_start..effective_end].join("\n");
            if !text.trim().is_empty() {
                let hash = crate::files::content_hash(text.as_bytes());
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "yaml".to_string(),
                    chunk_type: ChunkType::Section,
                    source_type: SourceType::Code,
                    name: None,
                    signature: None,
                    line_start: gap_start,
                    line_end: effective_end,
                    content_raw: text,
                    content_hash: hash,
                    importance: ChunkType::Section.importance(),
                    metadata: serde_json::Map::new(),
                    agent_id: None,
                    tags: None,
                    decay_rate: 0.0,
                    created_by: None,
                });
            }
        }
    }

    fn extract_imports(
        &self,
        _tree: &Tree,
        _source: &str,
    ) -> Vec<crate::relationships::ImportInfo> {
        Vec::new()
    }
}

fn walk_yaml_node(
    node: tree_sitter::Node,
    source: &str,
    file_path: &str,
    mapping_depth: usize,
    chunks: &mut Vec<Chunk>,
) {
    let kind = node.kind();
    let is_pair = kind == "block_mapping_pair" || kind == "flow_mapping_pair";

    if is_pair && mapping_depth <= 2 {
        let key_text = extract_yaml_key(node, source);

        if !key_text.is_empty() {
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            let first_line = text.lines().next().unwrap_or("").trim();
            let sig = if first_line.len() <= 120 {
                Some(first_line.to_string())
            } else {
                None
            };

            let line_count = text.lines().count();

            let importance = if mapping_depth == 0 {
                if line_count > 20 {
                    0.7
                } else {
                    0.5
                }
            } else if mapping_depth == 1 {
                0.4
            } else {
                0.3
            };

            let hash = crate::files::content_hash(text.as_bytes());
            chunks.push(Chunk {
                file_path: file_path.to_string(),
                language: "yaml".to_string(),
                chunk_type: ChunkType::Constant,
                source_type: SourceType::Code,
                name: Some(key_text),
                signature: sig,
                line_start: node.start_position().row,
                line_end: node.end_position().row,
                content_raw: text.to_string(),
                content_hash: hash,
                importance,
                metadata: serde_json::Map::new(),
                agent_id: None,
                tags: None,
                decay_rate: 0.0,
                created_by: None,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let child_is_mapping = child.kind() == "block_mapping" || child.kind() == "flow_mapping";
        let new_depth = if child_is_mapping && is_pair {
            mapping_depth + 1
        } else {
            mapping_depth
        };

        walk_yaml_node(child, source, file_path, new_depth, chunks);
    }
}

fn extract_yaml_key(pair_node: tree_sitter::Node, source: &str) -> String {
    let mut cursor = pair_node.walk();
    for child in pair_node.children(&mut cursor) {
        if child.kind() == "flow_node" || child.kind() == "block_node" {
            let mut inner = child.walk();
            for inner_child in child.children(&mut inner) {
                if inner_child.kind() == "plain_scalar"
                    || inner_child.kind() == "double_quote_scalar"
                    || inner_child.kind() == "single_quote_scalar"
                {
                    let mut leaf = inner_child.walk();
                    for leaf_child in inner_child.children(&mut leaf) {
                        if leaf_child.kind() == "string_scalar"
                            || leaf_child.kind() == "string_content"
                        {
                            return leaf_child
                                .utf8_text(source.as_bytes())
                                .unwrap_or("")
                                .trim()
                                .to_string();
                        }
                    }
                    let direct = inner_child.utf8_text(source.as_bytes()).unwrap_or("");
                    return direct.trim_matches('"').trim_matches('\'').to_string();
                }
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_yaml_basic_mapping() {
        let source = r#"
database:
  host: localhost
  port: 5432
  name: myapp

server:
  port: 8080
  workers: 4
"#;
        let chunker = YamlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "config.yaml");

        assert!(
            chunks.len() >= 2,
            "Should find at least 2 top-level mappings, got {}",
            chunks.len()
        );
        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"database"), "got: {:?}", names);
        assert!(names.contains(&"server"), "got: {:?}", names);
    }

    #[test]
    fn test_yaml_nested() {
        let source = r#"
redis:
  host: 127.0.0.1
  port: 6379
  tls:
    cert: /path/to/cert.pem
    key: /path/to/key.pem
"#;
        let chunker = YamlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "redis.yaml");

        let redis = chunks.iter().find(|c| c.name.as_deref() == Some("redis"));
        assert!(redis.is_some(), "Should find redis key");
        let redis_chunk = redis.unwrap();
        assert!(
            redis_chunk.content_raw.contains("6379"),
            "Content should contain port"
        );
    }

    #[test]
    fn test_yaml_document() {
        let source = "---\nversion: 1.0\nservices:\n  - name: api\n    image: myapp:latest\n";
        let chunker = YamlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "docker-compose.yaml");

        assert!(!chunks.is_empty(), "Should find chunks in a document");
    }
}
