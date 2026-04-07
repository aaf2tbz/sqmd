use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::Tree;

pub struct JavaChunker;

impl Default for JavaChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for JavaChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_java::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "java"
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
                "method_declaration" | "constructor_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    let ct = if kind == "constructor_declaration" {
                        ChunkType::Method
                    } else {
                        ChunkType::Function
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "java",
                        ct,
                        name.as_deref(),
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "class_declaration" | "interface_declaration" | "enum_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    let ct = match kind {
                        "class_declaration" => ChunkType::Class,
                        "interface_declaration" => ChunkType::Interface,
                        "enum_declaration" => ChunkType::Enum,
                        _ => ChunkType::Section,
                    };

                    let mut all_chunks: Vec<Option<Chunk>> = Vec::new();
                    all_chunks.push(make_chunk(
                        source,
                        child,
                        file_path,
                        "java",
                        ct,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ));

                    // Extract methods and fields from class body
                    if let Some(body) = child.child_by_field_name("body") {
                        let mut body_cursor = body.walk();
                        for member in body.children(&mut body_cursor) {
                            let mk = member.kind();
                            if mk == "method_declaration"
                                || mk == "constructor_declaration"
                                || mk == "field_declaration"
                            {
                                let mname = member
                                    .child_by_field_name("name")
                                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                                    .map(|s| s.to_string());

                                let mct = match mk {
                                    "method_declaration" | "constructor_declaration" => {
                                        ChunkType::Method
                                    }
                                    "field_declaration" => ChunkType::Constant,
                                    _ => ChunkType::Section,
                                };

                                all_chunks.push(make_chunk(
                                    source,
                                    member,
                                    file_path,
                                    "java",
                                    mct,
                                    mname.as_deref(),
                                    None,
                                    serde_json::Map::new(),
                                ));
                            }
                        }
                    }

                    chunks.extend(all_chunks.into_iter().flatten());
                }
                "import_declaration" => {
                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "java",
                        ChunkType::Import,
                        None,
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
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() != "import_declaration" {
                continue;
            }

            // Java imports look like: import com.example.ClassName;
            // or: import com.example.*
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            let text = text.trim();
            if let Some(path) = text
                .strip_prefix("import ")
                .and_then(|s| s.strip_suffix(';'))
            {
                let path = path.trim().to_string();
                let names = if path.ends_with(".*") {
                    vec!["*".to_string()]
                } else {
                    // Last segment is the class name
                    path.rsplit('.')
                        .next()
                        .map(|n| vec![n.to_string()])
                        .unwrap_or_default()
                };
                imports.push(crate::relationships::ImportInfo {
                    source_file: String::new(),
                    module_path: path,
                    names,
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
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "java", chunks);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_java_class() {
        let source = r#"package com.example;

import java.util.List;

public class UserService {
    private List<User> users;

    public User findById(Long id) {
        return users.stream().filter(u -> u.getId().equals(id)).findFirst().orElse(null);
    }
}
"#;
        let chunker = JavaChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "UserService.java");

        let cls = chunks.iter().find(|c| c.chunk_type == ChunkType::Class);
        assert!(cls.is_some(), "Should find a class");
        assert_eq!(cls.unwrap().name.as_deref(), Some("UserService"));

        let methods: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Method)
            .collect();
        assert!(!methods.is_empty(), "Should find methods");
    }

    #[test]
    fn test_java_extract_imports() {
        let source = "import java.util.List;\nimport java.util.Map;\n";
        let chunker = JavaChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "Test.java");
        let imports = chunker.extract_imports(&tree.unwrap(), source);
        assert_eq!(imports.len(), 2);
        assert_eq!(imports[0].module_path, "java.util.List");
        assert_eq!(imports[1].module_path, "java.util.Map");
    }
}
