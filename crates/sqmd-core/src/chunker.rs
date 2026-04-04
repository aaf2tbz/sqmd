use tree_sitter::{Parser, Language, Tree, Node};

pub trait LanguageChunker: Send + Sync {
    fn language(&self) -> Language;
    fn language_name(&self) -> &str;

    fn chunk(&self, source: &str, file_path: &str) -> Vec<crate::chunk::Chunk> {
        let mut parser = Parser::new();
        parser.set_language(&self.language()).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let mut chunks = Vec::new();

        self.walk_declarations(&tree, source, file_path, &mut chunks);

        self.chunk_unclaimed(&tree, source, file_path, &mut chunks);

        chunks.sort_by_key(|c| c.line_start);
        chunks
    }

    fn walk_declarations(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<crate::chunk::Chunk>);
    fn chunk_unclaimed(&self, tree: &Tree, source: &str, file_path: &str, chunks: &mut Vec<crate::chunk::Chunk>);

    fn extract_imports(&self, source: &str) -> Vec<crate::relationships::ImportInfo> {
        let _ = source;
        Vec::new()
    }
}

#[allow(dead_code)]
fn extract_imports(node: Node, source: &str, file_path: &str) -> Vec<crate::chunk::Chunk> {
    let mut chunks = Vec::new();
    let mut cursor = node.walk();

    let import_kinds = [
        "import_statement",
        "import_declaration",
        "use_declaration",
        "import_from_statement",
        "import",
    ];

    for child in node.children(&mut cursor) {
        if import_kinds.contains(&child.kind()) {
            let text = child.utf8_text(source.as_bytes()).unwrap_or("");
            let mut metadata = serde_json::Map::new();
            metadata.insert("imports".to_string(), serde_json::Value::String(text.to_string()));

            chunks.push(crate::chunk::Chunk {
                file_path: file_path.to_string(),
                language: String::new(),
                chunk_type: crate::chunk::ChunkType::Import,
                name: Some(text.lines().next().unwrap_or("").trim().to_string()),
                signature: None,
                line_start: child.start_position().row,
                line_end: child.end_position().row,
                content_md: format!(
                    "**File:** `{}`\n**Lines:** {}-{}\n**Type:** import\n\n```\n{}\n```",
                    file_path,
                    child.start_position().row + 1,
                    child.end_position().row + 1,
                    text
                ),
                content_hash: crate::files::content_hash(text.as_bytes()),
                metadata,
            });
        }
    }

    chunks
}

fn lines_before(node: Node, source: &str, count: usize) -> String {
    let start_byte = node.start_byte();
    let lines: Vec<&str> = source[..start_byte].split('\n').collect();
    let take = std::cmp::min(count, lines.len().saturating_sub(1));
    lines.into_iter().rev().take(take).rev().collect()
}

#[allow(clippy::too_many_arguments)]
pub fn make_chunk(
    source: &str,
    node: Node,
    file_path: &str,
    language: &str,
    chunk_type: crate::chunk::ChunkType,
    name: Option<&str>,
    signature: Option<&str>,
    extra_metadata: serde_json::Map<String, serde_json::Value>,
) -> crate::chunk::Chunk {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte().min(source.len());

    if start_byte >= source.len() || end_byte <= start_byte {
        return crate::chunk::Chunk {
            file_path: file_path.to_string(),
            language: language.to_string(),
            chunk_type,
            name: name.map(|s| s.to_string()),
            signature: signature.map(|s| s.to_string()),
            line_start: 0,
            line_end: 0,
            content_md: String::new(),
            content_hash: crate::files::content_hash(b""),
            metadata: extra_metadata,
        };
    }

    let text = &source[start_byte..end_byte];
    let context = lines_before(node, source, 3);

    let mut metadata = extra_metadata;
    if !context.is_empty() {
        metadata.insert("context_before".to_string(), serde_json::Value::String(context));
    }

    let content_md = if let Some(name) = &name {
        let sig_line = signature.map(|s| format!("\n**Signature:** `{}`", s)).unwrap_or_default();
        format!(
            "### `{}`{}\n\n**File:** `{}`\n**Lines:** {}-{}\n**Type:** {}\n\n```\n{}\n```",
            name,
            sig_line,
            file_path,
            node.start_position().row + 1,
            node.end_position().row + 1,
            chunk_type.as_str(),
            text
        )
    } else {
        format!(
            "### {}\n\n**File:** `{}`\n**Lines:** {}-{}\n**Type:** {}\n\n```\n{}\n```",
            "(unnamed)",
            file_path,
            node.start_position().row + 1,
            node.end_position().row + 1,
            chunk_type.as_str(),
            text
        )
    };

    crate::chunk::Chunk {
        file_path: file_path.to_string(),
        language: language.to_string(),
        chunk_type,
        name: name.map(|s| s.to_string()),
        signature: signature.map(|s| s.to_string()),
        line_start: node.start_position().row,
        line_end: node.end_position().row,
        content_md,
        content_hash: crate::files::content_hash(text.as_bytes()),
        metadata,
    }
}
