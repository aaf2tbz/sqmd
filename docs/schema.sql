-- sqmd SQLite Schema
-- Source: https://github.com/user/sqmd
-- Version: 0.1.0

PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;

-- ============================================================
-- Source file metadata
-- ============================================================

CREATE TABLE IF NOT EXISTS files (
    path       TEXT PRIMARY KEY,
    language   TEXT NOT NULL,
    size       INTEGER NOT NULL,
    mtime      REAL NOT NULL,
    hash       TEXT NOT NULL,
    indexed_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_files_hash ON files(hash);
CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);

-- ============================================================
-- Semantic code chunks
-- ============================================================

CREATE TABLE IF NOT EXISTS chunks (
    id           INTEGER PRIMARY KEY,
    file_path    TEXT NOT NULL REFERENCES files(path) ON DELETE CASCADE,
    language     TEXT NOT NULL,
    chunk_type   TEXT NOT NULL CHECK(chunk_type IN (
        'function', 'method', 'class', 'interface', 'type',
        'module', 'section', 'import', 'export', 'macro',
        'trait', 'impl', 'enum', 'struct', 'constant'
    )),
    name         TEXT,
    signature    TEXT,
    line_start   INTEGER NOT NULL,
    line_end     INTEGER NOT NULL,
    content_md   TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    metadata     TEXT,
    importance   REAL NOT NULL DEFAULT 0.5 CHECK(importance >= 0.0 AND importance <= 1.0),
    created_at   TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at   TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);
CREATE INDEX IF NOT EXISTS idx_chunks_type ON chunks(chunk_type);
CREATE INDEX IF NOT EXISTS idx_chunks_name ON chunks(name);
CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(content_hash);
CREATE INDEX IF NOT EXISTS idx_chunks_importance ON chunks(importance);
CREATE INDEX IF NOT EXISTS idx_chunks_lines ON chunks(file_path, line_start, line_end);

-- ============================================================
-- Import / call relationship graph
-- ============================================================

CREATE TABLE IF NOT EXISTS relationships (
    id       INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    rel_type  TEXT NOT NULL CHECK(rel_type IN (
        'imports', 'calls', 'contains', 'implements',
        'overrides', 'extends', 'references'
    )),
    metadata  TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    UNIQUE(source_id, target_id, rel_type)
);

CREATE INDEX IF NOT EXISTS idx_rels_source ON relationships(source_id, rel_type);
CREATE INDEX IF NOT EXISTS idx_rels_target ON relationships(target_id, rel_type);

-- ============================================================
-- Vector embeddings (BLOB fallback storage)
-- ============================================================

CREATE TABLE IF NOT EXISTS embeddings (
    chunk_id   INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    vector     BLOB NOT NULL,
    dimensions INTEGER NOT NULL,
    model      TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- ============================================================
-- FTS5 keyword search index
-- ============================================================

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    content_md,
    name,
    signature,
    file_path,
    content='chunks',
    content_rowid='id',
    tokenize='porter unicode61'
);

-- Triggers to keep FTS5 in sync with chunks table

CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, content_md, name, signature, file_path)
    VALUES (new.id, new.content_md, new.name, new.signature, new.file_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content_md, name, signature, file_path)
    VALUES ('delete', old.id, old.content_md, old.name, old.signature, old.file_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, content_md, name, signature, file_path)
    VALUES ('delete', old.id, old.content_md, old.name, old.signature, old.file_path);
    INSERT INTO chunks_fts(rowid, content_md, name, signature, file_path)
    VALUES (new.id, new.content_md, new.name, new.signature, new.file_path);
END;

-- ============================================================
-- sqlite-vec vector search (loaded dynamically)
-- If sqlite-vec is not available, use embeddings table + Rust KNN
-- ============================================================

-- This table is created at runtime if sqlite-vec extension loads:
-- CREATE VIRTUAL TABLE chunks_vec USING vec0(embedding float[768]);

-- ============================================================
-- Indexing metadata
-- ============================================================

CREATE TABLE IF NOT EXISTS schema_version (
    version   INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
);

INSERT INTO schema_version (version) VALUES (1);
