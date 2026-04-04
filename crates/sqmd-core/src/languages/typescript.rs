use tree_sitter::{Node, Tree};
use crate::chunk::{Chunk, ChunkType};
use crate::chunker::{LanguageChunker, make_chunk};

pub struct TypeScriptChunker {
    language: tree_sitter::Language,
}

impl Default for TypeScriptChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeScriptChunker {
    pub fn new() -> Self {
        Self {
            language: tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        }
    }

    pub fn tsx() -> Self {
        Self {
            language: tree_sitter_typescript::LANGUAGE_TSX.into(),
        }
    }

    fn extract_name(&self, node: Node, source: &str) -> Option<String> {
        if let Some(child) = node.children_by_field_name("name", &mut node.walk()).next() {
            return Some(child.utf8_text(source.as_bytes()).unwrap_or("").to_string());
        }
        let first_named = node.children(&mut node.walk()).find(|c| c.is_named())?;
        let text = first_named.utf8_text(source.as_bytes()).ok()?;
        Some(text.to_string())
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

impl LanguageChunker for TypeScriptChunker {
    fn language(&self) -> tree_sitter::Language {
        self.language.clone()
    }

    fn language_name(&self) -> &str {
        "typescript"
    }

    fn walk_declarations(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<Chunk>) {
        let mut cursor = tree.root_node().walk();

        let declaration_kinds = [
            "function_declaration",
            "generator_function_declaration",
            "arrow_function",
            "class_declaration",
            "interface_declaration",
            "type_alias_declaration",
            "enum_declaration",
            "export_statement",
            "variable_declaration",
            "lexical_declaration",
            "jsx_element",
            "jsx_fragment",
            "jsx_self_closing_element",
        ];

        for child in tree.root_node().children(&mut cursor) {
            let kind = child.kind();

            if kind == "import_statement" || kind == "import_declaration" {
                if let Some(chunk) = make_chunk(
                    source,
                    child,
                    file_path,
                    "typescript",
                    ChunkType::Import,
                    None,
                    None,
                    serde_json::Map::new(),
                ) {
                    chunks.push(chunk);
                }
                continue;
            }

            if kind == "export_statement" {
                if let Some(named_child) = child.child(1) {
                    let inner_kind = named_child.kind();
                    if inner_kind == "function_declaration"
                        || inner_kind == "class_declaration"
                        || inner_kind == "interface_declaration"
                        || inner_kind == "arrow_function"
                        || inner_kind == "variable_declaration"
                    {
                        let name = self.extract_name(named_child, source);
                        let sig = self.extract_signature(named_child, source);
                        let ct = match inner_kind {
                            "function_declaration" | "generator_function_declaration" => ChunkType::Function,
                            "class_declaration" => ChunkType::Class,
                            "interface_declaration" => ChunkType::Interface,
                            "arrow_function" => ChunkType::Function,
                            "variable_declaration" => ChunkType::Constant,
                            _ => ChunkType::Section,
                        };

                        let mut metadata = serde_json::Map::new();
                        metadata.insert("exported".to_string(), serde_json::Value::Bool(true));

                        let mut all_chunks: Vec<Option<Chunk>> = Vec::new();
                        all_chunks.push(make_chunk(source, named_child, file_path, "typescript", ct, name.as_deref(), sig.as_deref(), metadata));

                        if inner_kind == "class_declaration" {
                            Self::extract_class_members(named_child, source, file_path, &mut all_chunks);
                        }

                        chunks.extend(all_chunks.into_iter().flatten());
                        continue;
                    }
                }
                let name = self.extract_name(child, source);
                if let Some(chunk) = make_chunk(source, child, file_path, "typescript", ChunkType::Export, name.as_deref(), None, serde_json::Map::new()) {
                    chunks.push(chunk);
                }
                continue;
            }

            if declaration_kinds.contains(&kind) {
                let name = self.extract_name(child, source);
                let sig = self.extract_signature(child, source);
                let ct = match kind {
                    "function_declaration" | "generator_function_declaration" => ChunkType::Function,
                    "class_declaration" => ChunkType::Class,
                    "interface_declaration" => ChunkType::Interface,
                    "type_alias_declaration" => ChunkType::Type,
                    "enum_declaration" => ChunkType::Enum,
                    "variable_declaration" | "lexical_declaration" => ChunkType::Constant,
                    _ => ChunkType::Section,
                };

                let mut metadata = serde_json::Map::new();
                if child.child_by_field_name("export").is_some() {
                    metadata.insert("exported".to_string(), serde_json::Value::Bool(true));
                }

                let mut all_chunks: Vec<Option<Chunk>> = Vec::new();
                all_chunks.push(make_chunk(source, child, file_path, "typescript", ct, name.as_deref(), sig.as_deref(), metadata));


                if kind == "class_declaration" {
                    Self::extract_class_members(child, source, file_path, &mut all_chunks);
                }

                chunks.extend(all_chunks.into_iter().flatten());
                continue;
            }
        }
    }

    fn extract_imports(&self, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&self.language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut imports = Vec::new();
        let mut cursor = tree.root_node().walk();

        for child in tree.root_node().children(&mut cursor) {
            if child.kind() == "import_statement" || child.kind() == "import_declaration" {
                let mut module_path = String::new();
                let mut names = Vec::new();

                fn walk_import_nodes(node: tree_sitter::Node, source: &str, module_path: &mut String, names: &mut Vec<String>) {
                    for gc in node.children(&mut node.walk()) {
                        match gc.kind() {
                            "string" => {
                                *module_path = gc.utf8_text(source.as_bytes())
                                    .unwrap_or("")
                                    .trim_matches('"')
                                    .trim_matches('\'')
                                    .trim_start_matches('`')
                                    .trim_end_matches('`')
                                    .to_string();
                            }
                            "import_specifier" => {
                                if let Some(name_node) = gc.child_by_field_name("name") {
                                    if let Ok(name) = name_node.utf8_text(source.as_bytes()) {
                                        names.push(name.trim().to_string());
                                    }
                                }
                            }
                            "import_clause" | "named_imports" | "import_specifiers" | "es_import_clause" => {
                                walk_import_nodes(gc, source, module_path, names);
                            }
                            _ => {}
                        }
                    }
                }

                walk_import_nodes(child, source, &mut module_path, &mut names);

                if !module_path.is_empty() {
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
            let start = *start;
            let end = *end;
            if end > gap_start {
                let gap_size = start.saturating_sub(gap_start);
                if gap_size > 0 {
                    let effective_start = gap_start;
                    let effective_end = std::cmp::min(start, gap_start + max_gap);

                    if effective_end > effective_start {
                        let text: String = source_lines[effective_start..effective_end].join("\n");
                        if !text.trim().is_empty() {
                            chunks.push(Chunk {
                                file_path: file_path.to_string(),
                                language: "typescript".to_string(),
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
        }

        if gap_start < total_lines {
            let effective_end = std::cmp::min(total_lines, gap_start + max_gap);
            let text: String = source_lines[gap_start..effective_end].join("\n");
            if !text.trim().is_empty() {
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "typescript".to_string(),
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

impl TypeScriptChunker {
    fn extract_class_members(class_node: Node, source: &str, file_path: &str, chunks: &mut Vec<Option<Chunk>>) {
        let body = match class_node.child_by_field_name("body") {
            Some(b) => b,
            None => return,
        };

        let mut cursor = body.walk();
        for child in body.children(&mut cursor) {
            let kind = child.kind();
            if kind == "method_definition"
                || kind == "public_field_definition"
                || kind == "property_definition"
                || kind == "method_signature"
                || kind == "abstract_method_declaration"
                || kind == "constructor_definition"
            {
                let name = child.child_by_field_name("name")
                    .or_else(|| child.children(&mut child.walk()).find(|c| c.is_named()))
                    .and_then(|n| n.utf8_text(source.as_bytes()).ok())
                    .map(|s| s.to_string());

                let text = child.utf8_text(source.as_bytes()).unwrap_or("");
                let sig = text.lines().next().map(|l| l.trim().to_string()).filter(|l| l.len() <= 120);

                let ct = match kind {
                    "method_definition" | "method_signature" | "abstract_method_declaration" | "constructor_definition" => ChunkType::Method,
                    "public_field_definition" | "property_definition" => ChunkType::Constant,
                    _ => ChunkType::Section,
                };

                let mut metadata = serde_json::Map::new();
                metadata.insert("class_member".to_string(), serde_json::Value::Bool(true));

                chunks.push(make_chunk(source, child, file_path, "typescript", ct, name.as_deref(), sig.as_deref(), metadata));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_typescript_function() {
        let source = r#"
import { something } from './module';

export async function authenticateUser(
  credentials: Credentials
): Promise<AuthResult> {
  const user = await db.findUser(credentials.email);
  return createSession(user);
}

const MAX_RETRIES = 3;
"#;
        let chunker = TypeScriptChunker::new();
        let chunks = chunker.chunk(source, "src/auth.ts");

        assert!(!chunks.is_empty());

        let func = chunks.iter().find(|c| c.chunk_type == ChunkType::Function);
        assert!(func.is_some(), "Should find a function chunk");
        let func = func.unwrap();
        assert_eq!(func.name.as_deref(), Some("authenticateUser"));
        assert!(func.content_raw.contains("async function authenticateUser"));
        assert!(func.metadata.contains_key("exported"));

        let imp = chunks.iter().find(|c| c.chunk_type == ChunkType::Import);
        assert!(imp.is_some(), "Should find an import chunk");

        let constant = chunks.iter().find(|c| c.chunk_type == ChunkType::Constant);
        assert!(constant.is_some(), "Should find a constant chunk");
    }

    #[test]
    fn test_typescript_class() {
        let source = r#"
class AuthService {
  private user: User | null = null;

  async login(email: string, password: string): Promise<boolean> {
    this.user = await db.findUser(email);
    return this.user !== null;
  }

  logout(): void {
    this.user = null;
  }
}
"#;
        let chunker = TypeScriptChunker::new();
        let chunks = chunker.chunk(source, "src/auth/service.ts");

        let class = chunks.iter().find(|c| c.chunk_type == ChunkType::Class);
        assert!(class.is_some(), "Should find class");
        assert_eq!(class.unwrap().name.as_deref(), Some("AuthService"));

        let methods: Vec<_> = chunks.iter().filter(|c| c.chunk_type == ChunkType::Method).collect();
        assert_eq!(methods.len(), 2, "Should find 2 methods");
        assert_eq!(methods[0].name.as_deref(), Some("login"));
        assert_eq!(methods[1].name.as_deref(), Some("logout"));
    }

    #[test]
    fn test_typescript_interface() {
        let source = r#"
interface UserRepository {
  findById(id: string): Promise<User | null>;
  findByEmail(email: string): Promise<User | null>;
  save(user: User): Promise<void>;
}
"#;
        let chunker = TypeScriptChunker::new();
        let chunks = chunker.chunk(source, "src/types.ts");

        let iface = chunks.iter().find(|c| c.chunk_type == ChunkType::Interface);
        assert!(iface.is_some());
        assert_eq!(iface.unwrap().name.as_deref(), Some("UserRepository"));
    }

    #[test]
    fn test_typescript_extract_imports() {
        let source = r#"
import { authenticate } from './auth';
import { User, Session } from './models/user';
import type { Config } from './config';
import fs from 'fs';
"#;
        let chunker = TypeScriptChunker::new();
        let imports = chunker.extract_imports(source);

        assert_eq!(imports.len(), 4, "got {:?}: {:?}", imports.len(), imports);

        assert_eq!(imports[0].module_path, "./auth");
        assert!(imports[0].names.contains(&"authenticate".to_string()));

        assert_eq!(imports[1].module_path, "./models/user");
        assert!(imports[1].names.contains(&"User".to_string()));
        assert!(imports[1].names.contains(&"Session".to_string()));

        assert_eq!(imports[2].module_path, "./config");
        assert!(imports[2].names.contains(&"Config".to_string()));
    }
}
