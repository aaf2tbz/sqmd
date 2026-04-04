use crate::chunk;
use crate::chunker::LanguageChunker;
use crate::files::{content_hash, detect_language, walk_project, Language};
use crate::relationships::ImportInfo;
use crate::schema;
use rayon::prelude::*;
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

#[derive(Debug)]
pub struct FileIndexResult {
    pub file_path: String,
    pub chunks: usize,
    pub relationships: usize,
    pub was_deleted: bool,
}

#[derive(Debug, Clone)]
struct FileWork {
    relative: String,
    language: Language,
    size: u64,
    mtime: f64,
    hash: String,
    content: String,
}

fn chunk_file_content(
    language: &Language,
    content: &str,
    relative: &str,
) -> (Vec<chunk::Chunk>, Vec<ImportInfo>) {
    match language {
        Language::TypeScript | Language::JavaScript | Language::JSX => {
            let c = crate::languages::typescript::TypeScriptChunker::new();
            (c.chunk(content, relative), c.extract_imports(content))
        }
        Language::TSX => {
            let c = crate::languages::typescript::TypeScriptChunker::tsx();
            (c.chunk(content, relative), c.extract_imports(content))
        }
        Language::Rust => {
            let c = crate::languages::rust::RustChunker::new();
            (c.chunk(content, relative), c.extract_imports(content))
        }
        Language::Python => {
            let c = crate::languages::python::PythonChunker::new();
            (c.chunk(content, relative), c.extract_imports(content))
        }
        _ => (
            crate::chunker::FileChunker::chunk_file(content, relative, language.as_str()),
            Vec::new(),
        ),
    }
}

