use crate::chunk::{Chunk, ChunkType, SourceType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::{Node, Tree};

pub struct RubyChunker;

impl Default for RubyChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl RubyChunker {
    pub fn new() -> Self {
        Self
    }

    fn extract_name(&self, node: Node, source: &str) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            return name_node
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.to_string());
        }
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "constant" {
                return child
                    .utf8_text(source.as_bytes())
                    .ok()
                    .map(|s| s.to_string());
            }
        }
        None
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
}

impl LanguageChunker for RubyChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_ruby::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "ruby"
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
            match child.kind() {
                "call" => {
                    let first_child = child.children(&mut child.walk()).next();
                    if let Some(fc) = first_child {
                        let fc_text = fc.utf8_text(source.as_bytes()).unwrap_or("");
                        if fc_text == "require"
                            || fc_text == "require_relative"
                            || fc_text == "require_all"
                        {
                            if let Some(chunk) = make_chunk(
                                source,
                                child,
                                file_path,
                                "ruby",
                                ChunkType::Import,
                                None,
                                None,
                                serde_json::Map::new(),
                            ) {
                                chunks.push(chunk);
                            }
                        }
                    }
                }
                "method" | "singleton_method" => {
                    let name = self.extract_name(child, source);
                    let sig = self.extract_signature(child, source);

                    let mut metadata = serde_json::Map::new();
                    if child.kind() == "singleton_method" {
                        metadata.insert("singleton".into(), serde_json::Value::Bool(true));
                    }

                    let body = child.child_by_field_name("body");
                    if let Some(body_node) = body {
                        let mut inner = body_node.walk();
                        for inner_child in body_node.children(&mut inner) {
                            if inner_child.kind() == "comment" {
                                let comment_text =
                                    inner_child.utf8_text(source.as_bytes()).unwrap_or("");
                                if comment_text.contains("TODO")
                                    || comment_text.contains("FIXME")
                                    || comment_text.contains("HACK")
                                {
                                    metadata
                                        .insert("has_todo".into(), serde_json::Value::Bool(true));
                                }
                                break;
                            }
                        }
                    }

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "ruby",
                        ChunkType::Function,
                        name.as_deref(),
                        sig.as_deref(),
                        metadata,
                    ) {
                        chunks.push(chunk);
                    }
                }
                "class" | "module" => {
                    let name = self.extract_name(child, source);
                    let chunk_type = if child.kind() == "module" {
                        ChunkType::Module
                    } else {
                        ChunkType::Class
                    };

                    if let Some(chunk) = make_chunk(
                        source,
                        child,
                        file_path,
                        "ruby",
                        chunk_type,
                        name.as_deref(),
                        None,
                        serde_json::Map::new(),
                    ) {
                        chunks.push(chunk);
                    }

                    let body = child.child_by_field_name("body");
                    if let Some(body_node) = body {
                        let mut inner = body_node.walk();
                        for inner_child in body_node.children(&mut inner) {
                            if inner_child.kind() == "method"
                                || inner_child.kind() == "singleton_method"
                            {
                                let mname = self.extract_name(inner_child, source);
                                let msig = self.extract_signature(inner_child, source);

                                let mut metadata = serde_json::Map::new();
                                metadata
                                    .insert("class_member".into(), serde_json::Value::Bool(true));
                                if inner_child.kind() == "singleton_method" {
                                    metadata
                                        .insert("singleton".into(), serde_json::Value::Bool(true));
                                }

                                if let Some(chunk) = make_chunk(
                                    source,
                                    inner_child,
                                    file_path,
                                    "ruby",
                                    ChunkType::Method,
                                    mname.as_deref(),
                                    msig.as_deref(),
                                    metadata,
                                ) {
                                    chunks.push(chunk);
                                }
                            }
                        }
                    }
                }
                "assignment" => {
                    let left = child.child_by_field_name("left");
                    if let Some(left_node) = left {
                        if left_node.kind() == "constant" {
                            let name = left_node.utf8_text(source.as_bytes()).unwrap_or("");
                            if name.chars().all(|c| c.is_uppercase() || c == '_')
                                && name.contains('_')
                            {
                                let mut metadata = serde_json::Map::new();
                                metadata.insert("constant".into(), serde_json::Value::Bool(true));
                                if let Some(chunk) = make_chunk(
                                    source,
                                    child,
                                    file_path,
                                    "ruby",
                                    ChunkType::Constant,
                                    Some(name),
                                    None,
                                    metadata,
                                ) {
                                    chunks.push(chunk);
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }
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
                            language: "ruby".to_string(),
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
                    language: "ruby".to_string(),
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

    fn extract_imports(&self, tree: &Tree, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut imports = Vec::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() == "call" {
                let first_child = child.children(&mut child.walk()).next();
                if let Some(fc) = first_child {
                    let fc_text = fc.utf8_text(source.as_bytes()).unwrap_or("");
                    if fc_text == "require" || fc_text == "require_relative" {
                        let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                        let cleaned = text
                            .trim_start_matches("require")
                            .trim_start_matches("_relative")
                            .trim()
                            .trim_matches('"')
                            .trim_matches('\'');
                        if !cleaned.is_empty() {
                            imports.push(crate::relationships::ImportInfo {
                                source_file: String::new(),
                                module_path: cleaned.to_string(),
                                names: Vec::new(),
                            });
                        }
                    }
                }
            }
        }

        imports
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ruby_class_and_method() {
        let source = r#"
require 'json'

class User
  attr_accessor :name, :email

  def initialize(name, email)
    @name = name
    @email = email
  end

  def to_json
    { name: @name, email: @email }.to_json
  end
end
"#;
        let chunker = RubyChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "user.rb");

        let classes: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Class)
            .collect();
        assert_eq!(classes.len(), 1, "Should find 1 class");
        assert_eq!(classes[0].name.as_deref(), Some("User"));

        let methods: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Method)
            .collect();
        assert!(
            methods.len() >= 2,
            "Should find at least 2 methods, got {:?}",
            methods.len()
        );

        let imports: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Import)
            .collect();
        assert_eq!(imports.len(), 1);
    }

    #[test]
    fn test_ruby_module() {
        let source = r#"
module Admin
  def self.authenticate(token)
    User.find_by_token(token)
  end
end
"#;
        let chunker = RubyChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "admin.rb");

        let modules: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Module)
            .collect();
        assert_eq!(modules.len(), 1);
        assert_eq!(modules[0].name.as_deref(), Some("Admin"));
    }

    #[test]
    fn test_ruby_singleton_method() {
        let source = r#"
class Config
  def self.load(path)
    YAML.load_file(path)
  end
end
"#;
        let chunker = RubyChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "config.rb");

        let methods: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Method)
            .collect();
        let load = methods.iter().find(|m| m.name.as_deref() == Some("load"));
        assert!(load.is_some(), "Should find load method");
    }

    #[test]
    fn test_ruby_constants() {
        let source = "MAX_RETRIES = 3\nDATABASE_URL = \"postgres://localhost\"\nregular_var = 42\n";
        let chunker = RubyChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "config.rb");

        let constants: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Constant)
            .collect();
        assert!(
            constants.len() >= 2,
            "Should find SCREAMING_SNAKE_CASE constants"
        );
    }

    #[test]
    fn test_ruby_extract_imports() {
        let source = "require 'json'\nrequire 'net/http'\nrequire_relative 'models'\n";
        let chunker = RubyChunker::new();
        let (_chunks, tree) = chunker.chunk(source, "test.rb");
        let imports = chunker.extract_imports(&tree.unwrap(), source);

        assert!(
            imports.len() >= 2,
            "Should find at least 2 requires, got {}: {:?}",
            imports.len(),
            imports
        );
        assert_eq!(imports[0].module_path, "json");
    }
}
