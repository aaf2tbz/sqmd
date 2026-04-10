-- sqmd SQLite Schema
-- Version: 2.1.0 (schema v12)
-- Source: https://github.com/aaf2tbz/sqmd
--
-- NOTE: This is a reference only. The actual schema lives in
-- crates/sqmd-core/src/schema.rs and is managed by the migration system.
-- Schema v5 adds Porter stemming to chunks_fts and hints_fts.
-- Schema v12 adds hints_vec for semantic hint retrieval.

-- ============================================================
-- Source file metadata
-- ============================================================

CREATE TABLE IF NOT EXISTS files (
    path       TEXT PRIMARY KEY,
    language   TEXT NOT NULL,
    size       INTEGER NOT NULL,
    mtime      REAL NOT NULL,
    hash       TEXT NOT NULL,
    indexed_at TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_files_language ON files(language);

-- ============================================================
-- Semantic code and knowledge chunks
-- ============================================================

CREATE TABLE IF NOT EXISTS chunks (
    id           INTEGER PRIMARY KEY,
    file_path    TEXT NOT NULL,
    language     TEXT NOT NULL DEFAULT '',
    chunk_type   TEXT NOT NULL CHECK(chunk_type IN (
        'function', 'method', 'class', 'interface', 'type',
        'module', 'section', 'import', 'export', 'macro',
        'trait', 'impl', 'enum', 'struct', 'constant',
        'fact', 'summary', 'entity_description', 'document_section',
        'preference', 'decision'
    )),
    name         TEXT,
    signature    TEXT,
    line_start   INTEGER NOT NULL DEFAULT 0,
    line_end     INTEGER NOT NULL DEFAULT 0,
    content_raw  TEXT NOT NULL DEFAULT '',
    content_hash TEXT NOT NULL,
    importance   REAL NOT NULL DEFAULT 0.5 CHECK(importance >= 0.0 AND importance <= 1.0),
    metadata     TEXT,
    created_at   TEXT NOT NULL DEFAULT '',
    updated_at   TEXT NOT NULL DEFAULT '',
    is_deleted   INTEGER NOT NULL DEFAULT 0,
    deleted_at   TEXT,
    source_type  TEXT NOT NULL DEFAULT 'code' CHECK(source_type IN ('code', 'memory', 'transcript', 'document', 'entity')),
    agent_id     TEXT,
    tags         TEXT,
    decay_rate   REAL NOT NULL DEFAULT 0.0,
    last_accessed TEXT,
    created_by   TEXT
);

CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);
CREATE INDEX IF NOT EXISTS idx_chunks_type ON chunks(chunk_type);
CREATE INDEX IF NOT EXISTS idx_chunks_name ON chunks(name);
CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(content_hash);
CREATE INDEX IF NOT EXISTS idx_chunks_importance ON chunks(importance);
CREATE INDEX IF NOT EXISTS idx_chunks_lines ON chunks(file_path, line_start, line_end);
CREATE INDEX IF NOT EXISTS idx_chunks_deleted ON chunks(is_deleted);
CREATE INDEX IF NOT EXISTS idx_chunks_source_type ON chunks(source_type);
CREATE INDEX IF NOT EXISTS idx_chunks_agent_id ON chunks(agent_id);

-- ============================================================
-- Import / call / knowledge relationship graph
-- ============================================================

CREATE TABLE IF NOT EXISTS relationships (
    id       INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    rel_type  TEXT NOT NULL CHECK(rel_type IN (
        'imports', 'calls', 'contains', 'implements',
        'overrides', 'extends', 'references',
        'contradicts', 'supersedes', 'elaborates',
        'derived_from', 'mentioned_in', 'relates_to'
    )),
    metadata  TEXT,
    created_at TEXT NOT NULL DEFAULT '',
    UNIQUE(source_id, target_id, rel_type)
);

CREATE INDEX IF NOT EXISTS idx_rels_source ON relationships(source_id, rel_type);
CREATE INDEX IF NOT EXISTS idx_rels_target ON relationships(target_id, rel_type);

-- ============================================================
-- Entity knowledge graph
-- ============================================================

CREATE TABLE IF NOT EXISTS entities (
    id             INTEGER PRIMARY KEY,
    name           TEXT NOT NULL,
    canonical_name TEXT NOT NULL,
    entity_type    TEXT NOT NULL DEFAULT 'file',
    mentions       INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL DEFAULT '',
    updated_at     TEXT NOT NULL DEFAULT '',
    file_path      TEXT,
    language       TEXT,
    line_start     INTEGER,
    line_end       INTEGER,
    signature      TEXT,
    chunk_id       INTEGER REFERENCES chunks(id),
    UNIQUE(canonical_name)
);

CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);
CREATE INDEX IF NOT EXISTS idx_entities_file ON entities(file_path);
CREATE INDEX IF NOT EXISTS idx_entities_chunk ON entities(chunk_id);
CREATE INDEX IF NOT EXISTS idx_entities_type_file ON entities(entity_type, file_path);

CREATE TABLE IF NOT EXISTS entity_aspects (
    id             INTEGER PRIMARY KEY,
    entity_id      INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    canonical_name TEXT NOT NULL,
    weight         REAL NOT NULL DEFAULT 1.0,
    created_at     TEXT NOT NULL DEFAULT '',
    UNIQUE(entity_id, canonical_name)
);

