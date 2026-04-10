use crate::chunk::{Chunk, ChunkType, SourceType};
use crate::chunker::{make_chunk, LanguageChunker};
use tree_sitter::{Node, Tree};

pub struct HtmlChunker;

impl Default for HtmlChunker {
    fn default() -> Self {
        Self::new()
    }
}

impl HtmlChunker {
    pub fn new() -> Self {
        Self
    }
}

impl LanguageChunker for HtmlChunker {
    fn language(&self) -> tree_sitter::Language {
        tree_sitter_html::LANGUAGE.into()
    }

    fn language_name(&self) -> &str {
        "html"
    }

    fn walk_declarations(
        &self,
        tree: &Tree,
        source: &str,
        file_path: &str,
        chunks: &mut Vec<Chunk>,
    ) {
        walk_html_node(tree.root_node(), source, file_path, 0, chunks);
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
                            language: "html".to_string(),
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
                    language: "html".to_string(),
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

fn extract_tag_name_from_element(node: Node, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "start_tag" || child.kind() == "self_closing_tag" {
            let mut tc = child.walk();
            for tc_child in child.children(&mut tc) {
                if tc_child.kind() == "tag_name" {
                    return tc_child
                        .utf8_text(source.as_bytes())
                        .ok()
                        .map(|s| s.to_string());
                }
            }
        }
    }
    None
}

fn walk_html_node(
    node: Node,
    source: &str,
    file_path: &str,
    depth: usize,
    chunks: &mut Vec<Chunk>,
) {
    if depth > 5 {
        return;
    }

    let kind = node.kind();

    match kind {
        "element" | "script_element" | "style_element" => {
            let tag_name = extract_tag_name_from_element(node, source);

            let ct = match tag_name.as_deref() {
                Some("html") | Some("body") | Some("head") => ChunkType::Module,
                Some(n)
                    if n == "header"
                        || n == "footer"
                        || n == "nav"
                        || n == "main"
                        || n == "section"
                        || n == "article"
                        || n == "aside"
                        || n == "form" =>
                {
                    ChunkType::Struct
                }
                Some("script") | Some("style") => ChunkType::Section,
                _ => ChunkType::Section,
            };

            let chunk_name = tag_name
                .filter(|n| !matches!(n.as_str(), "div" | "span" | "p" | "br" | "hr" | "li"));

            if let Some(chunk) = make_chunk(
                source,
                node,
                file_path,
                "html",
                ct,
                chunk_name.as_deref(),
                None,
                serde_json::Map::new(),
            ) {
                if chunk.content_raw.lines().count() > 0 {
                    chunks.push(chunk);
                }
            }

            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_html_node(child, source, file_path, depth + 1, chunks);
            }
        }
        "doctype" => {
            if let Some(chunk) = make_chunk(
                source,
                node,
                file_path,
                "html",
                ChunkType::Section,
                Some("doctype"),
                None,
                serde_json::Map::new(),
            ) {
                chunks.push(chunk);
            }
        }
        "comment" => {
            if depth <= 1 {
                if let Some(chunk) = make_chunk(
                    source,
                    node,
                    file_path,
                    "html",
                    ChunkType::Section,
                    Some("comment"),
                    None,
                    serde_json::Map::new(),
                ) {
                    chunks.push(chunk);
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                walk_html_node(child, source, file_path, depth + 1, chunks);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_basic_elements() {
        let source = r#"<!DOCTYPE html>
<html>
<head>
    <title>Test Page</title>
</head>
<body>
    <h1>Hello World</h1>
    <p>This is a paragraph.</p>
</body>
</html>"#;
        let chunker = HtmlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "index.html");

        assert!(!chunks.is_empty(), "Should find chunks in HTML file");

        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(
            names.contains(&"html"),
            "Should find html element, got: {:?}",
            names
        );
        assert!(
            names.contains(&"body"),
            "Should find body element, got: {:?}",
            names
        );
        assert!(
            names.contains(&"head"),
            "Should find head element, got: {:?}",
            names
        );
    }

    #[test]
    fn test_html_with_script_and_style() {
        let source = r#"<html>
<head>
    <style>
        body { color: red; }
    </style>
</head>
<body>
    <script>
        console.log("hello");
    </script>
</body>
</html>"#;
        let chunker = HtmlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "page.html");

        assert!(!chunks.is_empty());
        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(
            names.contains(&"script"),
            "Should find script, got: {:?}",
            names
        );
        assert!(
            names.contains(&"style"),
            "Should find style, got: {:?}",
            names
        );
    }

    #[test]
    fn test_html_form() {
        let source = r#"<html>
<body>
    <form action="/submit" method="post">
        <input type="text" name="username" />
        <button type="submit">Submit</button>
    </form>
</body>
</html>"#;
        let chunker = HtmlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "form.html");

        let forms: Vec<_> = chunks
            .iter()
            .filter(|c| c.chunk_type == ChunkType::Struct)
            .collect();
        assert!(
            !forms.is_empty(),
            "Should find at least one structural element (form)"
        );
    }

    #[test]
    fn test_html_semantic_elements() {
        let source = r#"<html>
<body>
    <header>Header content</header>
    <nav>Navigation</nav>
    <main>Main content</main>
    <footer>Footer content</footer>
</body>
</html>"#;
        let chunker = HtmlChunker::new();
        let (chunks, _tree) = chunker.chunk(source, "semantic.html");

        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(
            names.contains(&"header"),
            "Should find header, got: {:?}",
            names
        );
        assert!(names.contains(&"nav"), "Should find nav, got: {:?}", names);
        assert!(
            names.contains(&"main"),
            "Should find main, got: {:?}",
            names
        );
        assert!(
            names.contains(&"footer"),
            "Should find footer, got: {:?}",
            names
        );
    }
}
