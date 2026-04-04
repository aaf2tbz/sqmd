use tree_sitter::{Node, Tree};
use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{LanguageChunker, make_chunk};

pub struct RustChunker;

impl Default for RustChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl RustChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_name(&self, node: Node, source: &str) -> Option<String> {
        node.child_by_field_name("name")
            .and_then(|n| n.utf8_text(source.as_bytes()).ok())
            .map(|s| s.to_string())
    }

    fn extract_signature(&self, node: Node, source: &str) -> Option<String> {
        let text = node.utf8_text(source.as_bytes()).ok()?;
        let first_line = text.lines().next()?.trim();
        if first_line.len() <= 120 {
            Some(first_line.to_string())
        } else {
            None
        }
    }

    fn extract_visibility(&self, node: Node) -> bool {
        for child in node.children(&mut node.walk()) {
            if child.kind() == "visibility_modifier" {
                return true;
            }
        }
        false
    }

    fn extract_impl_items(&self, impl_node: Node, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let mut cursor = impl_node.walk();
        let mut found_body = false;
        for child in impl_node.children(&mut cursor) {
            let kind = child.kind();
            if kind == "declaration_list" || kind == "trait_item" {
                found_body = true;
                let mut inner = child.walk();
                for item in child.children(&mut inner) {
                    let ik = item.kind();
                    if ik == "function_item" || ik == "const_item" || ik == "type_item" || ik == "assoc_type_item" {
                        let name = self.extract_name(item, source);
                        let sig = self.extract_signature(item, source);
                        let ct = match ik {
                            "function_item" => ChunkType::Method,
                            "const_item" => ChunkType::Constant,
                            "type_item" | "assoc_type_item" => ChunkType::Type,
                            _ => ChunkType::Section,
                        };

                        let mut metadata = serde_json::Map::new();
                        metadata.insert("impl_member".to_string(), serde_json::Value::Bool(true));

                        if let Some(chunk) = make_chunk(source, item, file_path, "rust", ct, name.as_deref(), sig.as_deref(), metadata) {
                            chunks.push(chunk);
                        }
                    }
                }
            }
        }
        if !found_body {
            let mut cursor = impl_node.walk();
            for child in impl_node.children(&mut cursor) {
                let kind = child.kind();
                if kind == "function_item" || kind == "const_item" || kind == "type_item" || kind == "assoc_type_item" {
                    let name = self.extract_name(child, source);
                    let sig = self.extract_signature(child, source);
                    let ct = match kind {
                        "function_item" => ChunkType::Method,
                        "const_item" => ChunkType::Constant,
                        "type_item" | "assoc_type_item" => ChunkType::Type,
                        _ => ChunkType::Section,
                    };

                    let mut metadata = serde_json::Map::new();
                    metadata.insert("impl_member".to_string(), serde_json::Value::Bool(true));

                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ct, name.as_deref(), sig.as_deref(), metadata) {
                        chunks.push(chunk);
                    }
                }
            }
        }
    }
}

