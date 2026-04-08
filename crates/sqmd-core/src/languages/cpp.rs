use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::Tree;

pub struct CppChunker;

impl Default for CppChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CppChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for CppChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_cpp::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "cpp"
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
                    let name = extract_cpp_declarator_name(&child, source);
                    let sig = child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .and_then(|t| t.lines().next().map(|l| l.trim().to_string()))
                        .filter(|l| l.len() <= 120);

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
                        ChunkType::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "class_specifier" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
                        ChunkType::Class,
                        name.as_deref(),
                        None,
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

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
                        ChunkType::Struct,
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
                        "cpp",
                        ChunkType::Enum,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "namespace_definition" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
                        ChunkType::Module,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "template_declaration" => {
                    self.walk_template(child, source, file_path, chunks);
                }
                "preproc_include" => {
                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
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
                        "cpp",
                        ct,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "type_definition" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
                        ChunkType::Type,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "alias_declaration" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "cpp",
                        ChunkType::Type,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                }
                "declaration" => {
                    let text = child.utf8_text(source.as_bytes()).ok().unwrap_or("");
                    let has_func = child
                        .children(&mut child.walk())
                        .any(|c| c.kind() == "function_declarator");
                    if has_func {
                        let name = extract_cpp_declarator_name(&child, source);
                        if let Some(chunk) = make_chunk(
                            source,
                            child,
                            file_path,
                            "cpp",
                            ChunkType::Function,
                            name.as_deref(),
                            None,
                            serde_json::Map::new(),
                        ) {
                            chunks.push(chunk);
                        }
                    } else if text.contains("constexpr") || text.contains("const ") {
                        if let Some(chunk) = make_chunk(
                            source,
                            child,
                            file_path,
                            "cpp",
                            ChunkType::Constant,
                            None,
                            None,
                            serde_json::Map::new(),
                        ) {
                            chunks.push(chunk);
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
        crate::chunker::FileChunker::chunk_file_into(source, file_path, "cpp", chunks);
    }
}

impl CppChunker {
    fn walk_template(
        &self,
        node: tree_sitter::Node,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "function_definition" => {
                    let name = extract_cpp_declarator_name(&child, source);
                    if let Some(chunk) = make_chunk(
                        source,
                        node,
                        file_path,
                        "cpp",
                        ChunkType::Function,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                    return;
                }
                "class_specifier" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());
                    if let Some(chunk) = make_chunk(
                        source,
                        node,
                        file_path,
                        "cpp",
                        ChunkType::Class,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                    return;
                }
                "struct_specifier" => {
                    let name = child
                        .child_by_field_name("name")
                        .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                        .map(|s| s.to_string());
                    if let Some(chunk) = make_chunk(
                        source,
                        node,
                        file_path,
                        "cpp",
                        ChunkType::Struct,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }
                    return;
                }
                "declaration" => {
                    let has_func = child
                        .children(&mut child.walk())
                        .any(|c| c.kind() == "function_declarator");
                    if has_func {
                        let name = extract_cpp_declarator_name(&child, source);
                        if let Some(chunk) = make_chunk(
                            source,
                            node,
                            file_path,
                            "cpp",
                            ChunkType::Function,
                            name.as_deref(),
                            None,
                            serde_json::Map::new(),
                        ) {
                            chunks.push(chunk);
                        }
                        return;
                    }
                }
                "template_declaration" => {
                    self.walk_template(child, source, file_path, chunks);
                    return;
                }
                _ => {}
            }
        }
    }
}

fn extract_cpp_declarator_name(node: &tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_declarator" | "structured_binding_declarator" => {
                if let Some(id) = child.child_by_field_name("declarator") {
                    if let Ok(name) = id.utf8_text(source.as_bytes()) {
                        let trimmed = name.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
                for gc in child.children(&mut child.walk()) {
                    if gc.kind() == "identifier"
                        || gc.kind() == "field_identifier"
                        || gc.kind() == "destructor_name"
                        || gc.kind() == "qualified_identifier"
                    {
                        if let Ok(name) = gc.utf8_text(source.as_bytes()) {
                            return Some(name.trim().to_string());
                        }
                    }
                }
            }
            "pointer_declarator" | "reference_declarator" => {
                if let Some(name) = extract_cpp_declarator_name(&child, source) {
                    return Some(name);
                }
            }
            _ => {}
        }
    }
    node.child_by_field_name("declarator").and_then(|d| {
        if d.kind() == "identifier" || d.kind() == "field_identifier" {
            d.utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.trim().to_string())
        } else {
            extract_cpp_declarator_name(&d, source)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cpp_class() {
        let source = "class Engine {\npublic:\n    void start();\n    void stop();\n};\n";
        let chunker = CppChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "engine.cpp");

        let cls = chunks.iter().find(|c| c.chunk_type == ChunkType::Class);
        assert!(
            cls.is_some(),
            "Should find a class chunk, got: {:?}",
            chunks
                .iter()
                .map(|c| (&c.chunk_type, &c.name))
                .collect::<Vec<_>>()
        );
        assert_eq!(cls.unwrap().name.as_deref(), Some("Engine"));
    }

    #[test]
    fn test_cpp_namespace() {
        let source = "namespace zrythm::engine {\n\nclass Processor {\n};\n\n}\n";
        let chunker = CppChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "proc.cpp");

        let ns = chunks.iter().find(|c| c.chunk_type == ChunkType::Module);
        assert!(ns.is_some(), "Should find a namespace chunk");
        assert_eq!(ns.unwrap().name.as_deref(), Some("zrythm::engine"));
    }

    #[test]
    fn test_cpp_template() {
        let source = "template <typename T>\nT max(T a, T b) {\n    return a > b ? a : b;\n}\n";
        let chunker = CppChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "utils.cpp");

        let func = chunks.iter().find(|c| c.chunk_type == ChunkType::Function);
        assert!(func.is_some(), "Should find a template function chunk");
    }

    #[test]
    fn test_cpp_includes() {
        let source = "#include <vector>\n#include \"engine.h\"\n";
        let chunker = CppChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "test.cpp");
        let imports = chunker.extract_imports(&tree.unwrap(), source);
        assert_eq!(imports.len(), 2);
        assert!(imports.iter().any(|i| i.module_path == "vector"));
        assert!(imports.iter().any(|i| i.module_path == "engine.h"));
    }

    #[test]
    fn test_cpp_struct() {
        let source = "struct Config {\n    int sample_rate;\n    int buffer_size;\n};\n";
        let chunker = CppChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "config.cpp");

        let st = chunks.iter().find(|c| c.chunk_type == ChunkType::Struct);
        assert!(st.is_some(), "Should find a struct chunk");
        assert_eq!(st.unwrap().name.as_deref(), Some("Config"));
    }
}
