use crate::chunk::{Chunk, ChunkType, SourceType};
use crate::chunker::LanguageChunker;
use tree_sitter::Tree;

pub struct TomlChunker;

impl Default for TomlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl TomlChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for TomlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_toml_ng::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "toml"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        walk_toml_node(tree.root_node(), source, file_path, 0, chunks);
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

fn walk_toml_node(
    node: tree_sitter::Node,
    source: &str,
    file_path: &str,
    depth: usize,
    chunks: &mut Vec<Chunk>,
) {
    let kind = node.kind();

    match kind {
        "table" | "table_array_element" if depth <= 1 => {
            let header = extract_toml_table_header(node, source);
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            if !text.trim().is_empty() {
                let first_line = text.lines().next().unwrap_or("").trim();
                let sig = if first_line.len() <= 120 {
                    Some(first_line.to_string())
                } else {
                    None
                };

                let hash = crate::files::content_hash(text.as_bytes());
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "toml".to_string(),
                    chunk_type: ChunkType::Module,
                    source_type: SourceType::Code,
                    name: Some(header),
                    signature: sig,
                    line_start: node.start_position().row,
                    line_end: node.end_position().row,
                    content_raw: text.to_string(),
                    content_hash: hash,
                    importance: 0.6,
                    metadata: serde_json::Map::new(),
                    agent_id: None,
                    tags: None,
                    decay_rate: 0.0,
                    created_by: None,
                });
            }
        }
        "pair" if depth <= 2 => {
            let key = extract_toml_key(node, source);
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            if !key.is_empty() && !text.trim().is_empty() {
                let first_line = text.lines().next().unwrap_or("").trim();
                let sig = if first_line.len() <= 120 {
                    Some(first_line.to_string())
                } else {
                    None
                };

                let hash = crate::files::content_hash(text.as_bytes());
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "toml".to_string(),
                    chunk_type: ChunkType::Constant,
                    source_type: SourceType::Code,
                    name: Some(key),
                    signature: sig,
                    line_start: node.start_position().row,
                    line_end: node.end_position().row,
                    content_raw: text.to_string(),
                    content_hash: hash,
                    importance: 0.4,
                    metadata: serde_json::Map::new(),
                    agent_id: None,
                    tags: None,
                    decay_rate: 0.0,
                    created_by: None,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_toml_node(child, source, file_path, depth + 1, chunks);
    }
}

fn extract_toml_table_header(table_node: tree_sitter::Node, source: &str) -> String {
    let mut cursor = table_node.walk();
    for child in table_node.children(&mut cursor) {
        if child.kind() == "table_header" || child.kind() == "array_table_header" {
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            return text.trim().to_string();
        }
    }
    let text = table_node.utf8_text(source.as_bytes()).unwrap_or("");
    text.lines().next().unwrap_or("").trim().to_string()
}

fn extract_toml_key(pair_node: tree_sitter::Node, source: &str) -> String {
    let mut cursor = pair_node.walk();
    for child in pair_node.children(&mut cursor) {
        if child.kind() == "bare_key" || child.kind() == "quoted_key" {
            return child
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .trim_matches('"')
                .to_string();
        }
        if child.kind() == "dotted_key" {
            return child
                .utf8_text(source.as_bytes())
                .unwrap_or("")
                .trim()
                .to_string();
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_toml_table() {
        let source =
            "[package]\nname = \"myapp\"\nversion = \"1.0.0\"\n\n[dependencies]\nserde = \"1.0\"\n";
        let chunker = TomlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "Cargo.toml");

        let tables: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Module)
            .collect();
        assert!(
            tables.len() >= 2,
            "Should find 2 tables, got {}",
            tables.len()
        );
    }

    #[test]
    fn test_toml_key_value() {
        let source = "[package]\nname = \"myapp\"\nversion = \"1.0.0\"\nedition = \"2021\"\n";
        let chunker = TomlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "Cargo.toml");

        let pairs: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Constant)
            .collect();
        assert!(
            pairs.len() >= 3,
            "Should find key-value pairs, got {}",
            pairs.len()
        );
        let names: Vec<_> = pairs.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"name"), "got: {:?}", names);
        assert!(names.contains(&"version"), "got: {:?}", names);
    }

    #[test]
    fn test_toml_array_table() {
        let source = "[[bin]]\nname = \"myapp\"\npath = \"src/main.rs\"\n\n[[bin]]\nname = \"mytool\"\npath = \"src/tool.rs\"\n";
        let chunker = TomlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "Cargo.toml");

        let array_tables: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Module)
            .collect();
        assert!(
            array_tables.len() >= 2,
            "Should find array table elements, got {}",
            array_tables.len()
        );
    }
}