impl LanguageChunker for RustChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_rust::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "rust"
    }

    fn walk_declarations(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            match kind {
                "use_declaration" => {
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Import, None, None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                }
                "function_item" => {
                    let name = self.extract_name(child, source);
                    let sig = self.extract_signature(child, source);
                    let is_pub = self.extract_visibility(child);
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("public".to_string(), serde_json::Value::Bool(is_pub));
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Function, name.as_deref(), sig.as_deref(), metadata) {
                        chunks.push(chunk);
                    }
                }
                "struct_item" => {
                    let name = self.extract_name(child, source);
                    let is_pub = self.extract_visibility(child);
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("public".to_string(), serde_json::Value::Bool(is_pub));
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Struct, name.as_deref(), None, metadata) {
                        chunks.push(chunk);
                    }
                }
                "enum_item" => {
                    let name = self.extract_name(child, source);
                    let is_pub = self.extract_visibility(child);
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("public".to_string(), serde_json::Value::Bool(is_pub));
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Enum, name.as_deref(), None, metadata) {
                        chunks.push(chunk);
                    }
                }
                "trait_item" => {
                    let name = self.extract_name(child, source);
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Trait, name.as_deref(), None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                    self.extract_impl_items(child, source, file_path, chunks);
                }
                "impl_item" => {
                    let name = self.extract_name(child, source);
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Impl, name.as_deref(), None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                    self.extract_impl_items(child, source, file_path, chunks);
                }
                "mod_item" => {
                    let name = self.extract_name(child, source);
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Module, name.as_deref(), None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                }
                "const_item" | "static_item" => {
                    let name = self.extract_name(child, source);
                    let is_pub = self.extract_visibility(child);
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("public".to_string(), serde_json::Value::Bool(is_pub));
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Constant, name.as_deref(), None, metadata) {
                        chunks.push(chunk);
                    }
                }
                "type_item" => {
                    let name = self.extract_name(child, source);
                    let is_pub = self.extract_visibility(child);
                    let mut metadata = serde_json::Map::new();
                    metadata.insert("public".to_string(), serde_json::Value::Bool(is_pub));
                    if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Type, name.as_deref(), None, metadata) {
                        chunks.push(chunk);
                    }
                }
                "macro_invocation" => {
                    let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                    let name = text.lines().next().unwrap_or("").trim();
                    if name.len() <= 80 && !name.starts_with('#') {
                        if let Some(chunk) = make_chunk(source, child, file_path, "rust", ChunkType::Macro, Some(name), None, serde_json::Map::new()) {
                            chunks.push(chunk);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn extract_imports(&self, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut imports = Vec::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() == "use_declaration" {
                let mut names = Vec::new();
                let mut module_path = String::new();

                let arg = child.child_by_field_name("argument");
                if let Some(arg_node) = arg {
                    let arg_text = arg_node.utf8_text(source.as_bytes()).unwrap_or("");
                    parse_use_path(arg_text, &mut module_path, &mut names);
                }

                if !module_path.is_empty() || !names.is_empty() {
                    imports.push(crate::relationships::ImportInfo {
                        source_file: String::new(),
                        module_path,
                        names,
                    });
                }
            }
        }

        imports
    }

    fn chunk_unclaimed(&self, _tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let mut claimed_ranges: Vec<(usize, usize)> = chunks.iter().map(|c| (c.line_start, c.line_end)).collect();
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
                        chunks.push(Chunk {
                            file_path: file_path.to_string(),
                            language: "rust".to_string(),
                            chunk_type: ChunkType::Section,
                            name: None,
                            signature: None,
                            line_start: effective_start,
                            line_end: effective_end,
                            content_raw: text.clone(),
                            importance: ChunkType::Section.importance(),
                            content_hash: crate::files::content_hash(text.as_bytes()),
                            metadata: serde_json::Map::new(),
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
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "rust".to_string(),
                    chunk_type: ChunkType::Section,
                    name: None,
                    signature: None,
                    line_start: gap_start,
                    line_end: effective_end,
                    content_raw: text.clone(),
                    importance: ChunkType::Section.importance(),
                    content_hash: crate::files::content_hash(text.as_bytes()),
                    metadata: serde_json::Map::new(),
                });
            }
        }
    }
}