fn get_mtime(path: &Path) -> f64 {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
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

        let db_paths: Vec<String> = {
            let mut stmt = self.db.prepare("SELECT path FROM files")?;
            let rows: Vec<String> = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            rows
        };

        // Phase 1: walk + mtime pre-filter (serial, needs DB)
        let mut seen_paths = std::collections::HashSet::new();
        let mut candidates: Vec<(PathBuf, String)> = Vec::new();

        for file_path in walk_project(&self.root) {
            stats.files_scanned += 1;
            let relative = file_path
                .strip_prefix(&self.root)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();
            seen_paths.insert(relative.clone());

            let mtime = get_mtime(&file_path);

            let db_mtime: Option<f64> = self
                .db
                .query_row(
                    "SELECT mtime FROM files WHERE path = ?1",
                    params![relative],
                    |r| r.get(0),
                )
                .ok();

            if db_mtime.is_some_and(|m| (m - mtime).abs() < 0.001) {
                stats.files_skipped += 1;
                continue;
            }

            candidates.push((file_path, relative));
        }

        // Phase 2: read + hash (parallel I/O)
        let work_items: Vec<FileWork> = candidates
            .into_par_iter()
            .filter_map(|(abs_path, relative)| {
                let content = std::fs::read_to_string(&abs_path).ok()?;
                let hash = content_hash(content.as_bytes());
                let metadata = std::fs::metadata(&abs_path).ok()?;
                let language = detect_language(&abs_path);
                Some(FileWork {
                    relative,
                    language,
                    size: metadata.len(),
                    mtime: get_mtime(&abs_path),
                    hash,
                    content,
                })
            })
            .collect();

        // Phase 3: parallel chunking (CPU-bound)
        let chunked: Vec<(FileWork, Vec<chunk::Chunk>, Vec<ImportInfo>)> = work_items
            .into_par_iter()
            .map(|work| {
                let (chunks, imports) =
                    chunk_file_content(&work.language, &work.content, &work.relative);
                (work, chunks, imports)
            })
            .collect();

        // Phase 4: write to DB (serial, needs Connection)
        self.db.execute_batch("BEGIN")?;

        for (work, chunks, raw_imports) in &chunked {
            // mtime pre-filter can have false positives; verify with content hash
            let existing_hash: Option<String> = self
                .db
                .query_row(
                    "SELECT hash FROM files WHERE path = ?1",
                    params![work.relative],
                    |r| r.get(0),
                )
                .ok();

            if existing_hash.as_deref() == Some(&work.hash) {
                let _ = self.db.execute(
                    "UPDATE files SET mtime = ?1, indexed_at = datetime('now') WHERE path = ?2",
                    params![work.mtime, work.relative],
                );
                continue;
            }

            if existing_hash.is_some() {
                self.delete_file_chunks(&work.relative)?;
            }

            self.db.execute(
                "INSERT OR REPLACE INTO files (path, language, size, mtime, hash, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                params![work.relative, work.language.as_str(), work.size as i64, work.mtime, work.hash],
            )?;

            if work.content.trim().is_empty() {
                stats.files_indexed += 1;
                continue;
            }

            let mut rel_count = 0;
            rel_count += self.write_chunks_and_contains(chunks)?;
            rel_count += self.write_import_relationships(&work.relative, raw_imports)?;

            stats.files_indexed += 1;
            stats.relationships_total += rel_count;
        }

        // Handle deletions
        for db_path in &db_paths {
            if !seen_paths.contains(db_path) {
                self.delete_file_chunks(db_path)?;
                self.db
                    .execute("DELETE FROM files WHERE path = ?1", params![db_path])?;
                stats.files_deleted += 1;
            }
        }

        stats.chunks_total = self
            .db
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;

        self.db.execute_batch("COMMIT")?;
        Ok(stats)
    }

    pub fn index_file(
        &mut self,
        abs_path: &Path,
    ) -> Result<Option<FileIndexResult>, Box<dyn std::error::Error>> {
        let relative = abs_path
            .strip_prefix(&self.root)
            .unwrap_or(abs_path)
            .to_string_lossy()
            .to_string();

        let language = detect_language(abs_path);
        if !language.supported() {
            return Ok(None);
        }

        let content = match std::fs::read_to_string(abs_path) {
            Ok(c) => c,
            Err(_) => {
                self.db.execute_batch("BEGIN")?;
                self.delete_file_chunks(&relative)?;
                self.db
                    .execute("DELETE FROM files WHERE path = ?1", params![relative])?;
                self.db.execute_batch("COMMIT")?;
                return Ok(Some(FileIndexResult {
                    file_path: relative,
                    chunks: 0,
                    relationships: 0,
                    was_deleted: true,
                }));
            }
        };

        let hash = content_hash(content.as_bytes());

        let existing_hash: Option<String> = self
            .db
            .query_row(
                "SELECT hash FROM files WHERE path = ?1",
                params![relative],
                |r| r.get(0),
            )
            .ok();

        if existing_hash.as_deref() == Some(&hash) {
            return Ok(None);
        }

        let metadata = std::fs::metadata(abs_path)?;
        let mtime = get_mtime(abs_path);

        self.db.execute_batch("BEGIN")?;

        if existing_hash.is_some() {
            self.delete_file_chunks(&relative)?;
        }

        self.db.execute(
            "INSERT OR REPLACE INTO files (path, language, size, mtime, hash, indexed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![
                relative,
                language.as_str(),
                metadata.len() as i64,
                mtime,
                hash
            ],
        )?;

        let (chunks, raw_imports) = chunk_file_content(&language, &content, &relative);
        let mut rel_count = 0;
        rel_count += self.write_chunks_and_contains(&chunks)?;
        rel_count += self.write_import_relationships(&relative, &raw_imports)?;

        self.db.execute_batch("COMMIT")?;

        Ok(Some(FileIndexResult {
            file_path: relative,
            chunks: chunks.len(),
            relationships: rel_count,
            was_deleted: false,
        }))
    }

    fn write_chunks_and_contains(
        &self,
        chunks: &[chunk::Chunk],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        // Track parent containers: (chunk_id, line_start, line_end)
        let mut parent_stack: Vec<(i64, usize, usize)> = Vec::new();
        let mut rel_count = 0;

        for chunk in chunks {
            let chunk_id = self.insert_chunk(chunk)?;

            if matches!(
                chunk.chunk_type,
                chunk::ChunkType::Class
                    | chunk::ChunkType::Impl
                    | chunk::ChunkType::Trait
                    | chunk::ChunkType::Module
                    | chunk::ChunkType::Enum
                    | chunk::ChunkType::Struct
            ) {
                if let Some(id) = chunk_id {
                    parent_stack.truncate(
                        parent_stack
                            .iter()
                            .rposition(|(_, _, end)| chunk.line_start >= *end)
                            .map(|p| p + 1)
                            .unwrap_or(0),
                    );
                    parent_stack.push((id, chunk.line_start, chunk.line_end));
                }
            }

            if matches!(
                chunk.chunk_type,
                chunk::ChunkType::Method
                    | chunk::ChunkType::Constant
                    | chunk::ChunkType::Type
            ) {
                if let Some(&(pid, p_start, p_end)) = parent_stack.last() {
                    if chunk.line_start > p_start && chunk.line_end <= p_end {
                        if let Some(cid) = chunk_id {
                            self.db.execute(
                                "INSERT OR IGNORE INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, 'contains')",
                                params![pid, cid],
                            )?;
                            rel_count += 1;
                        }
                    }
                }
            }
        }

        Ok(rel_count)
    }

    fn write_import_relationships(
        &self,
        relative: &str,
        raw_imports: &[ImportInfo],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut imports = raw_imports.to_vec();
        for imp in &mut imports {
            imp.source_file = relative.to_string();
        }
        let rels = crate::relationships::resolve_imports(self.db, &imports)?;
        crate::relationships::insert_relationships(self.db, &rels)?;
        Ok(rels.len())
    }

    fn insert_chunk(
        &self,
        chunk: &chunk::Chunk,
    ) -> Result<Option<i64>, Box<dyn std::error::Error>> {
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
        self.db.execute(
            "DELETE FROM chunks WHERE file_path = ?1",
            params![relative],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_indexer(dir: &std::path::Path) -> Indexer<'static> {
        let db_path = dir.join(".sqmd/index.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();

        let conn = Connection::open(&db_path).unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL").ok();

        let db: &'static mut Connection = Box::leak(Box::new(conn));
        let mut indexer = Indexer::new(db, dir);
        indexer.init_index().unwrap();
        indexer
    }

    #[test]
    fn test_mtime_pre_filter_skips_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {}\n").unwrap();

        let mut indexer = make_indexer(dir.path());

        let stats1 = indexer.index().unwrap();
        assert_eq!(stats1.files_indexed, 1);
        assert_eq!(stats1.files_scanned, 1);

        let stats2 = indexer.index().unwrap();
        assert_eq!(stats2.files_indexed, 0);
        assert_eq!(stats2.files_skipped, 1);
    }

    #[test]
    fn test_index_detects_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("src/main.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "fn main() {}\n").unwrap();

        let mut indexer = make_indexer(dir.path());
        indexer.index().unwrap();

        std::fs::write(&file_path, "fn main() { println!(\"hello\"); }\n").unwrap();

        let stats = indexer.index().unwrap();
        assert_eq!(stats.files_indexed, 1);
    }

    #[test]
    fn test_index_file_single() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("lib.rs");
        std::fs::write(&file_path, "fn hello() {}\nfn world() {}\n").unwrap();

        let mut indexer = make_indexer(dir.path());

        let result = indexer.index_file(&file_path).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().chunks, 2);

        let result = indexer.index_file(&file_path).unwrap();
        assert!(result.is_none());

        std::fs::write(&file_path, "fn hello() { println!(); }\nfn world() {}\nfn new() {}\n")
            .unwrap();
        let result = indexer.index_file(&file_path).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().chunks, 3);
    }

    #[test]
    fn test_index_file_deletes_removed_file() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("src/util.rs");
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(&file_path, "pub fn helper() {}\n").unwrap();

        let mut indexer = make_indexer(dir.path());
        let result = indexer.index_file(&file_path).unwrap();
        assert!(result.is_some());

        std::fs::remove_file(&file_path).unwrap();
        let result = indexer.index_file(&file_path).unwrap().unwrap();
        assert!(result.was_deleted);
    }

    #[test]
    fn test_parallel_index_consistency() {
        let dir = tempfile::tempdir().unwrap();

        for i in 0..20 {
            let path = dir.path().join(format!("src/mod_{i}.rs"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let content = format!("pub fn func_{i}() {{ println!({i}); }}\n");
            std::fs::write(&path, content).unwrap();
        }

        let mut indexer = make_indexer(dir.path());
        let stats = indexer.index().unwrap();

        assert_eq!(stats.files_indexed, 20);
        assert_eq!(stats.files_scanned, 20);

        let stats2 = indexer.index().unwrap();
        assert_eq!(stats2.files_indexed, 0);
        assert_eq!(stats2.files_skipped, 20);
    }
}
