use crate::chunk::{Chunk, ChunkType, SourceType};

pub struct MarkdownChunker;

impl MarkdownChunker {
    pub fn chunk(source: &str, file_path: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let lines: Vec<&str> = source.lines().collect();
        let mut current_start: usize = 0;
        let mut current_heading: String = String::new();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if is_heading(trimmed) {
                if i > current_start {
                    let text: String = lines[current_start..i].join("\n");
                    if !text.trim().is_empty() {
                        chunks.push(make_md_chunk(
                            &text,
                            file_path,
                            current_start,
                            i,
                            &current_heading,
                        ));
                    }
                }
                current_start = i;
                current_heading = trimmed.trim_start_matches('#').trim().to_string();
            }
        }

        if current_start < lines.len() {
            let text: String = lines[current_start..lines.len()].join("\n");
            if !text.trim().is_empty() {
                chunks.push(make_md_chunk(
                    &text,
                    file_path,
                    current_start,
                    lines.len(),
                    &current_heading,
                ));
            }
        }

        chunks
    }
}

fn is_heading(line: &str) -> bool {
    let hashes: usize = line.chars().take_while(|&c| c == '#').count();
    (1..=6).contains(&hashes) && line.len() > hashes && line.chars().nth(hashes) == Some(' ')
}

fn make_md_chunk(text: &str, file_path: &str, start: usize, end: usize, heading: &str) -> Chunk {
    let name = if heading.is_empty() {
        let first = text.lines().next().unwrap_or("").trim();
        if first.len() < 80 && !first.is_empty() {
            Some(first.to_string())
        } else {
            None
        }
    } else {
        Some(heading.to_string())
    };

    let sig = text.lines().next().unwrap_or("").trim();
    let sig = if sig.len() <= 120 {
        Some(sig.to_string())
    } else {
        None
    };

    let line_count = end - start;
    let importance = if heading.starts_with("# ") || heading.starts_with("## ") {
        if line_count > 20 {
            0.7
        } else {
            0.5
        }
    } else if heading.starts_with("### ") {
        0.4
    } else {
        0.3
    };

    Chunk {
        file_path: file_path.to_string(),
        language: "markdown".to_string(),
        chunk_type: ChunkType::Section,
        source_type: SourceType::Code,
        name,
        signature: sig,
        line_start: start,
        line_end: end,
        content_raw: text.to_string(),
        content_hash: crate::files::content_hash(text.as_bytes()),
        importance,
        metadata: serde_json::Map::new(),
        agent_id: None,
        tags: None,
        decay_rate: 0.0,
        created_by: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_headings() {
        let source = "# Introduction\n\nThis is the intro.\n\n## Getting Started\n\nSteps here.\n\n## API Reference\n\nEndpoints.";
        let chunks = MarkdownChunker::chunk(source, "README.md");

        assert!(
            chunks.len() >= 3,
            "Should find at least 3 sections, got {}",
            chunks.len()
        );
        let names: Vec<_> = chunks.iter().filter_map(|c| c.name.as_deref()).collect();
        assert!(names.iter().any(|n| n.contains("Introduction")));
        assert!(names.iter().any(|n| n.contains("Getting Started")));
        assert!(names.iter().any(|n| n.contains("API Reference")));
    }

    #[test]
    fn test_markdown_content_preserved() {
        let source = "# API\n\nThe API uses JSON.\n\n## Auth\n\nToken based.";
        let chunks = MarkdownChunker::chunk(source, "docs.md");

        let api = chunks.iter().find(|c| c.name.as_deref() == Some("API"));
        assert!(api.is_some());
        assert!(
            api.unwrap().content_raw.contains("JSON"),
            "Section content should include body text"
        );
    }

    #[test]
    fn test_markdown_preamble() {
        let source = "Title\n\nBody text.\n\n# Section One\n\nContent.";
        let chunks = MarkdownChunker::chunk(source, "doc.md");

        assert!(chunks.len() >= 2, "Should have preamble + heading section");
    }
}
