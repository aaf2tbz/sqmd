use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::Tree;

pub struct CChunker;

impl Default for CChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for CChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_c::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "c"
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
                "function_definition" => {
                    let name = extract_declarator_name(&child, source);
                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "c",
                        ChunkType::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "struct_specifier" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let is_typedef = child
                        .parent()
                        .map(|p| p.kind() == "type_definition")
                        .unwrap_or(false);

                    let ct = if is_typedef {
                        ChunkType::Type
                    } else {
                        ChunkType::Struct
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "c",
                        ct,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "enum_specifier" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "c",
                        ChunkType::Enum,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "type_definition" => {
                    let has_struct = child
                        .children(&mut child.walk())
                        .any(|c| c.kind() == "struct_specifier");
                    let has_enum = child
                        .children(&mut child.walk())
                        .any(|c| c.kind() == "enum_specifier");
                    if has_struct || has_enum {
                        continue;
                    }
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "c",
                        ChunkType::Type,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "preproc_include" => {
                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "c",
                        ChunkType::Import,
                        None,
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "preproc_def" | "preproc_function_def" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let ct = if kind == "preproc_function_def" {
                        ChunkType::Macro
                    } else {
                        ChunkType::Constant
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "c",
                        ct,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "declaration" => {
                    let text = child.utf8_text(source.as_bytes()).ok().unwrap_or("");
                    if text.contains("typedef")
                        || text.contains("static")
                        || text.contains("extern")
                    {
                        let has_func = child
                            .children(&mut child.walk())
                            .any(|c| c.kind() == "function_declarator");
                        if has_func {
                            let name = extract_declarator_name(&child, source);
                            if let Some(chunk) = make_chunk(
                                source,
                                child,
                                file_path,
                                "c",
                                ChunkType::Function,
                                name.as_deref(),
                                None,
                                serde_json::Map::new(),
                            ) {
                                chunks.push(chunk);
                            }
                        }
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
            if child.kind() != "preproc_include" {
                continue;
            }
            if let Some(path_node) = child.child_by_field_name("path") {
                if let Ok(path) = path_node.utf8_text(source.as_bytes()) {
                    let p = path
                        .trim()
                        .trim_matches('"')
                        .trim_matches('<')
                        .trim_matches('>')
                        .to_string();
                    if !p.is_empty() && seen.insert(p.clone()) {
                        imports.push(crate::relationships::ImportInfo {
                            source_file: String::new(),
                            module_path: p,
                            names: Vec::new(),
                        });
                    }
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
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "c", chunks);
    }
}

fn extract_declarator_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "function_declarator" {
            if let Some(id) = child.child_by_field_name("declarator") {
                if let Ok(name) = id.utf8_text(source.as_bytes()) {
                    return Some(name.trim().to_string());
                }
            }
            for gc in child.children(&mut child.walk()) {
                if gc.kind() == "identifier" || gc.kind() == "field_identifier" {
                    if let Ok(name) = gc.utf8_text(source.as_bytes()) {
                        return Some(name.trim().to_string());
                    }
                }
            }
        }
        if child.kind() == "pointer_declarator" {
            if let Some(name) = extract_declarator_name(&child, source) {
                return Some(name);
            }
        }
    }
    node.child_by_field_name("declarator").and_then(|d| {
        if d.kind() == "identifier" || d.kind() == "field_identifier" {
            d.utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            extract_declarator_name(&d, source)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_c_function() {
        let source = "#include <stdio.h>\n\nvoid greet(const char *name) {\n    printf(\"Hello, %s\", name);\n}\n";
        let chunker = CChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "main.c");

        let func = chunks.iter().find(|c| c.chunk_type == ChunkType::Function);
        assert!(
            func.is_some(),
            "Should find a function chunk, got: {:?}",
            chunks
                .iter()
                .map(|c| (&c.chunk_type, &c.name))
                .collect::<Vec<_>>()
        );
        assert_eq!(func.unwrap().name.as_deref(), Some("greet"));
    }

    #[test]
    fn test_c_struct() {
        let source = "struct Point {\n    int x;\n    int y;\n};\n";
        let chunker = CChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "point.c");

        let st = chunks.iter().find(|c| c.chunk_type == ChunkType::Struct);
        assert!(st.is_some(), "Should find a struct chunk");
        assert_eq!(st.unwrap().name.as_deref(), Some("Point"));
    }

    #[test]
    fn test_c_includes() {
        let source = "#include <stdio.h>\n#include \"mylib.h\"\n";
        let chunker = CChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "test.c");
        let imports = chunker.extract_imports(&tree.unwrap(), source);
        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.module_path == "stdio.h"));
        assert!(imports.iter().any(|i| i.module_path == "mylib.h"));
    }

    #[test]
    fn test_c_enum() {
        let source = "enum Color { RED, GREEN, BLUE };\n";
        let chunker = CChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "color.c");

        let en = chunks.iter().find(|c| c.chunk_type == ChunkType::Enum);
        assert!(en.is_some(), "Should find an enum chunk");
        assert_eq!(en.unwrap().name.as_deref(), Some("Color"));
    }
}
