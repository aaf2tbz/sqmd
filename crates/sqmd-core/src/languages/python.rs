use tree_sitter::{Node, Tree};
use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{LanguageChunker, make_chunk};

pub struct PythonChunker;

impl Default for PythonChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonChunker {
    pub fn new() -> Self { Self }

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

    fn extract_decorators_from_parent(&self, node: Node, source: &str) -> Option<String> {
        if node.kind() != "decorated_definition" {
            return None;
        }
        let first_child = node.children(&mut node.walk()).next()?;
        if first_child.kind() == "decorator" {
            Some(first_child.utf8_text(source.as_bytes()).unwrap_or("").to_string())
        } else {
            None
        }
    }

    fn extract_class_methods(&self, class_node: Node, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let body = match class_node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            let actual_node = if child.kind() == "decorated_definition" {
                child.children(&mut child.walk()).find(|c| c.kind() == "function_definition")
            } else if child.kind() == "function_definition" {
                Some(child)
            } else {
                None
            };

            if let Some(fn_node) = actual_node {
                let name = self.extract_name(fn_node, source);
                let sig = self.extract_signature(fn_node, source);
                let decos = self.extract_decorators_from_parent(child, source);

                let mut metadata = serde_json::Map::new();
                metadata.insert("class_member".to_string(), serde_json::Value::Bool(true));
                if let Some(d) = decos {
                    metadata.insert("decorator".to_string(), serde_json::Value::String(d));
                }

                let node_to_chunk = if child.kind() == "decorated_definition" { child } else { fn_node };
                if let Some(chunk) = make_chunk(source, node_to_chunk, file_path, "python", ChunkType::Method, name.as_deref(), sig.as_deref(), metadata) {
                    chunks.push(chunk);
                }
            }
        }
    }
}

