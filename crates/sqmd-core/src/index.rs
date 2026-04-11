use crate::chunk;
use crate::chunker::LanguageChunker;
use crate::entities;
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
    pub decisions: IndexDecisions,
}

#[derive(Debug, Default)]
pub struct IndexDecisions {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub tombstoned: usize,
}

#[derive(Debug)]
pub struct FileIndexResult {
    pub file_path: String,
    pub chunks: usize,
    pub relationships: usize,
    pub was_deleted: bool,
    pub decisions: IndexDecisions,
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
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::TSX => {
            let c = crate::languages::typescript::TypeScriptChunker::tsx();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Rust => {
            let c = crate::languages::rust::RustChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Python => {
            let c = crate::languages::python::PythonChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Go => {
            let c = crate::languages::go::GoChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Java => {
            let c = crate::languages::java::JavaChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::C => {
            let c = crate::languages::c::CChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Cpp => {
            let c = crate::languages::cpp::CppChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::CMake => {
            let c = crate::languages::cmake::CMakeChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Qml => {
            let c = crate::languages::qml::QmlChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Meson => {
            let chunks = crate::languages::meson::MesonChunker::chunk(content, relative);
            (chunks, Vec::new())
        }
        Language::Ruby => {
            let c = crate::languages::ruby::RubyChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Yaml => {
            let c = crate::languages::yaml::YamlChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Json => {
            let c = crate::languages::json::JsonChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Toml => {
            let c = crate::languages::toml::TomlChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Html => {
            let c = crate::languages::html::HtmlChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Css => {
            let c = crate::languages::css::CssChunker::new();
            let (chunks, tree) = c.chunk(content, relative);
            let imports = tree
                .map(|t| c.extract_imports(&t, content))
                .unwrap_or_default();
            (chunks, imports)
        }
        Language::Scss => {
            let chunks = crate::chunker::FileChunker::chunk_file(content, relative, "scss");
            (chunks, Vec::new())
        }
        Language::Markdown => {
            let chunks = crate::languages::markdown::MarkdownChunker::chunk(content, relative);
            (chunks, Vec::new())
        }
        Language::Shell
        | Language::Sql
        | Language::Dockerfile
        | Language::Makefile
        | Language::Kotlin
        | Language::Swift
        | Language::CSharp
        | Language::Php
        | Language::Lua
        | Language::Dart
        | Language::Scala
        | Language::Haskell
        | Language::Elixir
        | Language::Zig
        | Language::Xml
        | Language::GraphQL
        | Language::Protobuf => {
            let chunks =
                crate::chunker::FileChunker::chunk_file(content, relative, language.as_str());
            (chunks, Vec::new())
        }
        _ => (
            crate::chunker::FileChunker::chunk_file(content, relative, language.as_str()),
            Vec::new(),
        ),
    }
}

fn extract_structural_rels_for_file(
    language: &Language,
    content: &str,
) -> Vec<crate::relationships::StructuralRelation> {
    let (result, source_code) = match language.as_str() {
        "rust" => {
            let c = crate::languages::rust::RustChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "typescript" | "javascript" | "tsx" | "jsx" => {
            let c = if *language == Language::TSX {
                crate::languages::typescript::TypeScriptChunker::tsx()
            } else {
                crate::languages::typescript::TypeScriptChunker::new()
            };
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "python" => {
            let c = crate::languages::python::PythonChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "go" => {
            let c = crate::languages::go::GoChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "java" => {
            let c = crate::languages::java::JavaChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "c" => {
            let c = crate::languages::c::CChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "cpp" => {
            let c = crate::languages::cpp::CppChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        "ruby" => {
            let c = crate::languages::ruby::RubyChunker::new();
            let (_, tree) = c.chunk(content, "");
            let rels = tree
                .as_ref()
                .map(|t| c.extract_structural_rels(t, content))
                .unwrap_or_default();
            (rels, content.to_string())
        }
        _ => (Vec::new(), content.to_string()),
    };
    let _ = source_code;
    result
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
            decisions: IndexDecisions::default(),
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

        let mut pending_imports: Vec<(String, Vec<ImportInfo>)> = Vec::new();

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
                self.tombstone_file_chunks(&work.relative)?;
            }

            self.db.execute(
                "INSERT OR REPLACE INTO files (path, language, size, mtime, hash, indexed_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
                params![
                    work.relative,
                    work.language.as_str(),
                    work.size as i64,
                    work.mtime,
                    work.hash
                ],
            )?;

            if work.content.trim().is_empty() {
                stats.files_indexed += 1;
                continue;
            }

            let mut file_decisions = IndexDecisions::default();
            let mut rel_count = 0;
            rel_count += self.decision_write_chunks(&work.relative, chunks, &mut file_decisions)?;
            pending_imports.push((work.relative.clone(), raw_imports.clone()));
            self.build_entity_graph(&work.relative, chunks)?;
            let structural_rels = extract_structural_rels_for_file(&work.language, &work.content);
            stats.relationships_total += self.write_structural_rels(&structural_rels)?;

            stats.files_indexed += 1;
            stats.relationships_total += rel_count;
            stats.decisions.added += file_decisions.added;
            stats.decisions.updated += file_decisions.updated;
            stats.decisions.skipped += file_decisions.skipped;
        }

        for (relative, raw_imports) in &pending_imports {
            stats.relationships_total += self.write_import_relationships(relative, raw_imports)?;
        }

        // Handle deletions
        for db_path in &db_paths {
            if !seen_paths.contains(db_path) {
                let tombstoned = entities::tombstone_chunks(self.db, db_path)?;
                stats.decisions.tombstoned += tombstoned;
                self.db
                    .execute("DELETE FROM files WHERE path = ?1", params![db_path])?;
                stats.files_deleted += 1;
            }
        }

        stats.chunks_total = self
            .db
            .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;

        crate::relationships::materialize_entity_deps_to_relationships(self.db)?;
        entities::generate_relational_hints(self.db)?;
        crate::communities::ensure_graph_communities(self.db)?;

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
                let deleted_count = self.delete_file_chunks(&relative)?;
                self.db
                    .execute("DELETE FROM files WHERE path = ?1", params![relative])?;
                let _ = crate::episodes::record_episode(
                    self.db,
                    &relative,
                    "deleted",
                    None,
                    None,
                    deleted_count,
                );
                self.db.execute_batch("COMMIT")?;
                return Ok(Some(FileIndexResult {
                    file_path: relative,
                    chunks: 0,
                    relationships: 0,
                    was_deleted: true,
                    decisions: IndexDecisions::default(),
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
            self.tombstone_file_chunks(&relative)?;
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
        let mut file_decisions = IndexDecisions::default();
        let mut rel_count = 0;
        rel_count += self.decision_write_chunks(&relative, &chunks, &mut file_decisions)?;
        rel_count += self.write_import_relationships(&relative, &raw_imports)?;
        self.build_entity_graph(&relative, &chunks)?;

        let change_type = if existing_hash.is_some() {
            "modified"
        } else {
            "added"
        };
        let _ = crate::episodes::record_episode(
            self.db,
            &relative,
            change_type,
            None,
            None,
            chunks.len() as i64,
        );

        self.db.execute_batch("COMMIT")?;

        Ok(Some(FileIndexResult {
            file_path: relative,
            chunks: chunks.len(),
            relationships: rel_count,
            was_deleted: false,
            decisions: file_decisions,
        }))
    }

    fn decision_write_chunks(
        &self,
        relative: &str,
        chunks: &[chunk::Chunk],
        decisions: &mut IndexDecisions,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let old_hashes: std::collections::HashMap<String, (i64, Option<String>)> = {
            let mut stmt = self.db.prepare(
                "SELECT id, content_hash, name FROM chunks WHERE file_path = ?1 AND is_deleted = 0",
            )?;
            let rows: Vec<(i64, String, Option<String>)> = stmt
                .query_map(params![relative], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<Result<_, _>>()?;
            rows.into_iter()
                .map(|(id, hash, name)| (hash, (id, name)))
                .collect()
        };

        let new_hashes: std::collections::HashMap<String, usize> = chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| !c.content_hash.is_empty())
            .map(|(i, c)| (c.content_hash.clone(), i))
            .collect();

        let mut used_old_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();
        let mut chunk_id_map: std::collections::HashMap<usize, i64> =
            std::collections::HashMap::new();

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let chunk_hash = &chunk.content_hash;

            if let Some((old_id, old_name)) = old_hashes.get(chunk_hash) {
                let same_name = old_name.as_deref() == chunk.name.as_deref();
                if same_name && new_hashes.get(chunk_hash) == Some(&chunk_idx) {
                    let importance = entities::compute_structural_importance(
                        self.db,
                        *old_id,
                        chunk.importance,
                    )?;
                    if importance != chunk.importance {
                        self.db.execute(
                            "UPDATE chunks SET line_start = ?1, line_end = ?2, importance = ?3, updated_at = datetime('now') WHERE id = ?4",
                            params![chunk.line_start as i64, chunk.line_end as i64, importance, old_id],
                        )?;
                        decisions.updated += 1;
                    } else {
                        decisions.skipped += 1;
                    }
                    chunk_id_map.insert(chunk_idx, *old_id);
                    used_old_ids.insert(*old_id);
                    continue;
                }
            }

            if let Some((old_id, _)) = old_hashes.get(chunk_hash) {
                chunk_id_map.insert(chunk_idx, *old_id);
                used_old_ids.insert(*old_id);
                decisions.skipped += 1;
                continue;
            }

            let new_id = self.insert_chunk(chunk)?;
            if let Some(id) = new_id {
                chunk_id_map.insert(chunk_idx, id);
            }
            decisions.added += 1;
        }

        let stale_ids: Vec<i64> = old_hashes
            .iter()
            .filter(|(_, (old_id, _))| !used_old_ids.contains(old_id))
            .map(|(_, (old_id, _))| *old_id)
            .collect();

        if !stale_ids.is_empty() {
            let ph: Vec<String> = (0..stale_ids.len())
                .map(|i| format!("?{}", i + 1))
                .collect();
            let ph = ph.join(", ");
            self.db.execute(
                &format!("UPDATE chunks SET is_deleted = 1, deleted_at = datetime('now') WHERE id IN ({ph})"),
                rusqlite::params_from_iter(stale_ids.iter()),
            )?;
            decisions.tombstoned += stale_ids.len();
        }

        let rel_count = self.build_relationships(relative, chunks, &chunk_id_map)?;

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            if let Some(&chunk_id) = chunk_id_map.get(&chunk_idx) {
                let hints = entities::generate_hints(
                    chunk.name.as_deref(),
                    chunk.chunk_type.as_str(),
                    &chunk.content_raw,
                    &chunk.file_path,
                    "code",
                );
                if !hints.is_empty() {
                    self.db
                        .execute("DELETE FROM hints WHERE chunk_id = ?1", params![chunk_id])?;
                    entities::insert_hints(self.db, chunk_id, &hints)?;
                }

                let importance =
                    entities::compute_structural_importance(self.db, chunk_id, chunk.importance)?;
                if importance != chunk.importance {
                    self.db.execute(
                        "UPDATE chunks SET importance = ?1 WHERE id = ?2",
                        params![importance, chunk_id],
                    )?;
                }
            }
        }

        Ok(rel_count)
    }

    fn build_relationships(
        &self,
        _relative: &str,
        chunks: &[chunk::Chunk],
        chunk_id_map: &std::collections::HashMap<usize, i64>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let mut rel_count = 0;
        let mut parent_stack: Vec<(i64, usize, usize)> = Vec::new();

        for (chunk_idx, chunk) in chunks.iter().enumerate() {
            let chunk_id = chunk_id_map.get(&chunk_idx).copied();

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
                chunk::ChunkType::Method | chunk::ChunkType::Constant | chunk::ChunkType::Type
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

        let mut name_to_id: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for (idx, id) in chunk_id_map {
            if let Some(ref name) = chunks[*idx].name {
                name_to_id.insert(name.clone(), *id);
            }
        }

        let all_ids: Vec<i64> = chunk_id_map.values().copied().collect();
        let imported_names: Vec<String> = if all_ids.is_empty() {
            Vec::new()
        } else {
            let placeholders: Vec<String> =
                (0..all_ids.len()).map(|i| format!("?{}", i + 1)).collect();
            let sql = format!(
                "SELECT DISTINCT c.name FROM relationships r
                 JOIN chunks c ON r.target_id = c.id
                 WHERE r.source_id IN ({}) AND r.rel_type = 'imports' AND c.name IS NOT NULL",
                placeholders.join(", ")
            );
            let mut stmt = self.db.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::ToSql> = all_ids
                .iter()
                .map(|id| id as &dyn rusqlite::ToSql)
                .collect();
            let rows = stmt
                .query_map(params.as_slice(), |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            rows
        };

        for name in &imported_names {
            if !name_to_id.contains_key(name.as_str()) {
                if let Ok(Some(id)) = self.db.query_row(
                    "SELECT id FROM chunks WHERE name = ?1 AND chunk_type IN ('function', 'method', 'class', 'struct', 'enum', 'interface', 'trait', 'constant') LIMIT 1",
                    params![name],
                    |r| r.get(0),
                ) {
                    name_to_id.insert(name.clone(), id);
                }
            }
        }

        for (idx, caller_id) in chunk_id_map {
            let caller_id = *caller_id;
            let c = &chunks[*idx];
            if !matches!(
                c.chunk_type,
                chunk::ChunkType::Function | chunk::ChunkType::Method | chunk::ChunkType::Constant
            ) {
                continue;
            }

            for call in crate::relationships::extract_calls(&c.content_raw) {
                if let Some(&target_id) = name_to_id.get(&call) {
                    if target_id != caller_id {
                        self.db.execute(
                            "INSERT OR IGNORE INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, 'calls')",
                            params![caller_id, target_id],
                        )?;
                        rel_count += 1;
                    }
                }
            }
        }

        Ok(rel_count)
    }

    fn build_entity_graph(
        &self,
        relative: &str,
        chunks: &[chunk::Chunk],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let file_entity_id = entities::ensure_entity(self.db, relative, "file")?;

        let export_aspect_id = entities::ensure_aspect(self.db, file_entity_id, "exports")?;

        for chunk in chunks {
            let name = chunk.name.as_deref().unwrap_or("");
            if name.is_empty() {
                continue;
            }

            let entity_type = match chunk.chunk_type {
                chunk::ChunkType::Function => "function",
                chunk::ChunkType::Method => "method",
                chunk::ChunkType::Class => "class",
                chunk::ChunkType::Struct => "struct",
                chunk::ChunkType::Interface => "interface",
                chunk::ChunkType::Trait => "trait",
                chunk::ChunkType::Enum => "enum",
                chunk::ChunkType::Impl => "impl",
                chunk::ChunkType::Constant => "constant",
                chunk::ChunkType::Macro => "macro",
                chunk::ChunkType::Type => "type",
                chunk::ChunkType::Module => "module",
                _ => continue,
            };

            let chunk_id: Option<i64> = self.db.query_row(
                "SELECT id FROM chunks WHERE file_path = ?1 AND content_hash = ?2 AND is_deleted = 0 LIMIT 1",
                params![relative, chunk.content_hash],
                |r| r.get(0),
            ).ok();

            if let Some(cid) = chunk_id {
                let symbol_input = entities::SymbolEntityInput {
                    name,
                    entity_type,
                    file_path: relative,
                    language: &chunk.language,
                    line_start: chunk.line_start as i64,
                    line_end: chunk.line_end as i64,
                    signature: chunk.signature.as_deref(),
                    chunk_id: Some(cid),
                };
                entities::ensure_symbol_entity(self.db, &symbol_input)?;

                entities::add_attribute(
                    self.db,
                    file_entity_id,
                    Some(export_aspect_id),
                    cid,
                    "attribute",
                    &format!("{}: {}", entity_type, name),
                )?;
            }
        }

        Ok(())
    }

    fn write_structural_rels(
        &self,
        rels: &[crate::relationships::StructuralRelation],
    ) -> Result<usize, Box<dyn std::error::Error>> {
        if rels.is_empty() {
            return Ok(0);
        }

        let mut count = 0;
        let mut stmt = self.db.prepare(
            "INSERT OR IGNORE INTO entity_dependencies (source_entity, target_entity, dep_type, valid_from)
             VALUES (
                 (SELECT id FROM entities WHERE canonical_name = ?1),
                 (SELECT id FROM entities WHERE canonical_name = ?2),
                 ?3,
                 datetime('now')
             )",
        )?;

        for rel in rels {
            let src_canon = entities::canonicalize(&rel.source_name);
            let tgt_canon = entities::canonicalize(&rel.target_name);
            if src_canon.is_empty() || tgt_canon.is_empty() {
                continue;
            }
            stmt.execute(params![src_canon, tgt_canon, &rel.rel_type])?;
            count += 1;
        }

        Ok(count)
    }

    fn tombstone_file_chunks(&self, relative: &str) -> Result<(), Box<dyn std::error::Error>> {
        let chunk_ids: Vec<i64> = {
            let mut stmt = self
                .db
                .prepare("SELECT id FROM chunks WHERE file_path = ?1 AND is_deleted = 0")?;
            let ids: Vec<i64> = stmt
                .query_map(params![relative], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            ids
        };

        if chunk_ids.is_empty() {
            return Ok(());
        }

        let placeholders: Vec<String> = (0..chunk_ids.len())
            .map(|i| format!("?{}", i + 1))
            .collect();
        let ph = placeholders.join(", ");
        let ph2: Vec<String> = (0..chunk_ids.len())
            .map(|i| format!("?{}", i + 1 + chunk_ids.len()))
            .collect();
        let ph2 = ph2.join(", ");

        self.db.execute(
            &format!("DELETE FROM hints WHERE chunk_id IN ({ph})"),
            rusqlite::params_from_iter(chunk_ids.iter()),
        )?;
        self.db.execute(
            &format!("DELETE FROM entity_attributes WHERE chunk_id IN ({ph})"),
            rusqlite::params_from_iter(chunk_ids.iter()),
        )?;
        self.db.execute(
            &format!("DELETE FROM relationships WHERE source_id IN ({ph}) OR target_id IN ({ph2})"),
            rusqlite::params_from_iter(chunk_ids.iter().chain(chunk_ids.iter())),
        )?;

        entities::tombstone_chunks(self.db, relative)?;
        Ok(())
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

        let tags_json = chunk
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());

        self.db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance, metadata, source_type, agent_id, tags, decay_rate, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
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
                chunk.source_type.as_str(),
                chunk.agent_id,
                tags_json,
                chunk.decay_rate,
                chunk.created_by,
            ],
        )?;
        Ok(Some(self.db.last_insert_rowid()))
    }

    fn delete_file_chunks(&self, relative: &str) -> Result<i64, Box<dyn std::error::Error>> {
        let chunk_ids: Vec<i64> = {
            let mut stmt = self
                .db
                .prepare("SELECT id FROM chunks WHERE file_path = ?1")?;
            let ids: Vec<i64> = stmt
                .query_map(params![relative], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            drop(stmt);
            ids
        };

        let count = chunk_ids.len() as i64;

        if !chunk_ids.is_empty() {
            let placeholders: Vec<String> = (0..chunk_ids.len())
                .map(|i| format!("?{}", i + 1))
                .collect();
            let ph = placeholders.join(", ");

            self.db.execute(
                &format!("DELETE FROM hints WHERE chunk_id IN ({ph})"),
                rusqlite::params_from_iter(chunk_ids.iter()),
            )?;
            self.db.execute(
                &format!("DELETE FROM entity_attributes WHERE chunk_id IN ({ph})"),
                rusqlite::params_from_iter(chunk_ids.iter()),
            )?;
            self.db.execute(
                &format!("UPDATE entities SET chunk_id = NULL WHERE chunk_id IN ({ph})"),
                rusqlite::params_from_iter(chunk_ids.iter()),
            )?;
            let ph2: Vec<String> = (0..chunk_ids.len())
                .map(|i| format!("?{}", i + 1 + chunk_ids.len()))
                .collect();
            let ph2 = ph2.join(", ");

            self.db.execute(
                &format!(
                    "DELETE FROM relationships WHERE source_id IN ({ph}) OR target_id IN ({ph2})"
                ),
                rusqlite::params_from_iter(chunk_ids.iter().chain(chunk_ids.iter())),
            )?;
            self.db
                .execute("DELETE FROM chunks WHERE file_path = ?1", params![relative])?;
        }
        Ok(count)
    }
}

// ── Knowledge Ingestor ──────────────────────────────────────────────
// Accepts pre-chunked knowledge (memories, facts, transcripts, etc.)
// from external systems like Signet's pipeline.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct KnowledgeChunk {
    pub content: String,
    pub chunk_type: String,
    pub source_type: String,
    pub name: Option<String>,
    pub importance: Option<f64>,
    pub agent_id: Option<String>,
    pub tags: Option<Vec<String>>,
    pub decay_rate: Option<f64>,
    pub created_by: Option<String>,
    pub metadata: Option<serde_json::Map<String, serde_json::Value>>,
    /// Optional relationships to create: Vec<(target_chunk_id, rel_type)>
    pub relationships: Option<Vec<(i64, String)>>,
}

#[derive(Debug, serde::Serialize)]
pub struct IngestResult {
    pub chunk_id: i64,
    pub content_hash: String,
    pub was_duplicate: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct IngestBatchResult {
    pub ingested: usize,
    pub duplicates: usize,
    pub results: Vec<IngestResult>,
}

pub struct KnowledgeIngestor<'a> {
    db: &'a Connection,
}

impl<'a> KnowledgeIngestor<'a> {
    pub fn new(db: &'a Connection) -> Self {
        Self { db }
    }

    pub fn ingest(
        &self,
        input: &KnowledgeChunk,
    ) -> Result<IngestResult, Box<dyn std::error::Error>> {
        let chunk_type =
            chunk::ChunkType::from_str_name(&input.chunk_type).unwrap_or(chunk::ChunkType::Fact);
        let source_type = chunk::SourceType::from_str_name(&input.source_type)
            .unwrap_or(chunk::SourceType::Memory);
        let importance = input.importance.unwrap_or_else(|| chunk_type.importance());

        let mut c = chunk::Chunk::knowledge(
            chunk_type,
            source_type,
            input.name.clone(),
            input.content.clone(),
            importance,
        );
        c.agent_id = input.agent_id.clone();
        c.tags = input.tags.clone();
        c.decay_rate = input.decay_rate.unwrap_or(0.0);
        c.created_by = input.created_by.clone();
        if let Some(ref m) = input.metadata {
            c.metadata = m.clone();
        }

        // Check for duplicate by content hash
        let existing: Option<i64> = self.db.query_row(
            "SELECT id FROM chunks WHERE content_hash = ?1 AND source_type = ?2 AND is_deleted = 0",
            params![c.content_hash, source_type.as_str()],
            |r| r.get(0),
        ).ok();

        if let Some(existing_id) = existing {
            // Update last_accessed and importance if higher
            self.db.execute(
                "UPDATE chunks SET last_accessed = datetime('now'), importance = MAX(importance, ?1) WHERE id = ?2",
                params![importance, existing_id],
            )?;
            return Ok(IngestResult {
                chunk_id: existing_id,
                content_hash: c.content_hash,
                was_duplicate: true,
            });
        }

        // Ensure a synthetic file entry exists for this source type
        let file_path = &c.file_path;
        self.db.execute(
            "INSERT OR IGNORE INTO files (path, language, size, mtime, hash) VALUES (?1, '', 0, 0.0, ?1)",
            params![file_path],
        )?;

        // Insert the chunk
        let tags_json = c
            .tags
            .as_ref()
            .map(|t| serde_json::to_string(t).unwrap_or_default());
        let metadata_json = if c.metadata.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&c.metadata)?)
        };

        self.db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance, metadata, source_type, agent_id, tags, decay_rate, created_by)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                c.file_path, c.language, c.chunk_type.as_str(), c.name, c.signature,
                c.line_start as i64, c.line_end as i64, c.content_raw, c.content_hash,
                c.importance, metadata_json, c.source_type.as_str(), c.agent_id,
                tags_json, c.decay_rate, c.created_by,
            ],
        )?;

        let chunk_id = self.db.last_insert_rowid();

        // Generate hints for the knowledge chunk
        let hints = crate::entities::generate_hints(
            c.name.as_deref(),
            c.chunk_type.as_str(),
            &c.content_raw,
            &c.file_path,
            c.source_type.as_str(),
        );
        if !hints.is_empty() {
            crate::entities::insert_hints(self.db, chunk_id, &hints)?;
        }

        // Create any specified relationships
        if let Some(ref rels) = input.relationships {
            for (target_id, rel_type) in rels {
                self.db.execute(
                    "INSERT OR IGNORE INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, ?3)",
                    params![chunk_id, target_id, rel_type],
                )?;
            }
        }

        Ok(IngestResult {
            chunk_id,
            content_hash: c.content_hash,
            was_duplicate: false,
        })
    }

    pub fn ingest_batch(
        &self,
        inputs: &[KnowledgeChunk],
    ) -> Result<IngestBatchResult, Box<dyn std::error::Error>> {
        self.db.execute_batch("BEGIN")?;
        let mut results = Vec::with_capacity(inputs.len());
        let mut ingested = 0;
        let mut duplicates = 0;

        for input in inputs {
            match self.ingest(input) {
                Ok(result) => {
                    if result.was_duplicate {
                        duplicates += 1;
                    } else {
                        ingested += 1;
                    }
                    results.push(result);
                }
                Err(e) => {
                    self.db.execute_batch("ROLLBACK")?;
                    return Err(e);
                }
            }
        }

        if results.len() > 1 {
            let _ = self.generate_session_summary(&results, inputs);
        }

        self.db.execute_batch("COMMIT")?;
        Ok(IngestBatchResult {
            ingested,
            duplicates,
            results,
        })
    }

    fn generate_session_summary(
        &self,
        ingest_results: &[IngestResult],
        inputs: &[KnowledgeChunk],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let first_source_type = inputs
            .first()
            .and_then(|i| crate::chunk::SourceType::from_str_name(&i.source_type))
            .unwrap_or(crate::chunk::SourceType::Memory);

        let first_name = inputs
            .first()
            .and_then(|i| i.name.as_deref())
            .unwrap_or("Session");

        let max_importance = inputs
            .iter()
            .filter_map(|i| i.importance)
            .fold(0.8f64, f64::max);

        let summary_path = {
            let paths: Vec<String> = ingest_results
                .iter()
                .map(|r| {
                    self.db
                        .query_row(
                            "SELECT file_path FROM chunks WHERE id = ?1",
                            rusqlite::params![r.chunk_id],
                            |row| row.get::<_, String>(0),
                        )
                        .unwrap_or_default()
                })
                .collect();
            if paths.is_empty() {
                String::from("signet://summary")
            } else if paths.len() == 1 {
                format!("{}/summary", paths[0])
            } else {
                let first = &paths[0];
                let mut prefix_len = 0;
                for (i, c) in first.chars().enumerate() {
                    if paths.iter().all(|p| p.chars().nth(i) == Some(c)) {
                        prefix_len = i + 1;
                    } else {
                        break;
                    }
                }
                let prefix = &first[..prefix_len];
                if prefix.is_empty() {
                    format!("{}/summary", paths[0])
                } else {
                    format!("{}summary", prefix)
                }
            }
        };

        let mut content_parts: Vec<String> = Vec::new();
        for (input, result) in inputs.iter().zip(ingest_results.iter()) {
            if result.was_duplicate {
                continue;
            }
            let name_part = input.name.as_deref().unwrap_or("(unnamed)");
            let preview: String = if input.content.len() > 200 {
                input.content[..200].to_string()
            } else {
                input.content.clone()
            };
            content_parts.push(format!("{}: {}", name_part, preview));
        }

        let summary_content: String = content_parts.join("\n");
        let summary_content = if summary_content.len() > 4000 {
            summary_content[..4000].to_string()
        } else {
            summary_content
        };

        let mut summary_input = KnowledgeChunk {
            content: summary_content,
            chunk_type: "summary".to_string(),
            source_type: first_source_type.as_str().to_string(),
            name: Some(format!("{} Summary", first_name)),
            importance: Some(max_importance),
            agent_id: inputs.iter().filter_map(|i| i.agent_id.clone()).next(),
            tags: None,
            decay_rate: None,
            created_by: None,
            metadata: None,
            relationships: None,
        };
        summary_input.content = format!("{}\n\n{}", summary_path, summary_input.content);

        let summary_result = self.ingest(&summary_input)?;
        if !summary_result.was_duplicate {
            for ingest_result in ingest_results {
                if !ingest_result.was_duplicate {
                    self.db.execute(
                        "INSERT OR IGNORE INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, 'contains')",
                        rusqlite::params![summary_result.chunk_id, ingest_result.chunk_id],
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Soft-delete a knowledge chunk by ID
    pub fn forget(&self, chunk_id: i64) -> Result<bool, Box<dyn std::error::Error>> {
        let count = self.db.execute(
            "UPDATE chunks SET is_deleted = 1, deleted_at = datetime('now') WHERE id = ?1 AND source_type != 'code'",
            params![chunk_id],
        )?;
        Ok(count > 0)
    }

    /// Update importance/tags on a knowledge chunk
    pub fn modify(
        &self,
        chunk_id: i64,
        importance: Option<f64>,
        tags: Option<Vec<String>>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        if let Some(imp) = importance {
            self.db.execute(
                "UPDATE chunks SET importance = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![imp, chunk_id],
            )?;
        }
        if let Some(ref t) = tags {
            let json = serde_json::to_string(t)?;
            self.db.execute(
                "UPDATE chunks SET tags = ?1, updated_at = datetime('now') WHERE id = ?2",
                params![json, chunk_id],
            )?;
        }
        Ok(true)
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

        std::fs::write(
            &file_path,
            "fn hello() { println!(); }\nfn world() {}\nfn new() {}\n",
        )
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

    #[test]
    fn test_index_resolves_imports_after_all_chunks_are_written() {
        let dir = tempfile::tempdir().unwrap();
        let importer = dir.path().join("src/a_importer.ts");
        let target = dir.path().join("src/z_target.ts");
        std::fs::create_dir_all(importer.parent().unwrap()).unwrap();
        std::fs::write(
            &importer,
            "import {\n  targetFunction,\n} from './z_target';\n\nexport function caller() { return targetFunction(); }\n",
        )
        .unwrap();
        std::fs::write(
            &target,
            "export function targetFunction(): string { return 'ok'; }\n",
        )
        .unwrap();

        let mut indexer = make_indexer(dir.path());
        let stats = indexer.index().unwrap();
        assert_eq!(stats.files_indexed, 2);

        let deps = crate::relationships::get_dependencies(indexer.db, "src/a_importer.ts").unwrap();
        assert!(
            deps.iter().any(|dep| dep.target_file == "src/z_target.ts"
                && dep.target_name.as_deref() == Some("targetFunction")),
            "deps: {deps:?}",
        );
    }
}
