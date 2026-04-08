use crate::chunk::{Chunk, ChunkType, SourceType};
use crate::chunker::LanguageChunker;
use tree_sitter::Tree;

pub struct JsonChunker;

impl Default for JsonChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl JsonChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for JsonChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_json::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "json"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        walk_json_node(tree.root_node(), source, file_path, 0, chunks);
    }

    fn chunk_unclaimed(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let _ = (tree, source, file_path, chunks);
    }

    fn extract_imports(
        &self,
        _tree: &Tree,
        _source: &str,
    ) -> Vec<crate::relationships::ImportInfo> {
        Vec::new()
    }
}

fn walk_json_node(
    node: tree_sitter::Node,
    source: &str,
    file_path: &str,
    depth: usize,
    chunks: &mut Vec<Chunk>,
) {
    if depth > 2 {
        return;
    }

    let kind = node.kind();

    if kind == "pair" && depth <= 2 {
        let key_text = extract_json_key(node, source);
        let text = node.utf8_text(source.as_bytes()).unwrap_or("");
        let line_count = text.lines().count();

        if !key_text.is_empty() && line_count > 0 {
            let first_line = text.lines().next().unwrap_or("").trim();
            let sig = if first_line.len() <= 120 {
                Some(first_line.to_string())
            } else {
                None
            };

            let importance = if depth == 0 {
                if line_count > 20 {
                    0.7
                } else {
                    0.5
                }
            } else if depth == 1 {
                0.4
            } else {
                0.3
            };

            let hash = crate::files::content_hash(text.as_bytes());
            chunks.push(Chunk {
                file_path: file_path.to_string(),
                language: "json".to_string(),
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
        walk_json_node(child, source, file_path, depth + 1, chunks);
    }
}

fn extract_json_key(pair_node: tree_sitter::Node, source: &str) -> String {
    let mut cursor = pair_node.walk();
    for child in pair_node.children(&mut cursor) {
        if child.kind() == "string" {
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            return text.trim_matches('"').to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_object() {
        let source = r#"{
  "database": {
    "host": "localhost",
    "port": 5432
  },
  "server": {
    "port": 8080
  }
}"#;
        let chunker = JsonChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "config.json");

        assert!(
            chunks.len() >= 2,
            "Should find at least 2 top-level pairs, got {}",
            chunks.len()
        );
        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"database"), "got: {:?}", names);
        assert!(names.contains(&"server"), "got: {:?}", names);
    }

    #[test]
    fn test_json_nested_key_content() {
        let source = r#"{
  "redis": {
    "host": "127.0.0.1",
    "port": 6379
  }
}"#;
        let chunker = JsonChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "redis.json");

        let redis = chunks.iter().find(|c| c.name.as_deref() == Some("redis"));
        assert!(redis.is_some(), "Should find redis key");
        assert!(
            redis.unwrap().content_raw.contains("6379"),
            "Content should contain port"
        );
    }

    #[test]
    fn test_json_nested_in_array() {
        let source = r#"{
  "services": [
    {"name": "api", "image": "myapp:latest"},
    {"name": "worker", "image": "myapp:latest"}
  ]
}"#;
        let chunker = JsonChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "docker-compose.json");

        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(
            names.iter().any(|n| *n == "services"),
            "Should find services key, got: {:?}",
            names
        );
    }
}