impl LanguageChunker for PythonChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_python::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "python"
    }

    fn walk_declarations(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            match kind {
                "import_statement" | "import_from_statement" | "future_import_statement" => {
                    if let Some(chunk) = make_chunk(source, child, file_path, "python", ChunkType::Import, None, None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                }
                "function_definition" => {
                    let name = self.extract_name(child, source);
                    let sig = self.extract_signature(child, source);

                    let metadata = serde_json::Map::new();
                    if let Some(chunk) = make_chunk(source, child, file_path, "python", ChunkType::Function, name.as_deref(), sig.as_deref(), metadata) {
                        chunks.push(chunk);
                    }
                }
                "class_definition" => {
                    let name = self.extract_name(child, source);
                    if let Some(chunk) = make_chunk(source, child, file_path, "python", ChunkType::Class, name.as_deref(), None, serde_json::Map::new()) {
                        chunks.push(chunk);
                    }
                    self.extract_class_methods(child, source, file_path, chunks);
                }
                "expression_statement" => {
                    let assignment = child.children(&mut child.walk())
                        .find(|c| c.kind() == "assignment");
                    if let Some(assign) = assignment {
                        if let Some(left) = assign.child_by_field_name("left") {
                            if left.kind() == "identifier" {
                                let name = left.utf8_text(source.as_bytes()).unwrap_or("");
                                if name.chars().all(|c| c.is_uppercase() || c == '_') && name.contains('_') {
                                    let mut metadata = serde_json::Map::new();
                                    metadata.insert("constant".to_string(), serde_json::Value::Bool(true));
                                    if let Some(chunk) = make_chunk(source, child, file_path, "python", ChunkType::Constant, Some(name), None, metadata) {
                                        chunks.push(chunk);
                                    }
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
                        let hash = crate::files::content_hash(text.as_bytes());
                        chunks.push(Chunk {
                            file_path: file_path.to_string(),
                            language: "python".to_string(),
                            chunk_type: ChunkType::Section,
                            name: None,
                            signature: None,
                            line_start: effective_start,
                            line_end: effective_end,
                            content_raw: text,
                            content_hash: hash,
                            importance: ChunkType::Section.importance(),
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
                let hash = crate::files::content_hash(text.as_bytes());
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "python".to_string(),
                    chunk_type: ChunkType::Section,
                    name: None,
                    signature: None,
                    line_start: gap_start,
                    line_end: effective_end,
                    content_raw: text,
                    content_hash: hash,
                    importance: ChunkType::Section.importance(),
                    metadata: serde_json::Map::new(),
                });
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
            match child.kind() {
                "import_statement" | "future_import_statement" => {
                    let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                    let cleaned = text.trim_start_matches("from ").trim_start_matches("import ").trim();
                    let parts: Vec<&str> = cleaned.splitn(2, ',').collect();
                    let module = parts[0].trim().to_string();

                    let mut names = Vec::new();
                    if parts.len() > 1 {
                        for item in parts[1..].iter().flat_map(|s| s.split(',')) {
                            let item = item.trim();
                            if let Some(as_pos) = item.find(" as ") {
                                names.push(item[..as_pos].trim().to_string());
                            } else {
                                names.push(item.to_string());
                            }
                        }
                    }

                    imports.push(crate::relationships::ImportInfo {
                        source_file: String::new(),
                        module_path: module,
                        names,
                    });
                }
                "import_from_statement" => {
                    let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                    let module = text.trim_start_matches("from ").split_whitespace().next().unwrap_or("").to_string();

                    let mut names = Vec::new();
                    if let Some(import_start) = text.find("import ") {
                        let import_part = &text[import_start + 7..];
                        if import_part.trim() == "*" {
                            names.push("*".to_string());
                        } else {
                            for item in import_part.split(',') {
                                let item = item.trim();
                                if item.is_empty() {
                                    continue;
                                }
                                if let Some(as_pos) = item.find(" as ") {
                                    names.push(item[..as_pos].trim().to_string());
                                } else {
                                    names.push(item.trim_start_matches('(').trim_end_matches(')').to_string());
                                }
                            }
                        }
                    }

                    imports.push(crate::relationships::ImportInfo {
                        source_file: String::new(),
                        module_path: module,
                        names,
                    });
                }
                _ => {}
            }
        }

        imports
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_function() {
        let source = "from typing import Optional\nimport os\n\nasync def authenticate(email: str, password: str) -> Optional[dict]:\n    user = await db.find_user(email)\n    if not user:\n        return None\n    return {\"token\": create_jwt(user)}\n\ndef sync_helper():\n    pass\n";
        let chunker = PythonChunker::new();
        let chunks = chunker.chunk(source, "auth.py");

        let funcs: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Function).collect();
        assert!(funcs.len() >= 2, "Should find at least 2 functions");

        let imports: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Import).collect();
        assert_eq!(imports.len(), 2);
    }

    #[test]
    fn test_python_class() {
        let source = "class UserRepository:\n    def __init__(self, db_url: str):\n        self.db = Database(db_url)\n\n    @cached(ttl=300)\n    def get_all(self) -> list:\n        pass\n";
        let chunker = PythonChunker::new();
        let chunks = chunker.chunk(source, "repo.py");

        let class = chunks.iter().find(|c| c.chunk_type == ChunkType::Class);
        assert!(class.is_some());
        assert_eq!(class.unwrap().name.as_deref(), Some("UserRepository"));

        let methods: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Method).collect();
        assert_eq!(methods.len(), 2);
    }

    #[test]
    fn test_python_constants() {
        let source = "MAX_RETRIES = 3\nDATABASE_URL = \"postgres://localhost/mydb\"\nregular_var = \"something\"\n";
        let chunker = PythonChunker::new();
        let chunks = chunker.chunk(source, "config.py");

        let constants: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Constant).collect();
        assert!(constants.len() >= 2, "Should find SCREAMING_SNAKE_CASE constants");
    }

    #[test]
    fn test_python_extract_imports() {
        let source = "from typing import Optional, List\nimport os\nfrom .models import User, Session as Sess\nfrom auth import *\n";
        let chunker = PythonChunker::new();
        let imports = chunker.extract_imports(source);

        assert_eq!(imports.len(), 4, "got {:?}: {:?}", imports.len(), imports);

        assert_eq!(imports[0].module_path, "typing");
        assert!(imports[0].names.contains(&"Optional".to_string()));
        assert!(imports[0].names.contains(&"List".to_string()));

        assert_eq!(imports[1].module_path, "os");

        assert_eq!(imports[2].module_path, ".models");
        assert!(imports[2].names.contains(&"User".to_string()));

        assert_eq!(imports[3].module_path, "auth");
        assert!(imports[3].names.contains(&"*".to_string()));
    }
}