CREATE TABLE IF NOT EXISTS entity_attributes (
    id          INTEGER PRIMARY KEY,
    entity_id   INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    aspect_id   INTEGER REFERENCES entity_aspects(id),
    chunk_id    INTEGER REFERENCES chunks(id),
    kind        TEXT NOT NULL DEFAULT 'attribute' CHECK(kind IN ('attribute', 'constraint')),
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_ea_entity ON entity_attributes(entity_id);
CREATE INDEX IF NOT EXISTS idx_ea_chunk ON entity_attributes(chunk_id);

CREATE TABLE IF NOT EXISTS entity_dependencies (
    id             INTEGER PRIMARY KEY,
    source_entity  INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    target_entity  INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
    dep_type       TEXT NOT NULL DEFAULT 'imports' CHECK(dep_type IN (
        'imports', 'calls', 'contains', 'extends', 'implements',
        'contradicts', 'supersedes', 'elaborates', 'relates_to'
    )),
    strength       REAL NOT NULL DEFAULT 1.0,
    mentions       INTEGER NOT NULL DEFAULT 1,
    created_at     TEXT NOT NULL DEFAULT '',
    valid_from     TEXT NOT NULL DEFAULT '',
    valid_to       TEXT,
    UNIQUE(source_entity, target_entity, dep_type)
);

CREATE INDEX IF NOT EXISTS idx_ed_source ON entity_dependencies(source_entity, dep_type);
CREATE INDEX IF NOT EXISTS idx_ed_target ON entity_dependencies(target_entity);
CREATE INDEX IF NOT EXISTS idx_ed_temporal ON entity_dependencies(valid_from, valid_to);

-- ============================================================
-- Prospective search hints (template + LLM-generated)
-- ============================================================

CREATE TABLE IF NOT EXISTS hints (
    id         INTEGER PRIMARY KEY,
    chunk_id   INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    hint_text  TEXT NOT NULL,
    hint_type  TEXT NOT NULL DEFAULT 'symbol',
    created_at TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_hints_chunk ON hints(chunk_id);
CREATE INDEX IF NOT EXISTS idx_hints_type ON hints(hint_type);

-- ============================================================
-- Vector embeddings (BLOB fallback storage)
-- ============================================================

CREATE TABLE IF NOT EXISTS embeddings (
    chunk_id   INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    vector     BLOB NOT NULL,
    dimensions INTEGER NOT NULL,
    model      TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT ''
);

-- ============================================================
-- Community detection
-- ============================================================

CREATE TABLE IF NOT EXISTS communities (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL,
    depth INTEGER NOT NULL DEFAULT 1,
    name TEXT NOT NULL,
    chunk_count INTEGER NOT NULL DEFAULT 0,
    entity_count INTEGER NOT NULL DEFAULT 0,
    summary TEXT,
    generated_at TEXT,
    created_at TEXT NOT NULL DEFAULT '',
    community_type TEXT NOT NULL DEFAULT 'directory',
    CONSTRAINT uq_comm_path_type UNIQUE(path, community_type)
);

CREATE INDEX IF NOT EXISTS idx_communities_path ON communities(path);
CREATE INDEX IF NOT EXISTS idx_communities_type ON communities(community_type);
CREATE INDEX IF NOT EXISTS idx_communities_depth ON communities(depth);

-- ============================================================
-- Change provenance
-- ============================================================

CREATE TABLE IF NOT EXISTS episodes (
    id               INTEGER PRIMARY KEY,
    file_path        TEXT NOT NULL,
    change_type      TEXT NOT NULL CHECK(change_type IN ('added', 'modified', 'deleted')),
    commit_hash      TEXT,
    author           TEXT,
    summary          TEXT,
    chunks_affected  INTEGER NOT NULL DEFAULT 0,
    created_at       TEXT NOT NULL DEFAULT ''
);

CREATE INDEX IF NOT EXISTS idx_episodes_file ON episodes(file_path);
CREATE INDEX IF NOT EXISTS idx_episodes_type ON episodes(change_type);
CREATE INDEX IF NOT EXISTS idx_episodes_time ON episodes(created_at);

-- ============================================================
-- FTS5 keyword search index (porter-stemmed)
-- ============================================================

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    name,
    signature,
    content_raw,
    file_path,
    content='chunks',
    content_rowid='id',
    tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
    INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
    VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
    VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
END;

CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
    INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
    VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
    INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
    VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
END;

-- ============================================================
-- FTS5 hints index (porter-stemmed)
-- ============================================================

CREATE VIRTUAL TABLE IF NOT EXISTS hints_fts USING fts5(
    hint_text,
    content='hints',
    content_rowid='id',
    tokenize='porter unicode61'
);

CREATE TRIGGER IF NOT EXISTS hints_fts_insert AFTER INSERT ON hints BEGIN
    INSERT INTO hints_fts(rowid, hint_text) VALUES (new.id, new.hint_text);
END;

CREATE TRIGGER IF NOT EXISTS hints_fts_delete AFTER DELETE ON hints BEGIN
    INSERT INTO hints_fts(hints_fts, rowid, hint_text) VALUES ('delete', old.id, old.hint_text);
END;

-- ============================================================
-- Vector indexes (sqlite-vec virtual tables)
-- Created by schema.rs init() — shown here for reference
-- ============================================================

-- Content vector index: KNN search over chunk embeddings (768 dims)
-- CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(embedding float[768]);

-- Hint vector index: KNN search over hint text embeddings (768 dims, schema v12)
-- CREATE VIRTUAL TABLE IF NOT EXISTS hints_vec USING vec0(embedding float[768]);

-- ============================================================
-- Schema versioning
-- ============================================================

CREATE TABLE IF NOT EXISTS schema_version (
    version   INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT ''
);

INSERT OR IGNORE INTO schema_version (version) VALUES (12);
