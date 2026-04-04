use tree_sitter::{Parser, Language, Tree, Node};

pub struct ChunkResult {
    pub chunk: crate::chunk::Chunk,
    pub contains: Vec<crate::chunk::Chunk>,
}

pub trait LanguageChunker: Send + Sync {
    fn language(&self) -> Language;
    fn language_name(&self) -> &str;

    fn chunk(&self, source: &str, file_path: &str) -> Vec<crate::chunk::Chunk> {
        let mut parser = Parser::new();
        if parser.set_language(&self.language()).is_err() {
            return FileChunker::chunk_file(source, file_path, self.language_name());
        }
        let tree = match parser.parse(source, None) {
            Some(t) => t,
            None => return FileChunker::chunk_file(source, file_path, self.language_name()),
        };
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

    fn extract_contains(&self, source: &str, file_path: &str) -> Vec<crate::relationships::ImportInfo> {
        let _ = (source, file_path);
        Vec::new()
    }
}

fn lines_before(node: Node, source: &str, count: usize) -> String {
    let start_byte = node.start_byte();
    let prefix = &source[..start_byte];
    let mut lines = prefix.rsplit('\n');
    lines.next();
    let take = std::cmp::min(count, 3);
    lines.take(take).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")
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
) -> Option<crate::chunk::Chunk> {
    let start_byte = node.start_byte();
    let end_byte = node.end_byte().min(source.len());

    if start_byte >= source.len() || end_byte <= start_byte {
        return None;
    }

    let text = &source[start_byte..end_byte];
    if text.trim().is_empty() {
        return None;
    }

    let context = lines_before(node, source, 3);

    let mut metadata = extra_metadata;
    if !context.is_empty() {
        metadata.insert("context_before".to_string(), serde_json::Value::String(context));
    }

    Some(crate::chunk::Chunk {
        file_path: file_path.to_string(),
        language: language.to_string(),
        chunk_type,
        name: name.map(|s| s.to_string()),
        signature: signature.map(|s| s.to_string()),
        line_start: node.start_position().row,
        line_end: node.end_position().row,
        content_raw: text.to_string(),
        content_hash: crate::files::content_hash(text.as_bytes()),
        importance: chunk_type.importance(),
        metadata,
    })
}

pub(crate) struct FileChunker;

impl FileChunker {
    pub fn chunk_file(content: &str, relative: &str, language: &str) -> Vec<crate::chunk::Chunk> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return vec![];
        }

        let mut chunks = Vec::new();
        let mut current_start = 0;
        let max_section_lines = 50;

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            let is_block_boundary = Self::is_declaration(trimmed, language);

            if is_block_boundary && i > current_start {
                let section_text = lines[current_start..i].join("\n");
                if !section_text.trim().is_empty() {
                    chunks.push(Self::make_section_chunk(
                        &section_text,
                        relative,
                        language,
                        current_start,
                        i,
                    ));
                }
                current_start = i;
            }

            if (i - current_start >= max_section_lines) || (i == lines.len() - 1 && i >= current_start) {
                let end = if i == lines.len() - 1 { i + 1 } else { i };
                let section_text = lines[current_start..end].join("\n");
                if !section_text.trim().is_empty() {
                    chunks.push(Self::make_section_chunk(
                        &section_text,
                        relative,
                        language,
                        current_start,
                        end,
                    ));
                }
                current_start = end;
            }
        }

        if chunks.is_empty() && !content.trim().is_empty() {
            chunks.push(Self::make_section_chunk(
                content,
                relative,
                language,
                0,
                lines.len(),
            ));
        }

        chunks
    }

    fn is_declaration(trimmed: &str, _language: &str) -> bool {
        let keywords = [
            "fn ", "function ", "async function ", "const ", "let ", "var ",
            "class ", "interface ", "type ", "enum ", "struct ", "impl ",
            "trait ", "def ", "pub fn ", "pub struct ", "pub enum ",
            "pub trait ", "pub mod ", "mod ", "export function ",
            "export async function ", "export const ", "export default ",
            "export class ", "export interface ", "export type ",
            "@", "#[",
        ];
        keywords.iter().any(|kw| trimmed.starts_with(kw))
    }

    fn make_section_chunk(
        content: &str,
        relative: &str,
        language: &str,
        start: usize,
        end: usize,
    ) -> crate::chunk::Chunk {
        let first_line = content.lines().next().unwrap_or("");
        let name = if first_line.trim().len() < 80 {
            Some(first_line.trim().to_string())
        } else {
            None
        };

        crate::chunk::Chunk {
            file_path: relative.to_string(),
            language: language.to_string(),
            chunk_type: crate::chunk::ChunkType::Section,
            name,
            signature: None,
            line_start: start,
            line_end: end,
            content_raw: content.to_string(),
            content_hash: crate::files::content_hash(content.as_bytes()),
            importance: crate::chunk::ChunkType::Section.importance(),
            metadata: serde_json::Map::new(),
        }
    }
}
