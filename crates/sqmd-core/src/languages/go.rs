use tree_sitter::Tree;
use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{LanguageChunker, make_chunk};

pub struct GoChunker;

impl Default for GoChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl GoChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for GoChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_go::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "go"
    }

    fn walk_declarations(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            match kind {
                "function_declaration" | "method_declaration" => {
                    let name = child.child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let sig = child.utf8_text(source.as_bytes()).ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    let ct = if kind == "method_declaration" {
                        ChunkType::Method
                    } else {
                        ChunkType::Function
                    };

                    if let Some(chunk) = make_chunk(source, child, file_path, "go", ct, name.as_deref(), sig.as_deref(), serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                }
                "type_declaration" | "type_spec" => {
                    let node = if kind == "type_declaration" { child } else { child.parent().unwrap_or(child) };
                    let name = node.child_by_field_name("name")
                        .or_else(|| {
                            // fallback: try to find identifier in first child
                            node.children(&mut node.walk())
                                .find(|c| c.kind() == "type_identifier")
                        })
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let ct = ChunkType::Type;

                    if let Some(chunk) = make_chunk(source, node, file_path, "go", ct, name.as_deref(), None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                }
                "import_declaration" | "import_spec" | "import_spec_list" => {
                    if let Some(chunk) = make_chunk(source, child, file_path, "go", ChunkType::Import, None, None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                }
                _ => {
                    // Try to detect any top-level function/method patterns missed above
                    let text = child.utf8_text(source.as_bytes()).ok().unwrap_or("");
                    let trimmed = text.trim();
                    if trimmed.starts_with("func ") || trimmed.starts_with("func\t") {
                        let name = child.child_by_field_name("name")
                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                            .map(|s| s.to_string());

                        let sig = trimmed.lines().next().map(|l| l.to_string()).filter(|l| l.len() <= 120);
                        if let Some(chunk) = make_chunk(source, child, file_path, "go", ChunkType::Function, name.as_deref(), sig.as_deref(), serde_json::Map::new()) {
                            chunks.push(chunk);
                        }
                    }
                }
            }
        }
    }

    fn extract_imports(&self, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut parser = tree_sitter::Parser::new();
        if parser.set_language(&self.language()).is_err() {
            return Vec::new();
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return Vec::new(),
        };

        let mut imports = Vec::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            match child.kind() {
                "import_declaration" => {
                    for gc in child.children(&mut child.walk()) {
                        match gc.kind() {
                            "import_spec" => {
                                let mut module_path = String::new();
                                let mut names = Vec::new();
                                if let Some(name_node) = gc.child_by_field_name("name") {
                                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                                        names.push(name.trim().to_string());
                                    }
                                }
                                if let Some(path_node) = gc.child_by_field_name("path") {
                                    if let Ok(path) = path_node.utf8_text(source.as_bytes()) {
                                        module_path = path.trim().trim_matches('"').to_string();
                                    }
                                }
                                if !module_path.is_empty() {
                                    imports.push(crate::relationships::ImportInfo {
                                        source_file: String::new(),
                                        module_path,
                                        names,
                                    });
                                }
                            }
                            "import_path" => {
                                if let Ok(path) = gc.utf8_text(source.as_bytes()) {
                                    let module_path = path.trim().trim_matches('"').to_string();
                                    if !module_path.is_empty() {
                                        imports.push(crate::relationships::ImportInfo {
                                            source_file: String::new(),
                                            module_path,
                                            names: Vec::new(),
                                        });
                                    }
                                }
                            }
                            "import_spec_list" => {
                                for spec in gc.children(&mut gc.walk()) {
                                    if spec.kind() == "import_spec" {
                                        let mut module_path = String::new();
                                        let mut names = Vec::new();
                                        if let Some(name_node) = spec.child_by_field_name("name") {
                                            if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                                                names.push(name.trim().to_string());
                                            }
                                        }
                                        if let Some(path_node) = spec.child_by_field_name("path") {
                                            if let Ok(path) = path_node.utf8_text(source.as_bytes()) {
                                                module_path = path.trim().trim_matches('"').to_string();
                                            }
                                        }
                                        if !module_path.is_empty() {
                                            imports.push(crate::relationships::ImportInfo {
                                                source_file: String::new(),
                                                module_path,
                                                names,
                                            });
                                        }
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
                "import_spec" => {
                    let mut module_path = String::new();
                    let mut names = Vec::new();
                    if let Some(name_node) = child.child_by_field_name("name") {
                        if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                            names.push(name.trim().to_string());
                        }
                    }
                    if let Some(path_node) = child.child_by_field_name("path") {
                        if let Ok(path) = path_node.utf8_text(source.as_bytes()) {
                            module_path = path.trim().trim_matches('"').to_string();
                        }
                    }
                    if !module_path.is_empty() {
                        imports.push(crate::relationships::ImportInfo {
                            source_file: String::new(),
                            module_path,
                            names,
                        });
                    }
                }
                _ => continue,
            }
        }

        imports
    }

    fn chunk_unclaimed(&self, _tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "go", chunks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_function() {
        let source = r#"package main

import "fmt"

func greet(name string) string {
    return "Hello, " + name
}
"#;
        let chunker = GoChunker::new();
        let chunks = chunker.chunk(source, "main.go");

        assert!(!chunks.is_empty());
        let func = chunks.iter().find(|c| c.chunk_type == ChunkType::Function);
        assert!(func.is_some(), "Should find a function chunk");
        assert_eq!(func.unwrap().name.as_deref(), Some("greet"));
    }

    #[test]
    fn test_go_struct() {
        let source = r#"package models

type User struct {
    Name string
    Age  int
}
"#;
        let chunker = GoChunker::new();
        let chunks = chunker.chunk(source, "user.go");

        let st = chunks.iter().find(|c| matches!(c.chunk_type, ChunkType::Type | ChunkType::Struct));
        assert!(st.is_some(), "Should find a struct/type chunk, got: {:?}", chunks.iter().map(|c| (&c.chunk_type, &c.name)).collect::<Vec<_>>());
    }

    #[test]
    fn test_go_extract_imports() {
        let source = r#"package main

import (
    "fmt"
    "net/http"

    "github.com/go-chi/chi/v5"
)
"#;
        let chunker = GoChunker::new();
        let imports = chunker.extract_imports(source);
        assert!(!imports.is_empty());
        assert!(imports.iter().any(|i| i.module_path == "fmt"));
        assert!(imports.iter().any(|i| i.module_path == "net/http"));
        assert!(imports.iter().any(|i| i.module_path == "github.com/go-chi/chi/v5"));
    }
}
