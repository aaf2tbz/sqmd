use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::Tree;

pub struct QmlChunker;

impl Default for QmlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl QmlChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for QmlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_qmljs::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "qml"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            match kind {
                "ui_object_definition" => {
                    let name = extract_qml_object_name(&child, source);
                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "qml",
                        ChunkType::Class,
                        name.as_deref(),
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "ui_import" | "import_statement" => {
                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "qml",
                        ChunkType::Import,
                        None,
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "function_declaration" | "function_expression" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "qml",
                        ChunkType::Function,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "property" | "property_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "qml",
                        ChunkType::Constant,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut imports = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() != "ui_import" && child.kind() != "import_statement" {
                continue;
            }
            let text = child
                .utf8_text(source.as_bytes())
                .ok()
                .unwrap_or("")
                .trim()
                .to_string();
            let module_path = text
                .trim_start_matches("import")
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string();
            if !module_path.is_empty() && seen.insert(module_path.clone()) {
                imports.push(crate::relationships::ImportInfo {
                    source_file: String::new(),
                    module_path,
                    names: Vec::new(),
                });
            }
        }

        imports
    }

    fn chunk_unclaimed(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "qml", chunks);
    }
}

fn extract_qml_object_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "qualified_identifier" {
            if let Ok(name) = child.utf8_text(source.as_bytes()) {
                return Some(name.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_qml_component() {
        let source = "import QtQuick 2.15\n\nRectangle {\n    width: 200\n    height: 200\n    color: \"red\"\n}\n";
        let chunker = QmlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "main.qml");

        let obj = chunks.iter().find(|c| c.chunk_type == ChunkType::Class);
        assert!(
            obj.is_some(),
            "Should find a component chunk, got: {:?}",
            chunks
                .iter()
                .map(|c| (&c.chunk_type, &c.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_qml_imports() {
        let source = "import QtQuick 2.15\nimport QtQuick.Controls 2.15\n\nItem {\n}\n";
        let chunker = QmlChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "test.qml");
        let imports = chunker.extract_imports(&tree.unwrap(), source);
        assert!(!imports.is_empty(), "Should find at least 1 import");
    }
}
