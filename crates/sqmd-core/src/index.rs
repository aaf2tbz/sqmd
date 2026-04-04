use crate::chunker::LanguageChunker;
use crate::files::{SourceFile, walk_project};
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
    pub relationships_total: usize,
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
            relationships_total: 0,
        };

        self.db.execute_batch("BEGIN")?;

        let db_paths: Vec<String> = {
            let mut stmt = self.db.prepare("SELECT path FROM files")?;
            let rows: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
            drop(stmt);
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

            let content = std::fs::read_to_string(&file_path).unwrap_or_default();
            let count = self.insert_file(&relative, &source_file, &content)?;
            stats.files_indexed += 1;
            stats.relationships_total += count;
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

        self.db.execute_batch("COMMIT")?;

        Ok(stats)
    }

    fn insert_file(&self, relative: &str, file: &SourceFile, content: &str) -> Result<usize, Box<dyn std::error::Error>> {
        self.db.execute(
            "INSERT OR REPLACE INTO files (path, language, size, mtime, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![relative, file.language.as_str(), file.size as i64, file.mtime, file.hash],
        )?;

        if content.trim().is_empty() {
            return Ok(0);
        }

        let (chunks, raw_imports) = match file.language {
            crate::files::Language::TypeScript | crate::files::Language::JavaScript | crate::files::Language::JSX => {
                let chunker = crate::languages::typescript::TypeScriptChunker::new();
                let chunks = chunker.chunk(content, relative);
                (chunks, chunker.extract_imports(content))
            }
            crate::files::Language::TSX => {
                let chunker = crate::languages::typescript::TypeScriptChunker::tsx();
                let chunks = chunker.chunk(content, relative);
                (chunks, chunker.extract_imports(content))
            }
            crate::files::Language::Rust => {
                let chunker = crate::languages::rust::RustChunker::new();
                let chunks = chunker.chunk(content, relative);
                (chunks, chunker.extract_imports(content))
            }
            crate::files::Language::Python => {
                let chunker = crate::languages::python::PythonChunker::new();
                let chunks = chunker.chunk(content, relative);
                (chunks, chunker.extract_imports(content))
            }
            _ => {
                let chunks = crate::chunker::FileChunker::chunk_file(content, relative, file.language.as_str());
                (chunks, Vec::new())
            }
        };

        let mut parent_ids: Vec<(i64, i64)> = Vec::new();

        for chunk in &chunks {
            let chunk_id = self.insert_chunk(chunk)?;
            if matches!(
                chunk.chunk_type,
                crate::chunk::ChunkType::Class
                    | crate::chunk::ChunkType::Impl
                    | crate::chunk::ChunkType::Trait
                    | crate::chunk::ChunkType::Module
            ) {
                if let Some(id) = chunk_id {
                    parent_ids.push((id, chunk.line_end as i64));
                }
            }
            if matches!(chunk.chunk_type, crate::chunk::ChunkType::Method | crate::chunk::ChunkType::Constant | crate::chunk::ChunkType::Type) {
                if let Some(parent) = parent_ids.last() {
            if chunk.line_start as i64 >= parent.0 && chunk.line_end as i64 <= parent.1 {
                        if let (Some(cid), Some(pid)) = (chunk_id, Some(parent.0)) {
                            self.db.execute(
                                "INSERT OR IGNORE INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, 'contains')",
                                params![pid, cid],
                            )?;
                        }
                    }
                }
            }
        }

        let mut imports = raw_imports;
        for imp in &mut imports {
            imp.source_file = relative.to_string();
        }
        let rels = crate::relationships::resolve_imports(self.db, &imports)?;
        crate::relationships::insert_relationships(self.db, &rels)?;

        Ok(rels.len())
    }

    fn insert_chunk(&self, chunk: &crate::chunk::Chunk) -> Result<Option<i64>, Box<dyn std::error::Error>> {
        let metadata = if chunk.metadata.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&chunk.metadata)?)
        };

        self.db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance, metadata)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                chunk.file_path,
                chunk.language,
                chunk.chunk_type.as_str(),
                chunk.name,
                chunk.signature,
                chunk.line_start as i64,
                chunk.line_end as i64,
                chunk.content_raw,
                chunk.content_hash,
                chunk.importance,
                metadata,
            ],
        )?;
        Ok(Some(self.db.last_insert_rowid()))
    }

    fn delete_file_chunks(&self, relative: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.db.execute(
            "DELETE FROM relationships WHERE source_id IN (SELECT id FROM chunks WHERE file_path = ?1) OR target_id IN (SELECT id FROM chunks WHERE file_path = ?1)",
            params![relative],
        )?;
        self.db.execute("DELETE FROM chunks WHERE file_path = ?1", params![relative])?;
        Ok(())
    }
}
