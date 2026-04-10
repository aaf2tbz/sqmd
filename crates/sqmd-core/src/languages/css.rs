use crate::chunk::{Chunk, ChunkType, SourceType};
use crate::chunker::LanguageChunker;
use tree_sitter::Tree;

pub struct CssChunker;

impl Default for CssChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl CssChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for CssChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_css::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "css"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        walk_css_node(tree.root_node(), source, file_path, 0, chunks);
    }

    fn chunk_unclaimed(
        &self,
        _tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
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
                            language: "css".to_string(),
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
                    language: "css".to_string(),
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
}

fn walk_css_node(
    node: tree_sitter::Node,
    source: &str,
    file_path: &str,
    depth: usize,
    chunks: &mut Vec<Chunk>,
) {
    if depth > 4 {
        return;
    }

    let kind = node.kind();

    match kind {
        "rule_set" => {
            let name = extract_css_selector(node, source);
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            let first_line = text.lines().next().unwrap_or("").trim();
            let sig = if first_line.len() <= 120 {
                Some(first_line.to_string())
            } else {
                None
            };

            let line_count = text.lines().count();
            let importance = if line_count > 20 { 0.6 } else { 0.4 };

            let hash = crate::files::content_hash(text.as_bytes());
            chunks.push(Chunk {
                file_path: file_path.to_string(),
                language: "css".to_string(),
                chunk_type: ChunkType::Struct,
                source_type: SourceType::Code,
                name,
                signature: sig,
                line_start: node.start_position().row,
                line_end: node.end_position().row,
                content_raw: text.to_string(),
                content_hash: hash,
                importance,
                metadata: serde_json::Map::new(),
                agent_id: None,
                tags: None,
                decay_rate: 0.0,
                created_by: None,
            });
        }
        "keyframes_statement" | "media_statement" | "supports_statement" | "layer_statement" => {
            let name = extract_at_rule_name(node, source);
            let text = node.utf8_text(source.as_bytes()).unwrap_or("");
            let first_line = text.lines().next().unwrap_or("").trim();
            let sig = if first_line.len() <= 120 {
                Some(first_line.to_string())
            } else {
                None
            };

            let hash = crate::files::content_hash(text.as_bytes());
            chunks.push(Chunk {
                file_path: file_path.to_string(),
                language: "css".to_string(),
                chunk_type: ChunkType::Module,
                source_type: SourceType::Code,
                name,
                signature: sig,
                line_start: node.start_position().row,
                line_end: node.end_position().row,
                content_raw: text.to_string(),
                content_hash: hash,
                importance: 0.6,
                metadata: serde_json::Map::new(),
                agent_id: None,
                tags: None,
                decay_rate: 0.0,
                created_by: None,
            });
        }
        "comment" => {
            if depth <= 1 {
                let text = node.utf8_text(source.as_bytes()).unwrap_or("");
                let hash = crate::files::content_hash(text.as_bytes());
                chunks.push(Chunk {
                    file_path: file_path.to_string(),
                    language: "css".to_string(),
                    chunk_type: ChunkType::Section,
                    source_type: SourceType::Code,
                    name: Some("comment".to_string()),
                    signature: None,
                    line_start: node.start_position().row,
                    line_end: node.end_position().row,
                    content_raw: text.to_string(),
                    content_hash: hash,
                    importance: 0.2,
                    metadata: serde_json::Map::new(),
                    agent_id: None,
                    tags: None,
                    decay_rate: 0.0,
                    created_by: None,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_css_node(child, source, file_path, depth + 1, chunks);
    }
}

fn extract_css_selector(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "selectors" {
            let text = child.utf8_text(source.as_bytes()).ok()?;
            let selector = text.split(',').next()?.trim();
            if !selector.is_empty() && selector.len() <= 120 {
                return Some(selector.to_string());
            }
        }
    }
    None
}

fn extract_at_rule_name(node: tree_sitter::Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "identifier" || child.kind() == "keyframes_name" {
            return child
                .utf8_text(source.as_bytes())
                .ok()
                .map(|s| s.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_css_basic_rules() {
        let source = r#"
body {
    font-family: Arial, sans-serif;
    margin: 0;
    padding: 0;
}

.container {
    max-width: 1200px;
    margin: 0 auto;
}
"#;
        let chunker = CssChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "styles.css");

        assert!(
            chunks.len() >= 2,
            "Should find at least 2 rules, got {}",
            chunks.len()
        );
        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.contains(&"body"), "got: {:?}", names);
        assert!(names.contains(&".container"), "got: {:?}", names);
    }

    #[test]
    fn test_css_media_query() {
        let source = r#"
@media (max-width: 768px) {
    .container {
        padding: 0 16px;
    }
}
"#;
        let chunker = CssChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "responsive.css");

        assert!(!chunks.is_empty(), "Should find chunks");
        let media: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Module)
            .collect();
        assert!(!media.is_empty(), "Should find media query as Module");
    }

    #[test]
    fn test_css_keyframes() {
        let source = r#"
@keyframes fadeIn {
    from { opacity: 0; }
    to { opacity: 1; }
}
"#;
        let chunker = CssChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "animations.css");

        assert!(!chunks.is_empty());
        let kf: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Module)
            .collect();
        assert!(!kf.is_empty(), "Should find keyframes as Module");
    }

    #[test]
    fn test_css_nested_selectors() {
        let source = r#"
nav ul {
    list-style: none;
}

nav ul li {
    display: inline-block;
}

nav a {
    text-decoration: none;
    color: #333;
}
"#;
        let chunker = CssChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "nav.css");

        assert!(
            chunks.len() >= 3,
            "Should find at least 3 rules, got {}",
            chunks.len()
        );
    }
}