fn parse_use_path(text: &str, module_path: &mut String, names: &mut Vec<String>) {
    let trimmed = text.trim();

    if let Some(brace_pos) = trimmed.find("::{") {
        *module_path = trimmed[..brace_pos].to_string();
        let inner = &trimmed[brace_pos + 3..];
        let inner = inner.trim_start_matches('{').trim_end_matches('}');
        for item in inner.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if let Some(as_pos) = item.find(" as ") {
                names.push(item[..as_pos].trim().to_string());
            } else {
                names.push(item.to_string());
            }
        }
    } else if let Some(star_pos) = trimmed.rfind("::*") {
        *module_path = trimmed[..star_pos].to_string();
    } else if let Some(as_pos) = trimmed.find(" as ") {
        let path = trimmed[..as_pos].trim();
        if let Some(last) = path.rfind("::") {
            *module_path = path[..last].to_string();
            names.push(path[last + 2..].to_string());
        } else {
            *module_path = path.to_string();
        }
    } else if let Some(last) = trimmed.rfind("::") {
        *module_path = trimmed[..last].to_string();
        names.push(trimmed[last + 2..].to_string());
    } else {
        *module_path = trimmed.to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_function() {
        let source = r#"use std::collections::HashMap;

pub fn authenticate(credentials: &Credentials) -> Result<Auth, Error> {
    let user = db::find_user(&credentials.email)?;
    verify_password(&user, &credentials.password)?;
    Ok(Auth::new(user))
}

fn private_helper() -> bool {
    true
}
"#;
        let chunker = RustChunker::new();
        let chunks = chunker.chunk(source, "src/auth.rs");

        let funcs: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Function).collect();
        assert_eq!(funcs.len(), 2);

        assert_eq!(funcs[0].name.as_deref(), Some("authenticate"));
        assert!(funcs[0].metadata["public"].as_bool().unwrap());

        assert_eq!(funcs[1].name.as_deref(), Some("private_helper"));
        assert!(!funcs[1].metadata["public"].as_bool().unwrap());

        let imp = chunks.iter().find(|c| c.chunk_type == ChunkType::Import);
        assert!(imp.is_some());
    }

    #[test]
    fn test_rust_struct_and_impl() {
        let source = r#"
pub struct User {
    id: String,
    email: String,
}

impl User {
    pub fn new(id: &str, email: &str) -> Self {
        Self { id: id.to_string(), email: email.to_string() }
    }

    fn is_valid(&self) -> bool {
        !self.email.is_empty()
    }
}
"#;
        let chunker = RustChunker::new();
        let chunks = chunker.chunk(source, "src/user.rs");

        let structs: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Struct).collect();
        assert_eq!(structs.len(), 1);
        assert_eq!(structs[0].name.as_deref(), Some("User"));

        let impls: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Impl).collect();
        assert_eq!(impls.len(), 1);

        let methods: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Method).collect();
        assert_eq!(methods.len(), 2);
        assert_eq!(methods[0].name.as_deref(), Some("new"));
        assert_eq!(methods[1].name.as_deref(), Some("is_valid"));
    }

    #[test]
    fn test_rust_enum() {
        let source = r#"
pub enum AuthResult {
    Success { token: String },
    Failure(AuthError),
    Pending,
}
"#;
        let chunker = RustChunker::new();
        let chunks = chunker.chunk(source, "src/auth.rs");

        let enums: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Enum).collect();
        assert_eq!(enums.len(), 1);
        assert_eq!(enums[0].name.as_deref(), Some("AuthResult"));
    }

    #[test]
    fn test_rust_extract_imports() {
        let source = "use crate::chunker::LanguageChunker;";
        let chunker = RustChunker::new();
        let imports = chunker.extract_imports(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module_path, "crate::chunker");
        assert_eq!(imports[0].names, vec!["LanguageChunker"]);
    }

    #[test]
    fn test_rust_extract_grouped_imports() {
        let source = "use crate::files::{SourceFile, walk_project, content_hash};";
        let chunker = RustChunker::new();
        let imports = chunker.extract_imports(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module_path, "crate::files");
        assert!(imports[0].names.contains(&"SourceFile".to_string()));
    }

    #[test]
    fn test_rust_extract_glob_import() {
        let source = "use crate::files::*;";
        let chunker = RustChunker::new();
        let imports = chunker.extract_imports(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module_path, "crate::files");
        assert_eq!(imports[0].names, Vec::<String>::new());
    }

    #[test]
    fn test_rust_extract_as_import() {
        let source = "use std::collections::HashMap as Map;";
        let chunker = RustChunker::new();
        let imports = chunker.extract_imports(source);
        assert_eq!(imports.len(), 1);
        assert_eq!(imports[0].module_path, "std::collections");
        assert_eq!(imports[0].names, vec!["HashMap"]);
    }
}
