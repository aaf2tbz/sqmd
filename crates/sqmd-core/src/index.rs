use crate::chunker::LanguageChunker;
use crate::files::{SourceFile, walk_project, content_hash};
use crate::schema;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};

pub struct Indexer<'a> {
    db: &'a mut Connection,
    root: PathBuf,
}

#[derive(Debug)]
pub struct IndexStats {
    pub files_scanned: usize,
    pub files_indexed: usize,
    pub files_skipped: usize,
    pub files_deleted: usize,
    pub chunks_total: usize,
}

impl<'a> Indexer<'a> {
    pub fn new(db: &'a mut Connection, root: &Path) -> Self {
        Self {
            db,
            root: root.to_path_buf(),
        }
    }

    pub fn init_index(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        schema::init(self.db)?;
        Ok(())
    }

    pub fn index(&mut self) -> Result<IndexStats, Box<dyn std::error::Error>> {
        let mut stats = IndexStats {
            files_scanned: 0,
            files_indexed: 0,
            files_skipped: 0,
            files_deleted: 0,
            chunks_total: 0,
        };

        let db_paths: Vec<String> = {
            let mut stmt = self.db.prepare("SELECT path FROM files")?;
            let rows: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            rows
        };

        let mut seen_paths = std::collections::HashSet::new();

        for file_path in walk_project(&self.root) {
            stats.files_scanned += 1;
            let relative = file_path
                .strip_prefix(&self.root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();

            seen_paths.insert(relative.clone());

            let source_file = match SourceFile::from_path(&file_path) {
                Ok(f) => f,
                Err(_) => {
                    stats.files_skipped += 1;
                    continue;
                }
            };

            let existing_hash: Option<String> = self
                .db
                .query_row(
                    "SELECT hash FROM files WHERE path = ?1",
                    params![relative],
                    |r| r.get(0),
                )
                .ok();

            if existing_hash.as_deref() == Some(&source_file.hash) {
                stats.files_skipped += 1;
                continue;
            }

            if existing_hash.is_some() {
                self.delete_file_chunks(&relative)?;
            }

            self.insert_file(&relative, &source_file)?;
            stats.files_indexed += 1;
        }

        for db_path in &db_paths {
            if !seen_paths.contains(db_path) {
                self.delete_file_chunks(db_path)?;
                self.db.execute("DELETE FROM files WHERE path = ?1", params![db_path])?;
                stats.files_deleted += 1;
            }
        }

        stats.chunks_total = self
            .db
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;

        Ok(stats)
    }

    fn insert_file(&self, relative: &str, file: &SourceFile) -> Result<(), Box<dyn std::error::Error>> {
        self.db.execute(
            "INSERT OR REPLACE INTO files (path, language, size, mtime, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![relative, file.language.as_str(), file.size as i64, file.mtime, file.hash],
        )?;

        let content = std::fs::read_to_string(
            self.root.join(relative)
        ).unwrap_or_default();

        if content.trim().is_empty() {
            return Ok(());
        }

        let (chunks, raw_imports) = match file.language {
            crate::files::Language::TypeScript | crate::files::Language::JavaScript | crate::files::Language::JSX => {
                let chunker = crate::languages::typescript::TypeScriptChunker::new();
                let chunks = chunker.chunk(&content, relative);
                (chunks, chunker.extract_imports(&content))
            }
            crate::files::Language::TSX => {
                let chunker = crate::languages::typescript::TypeScriptChunker::tsx();
                let chunks = chunker.chunk(&content, relative);
                (chunks, chunker.extract_imports(&content))
            }
            crate::files::Language::Rust => {
                let chunker = crate::languages::rust::RustChunker::new();
                let chunks = chunker.chunk(&content, relative);
                (chunks, chunker.extract_imports(&content))
            }
            crate::files::Language::Python => {
                let chunker = crate::languages::python::PythonChunker::new();
                let chunks = chunker.chunk(&content, relative);
                (chunks, chunker.extract_imports(&content))
            }
            _ => {
                let chunks = FileChunker::chunk_file(&content, relative, file.language.as_str());
                (chunks, Vec::new())
            }
        };

        for chunk in &chunks {
            self.insert_chunk(chunk)?;
        }

        let mut imports = raw_imports;
        for imp in &mut imports {
            imp.source_file = relative.to_string();
        }
        let rels = crate::relationships::resolve_imports(self.db, &imports)?;
        crate::relationships::insert_relationships(self.db, &rels)?;

        Ok(())
    }

    fn insert_chunk(&self, chunk: &crate::chunk::Chunk) -> Result<(), Box<dyn std::error::Error>> {
        let metadata = if chunk.metadata.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&chunk.metadata)?)
        };

        self.db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, signature, line_start, line_end, content_md, content_hash, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                chunk.file_path,
                chunk.language,
                chunk.chunk_type.as_str(),
                chunk.name,
                chunk.signature,
                chunk.line_start as i64,
                chunk.line_end as i64,
                chunk.content_md,
                chunk.content_hash,
                metadata,
            ],
        )?;
        Ok(())
    }

    fn delete_file_chunks(&self, relative: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.db.execute("DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE file_path = ?1)", params![relative])?;
        self.db.execute("DELETE FROM relationships WHERE source_id IN (SELECT id FROM chunks WHERE file_path = ?1) OR target_id IN (SELECT id FROM chunks WHERE file_path = ?1)", params![relative])?;
        self.db.execute("DELETE FROM chunks WHERE file_path = ?1", params![relative])?;
        Ok(())
    }
}

struct FileChunker;

impl FileChunker {
    fn chunk_file(content: &str, relative: &str, language: &str) -> Vec<crate::chunk::Chunk> {
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

        let content_md = format!(
            "### {}\n\n**File:** `{}`\n**Lines:** {}-{}\n**Type:** section\n\n```\n{}\n```",
            name.as_deref().unwrap_or("(unnamed)"),
            relative,
            start + 1,
            end,
            content
        );

        crate::chunk::Chunk {
            file_path: relative.to_string(),
            language: language.to_string(),
            chunk_type: crate::chunk::ChunkType::Section,
            name,
            signature: None,
            line_start: start,
            line_end: end,
            content_md,
            content_hash: content_hash(content.as_bytes()),
            metadata: serde_json::Map::new(),
        }
    }
}
