use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::Tree;

fn parse_go_import_spec(
    node: &tree_sitter::Node,
    source: &str,
) -> Option<crate::relationships::ImportInfo> {
    let mut module_path = String::new();
    let mut names = Vec::new();
    if let Some(name_node) = node.child_by_field_name("name") {
        if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
            names.push(name.trim().to_string());
        }
    }
    if let Some(path_node) = node.child_by_field_name("path") {
        if let Ok(path) = path_node.utf8_text(source.as_bytes()) {
            module_path = path.trim().trim_matches('"').to_string();
        }
    }
    if module_path.is_empty() {
        None
    } else {
        Some(crate::relationships::ImportInfo {
            source_file: String::new(),
            module_path,
            names,
        })
    }
}

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
                "function_declaration" | "method_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    let ct = if kind == "method_declaration" {
                        ChunkType::Method
                    } else {
                        ChunkType::Function
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "go",
                        ct,
                        name.as_deref(),
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "type_declaration" | "type_spec" => {
                    let node = if kind == "type_declaration" {
                        child
                    } else {
                        child.parent().unwrap_or(child)
                    };
                    let name = node
                        .child_by_field_name("name")
                        .or_else(|| {
                            node.children(&mut node.walk())
                                .find(|c| c.kind() == "type_identifier")
                        })
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let text = node
                        .utf8_text(source.as_bytes())
                        .ok()
                        .unwrap_or("")
                        .to_string();
                    let ct = if text.contains("struct") {
                        ChunkType::Struct
                    } else if text.contains("interface") {
                        ChunkType::Interface
                    } else {
                        ChunkType::Type
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        node,
                        file_path,
                        "go",
                        ct,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "import_declaration" | "import_spec_list" => {
                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "go",
                        ChunkType::Import,
                        None,
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                _ => {
                    let text = child.utf8_text(source.as_bytes()).ok().unwrap_or("");
                    let trimmed = text.trim();
                    if trimmed.starts_with("func ") || trimmed.starts_with("func\t") {
                        let name = child
                            .child_by_field_name("name")
                            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                            .map(|s| s.to_string());

                        let sig = trimmed
                            .lines()
                            .next()
                            .map(|l| l.to_string())
                            .filter(|l| l.len() <= 120);
                        if let Some(chunk) = make_chunk(
                            source,
                            child,
                            file_path,
                            "go",
                            ChunkType::Function,
                            name.as_deref(),
                            sig.as_deref(),
                            serde_json::Map::new(),
                        ) {
                            chunks.push(chunk);
                        }
                    }
                }
            }
        }
    }

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut imports = Vec::new();
        let mut seen_paths: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() != "import_declaration" {
                continue;
            }
            for gc in child.children(&mut child.walk()) {
                match gc.kind() {
                    "import_spec" => {
                        if let Some(info) = parse_go_import_spec(&gc, source) {
                            if seen_paths.insert(info.module_path.clone()) {
                                imports.push(info);
                            }
                        }
                    }
                    "import_path" => {
                        if let Ok(path) = gc.utf8_text(source.as_bytes()) {
                            let module_path = path.trim().trim_matches('"').to_string();
                            if !module_path.is_empty() && seen_paths.insert(module_path.clone()) {
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
                                if let Some(info) = parse_go_import_spec(&spec, source) {
                                    if seen_paths.insert(info.module_path.clone()) {
                                        imports.push(info);
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
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
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "go", chunks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_go_function() {
        let source = "package main\n\nimport \"fmt\"\n\nfunc greet(name string) string {\n    return \"Hello, \" + name\n}\n";
        let chunker = GoChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "main.go");

        assert!(!chunks.is_empty());
        let func = chunks.iter().find(|c| c.chunk_type == ChunkType::Function);
        assert!(func.is_some(), "Should find a function chunk");
        assert_eq!(func.unwrap().name.as_deref(), Some("greet"));
    }

    #[test]
    fn test_go_struct() {
        let source = "package models\n\ntype User struct {\n    Name string\n    Age  int\n}\n";
        let chunker = GoChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "user.go");

        let st = chunks
            .iter()
            .find(|c| matches!(c.chunk_type, ChunkType::Type | ChunkType::Struct));
        assert!(
            st.is_some(),
            "Should find a struct/type chunk, got: {:?}",
            chunks
                .iter()
                .map(|c| (&c.chunk_type, &c.name))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_go_extract_imports() {
        let source = "package main\n\nimport (\n    \"fmt\"\n    \"net/http\"\n\n    \"github.com/go-chi/chi/v5\"\n)\n";
        let chunker = GoChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "test.go");
        let imports = chunker.extract_imports(&tree.unwrap(), source);
        assert!(!imports.is_empty());
        assert!(imports.iter().any(|i| i.module_path == "fmt"));
        assert!(imports.iter().any(|i| i.module_path == "net/http"));
        assert!(imports
            .iter()
            .any(|i| i.module_path == "github.com/go-chi/chi/v5"));
    }
}
