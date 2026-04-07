use rusqlite::{Connection, Result as SqlResult};
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;

const CURRENT_VERSION: i64 = 8;

pub fn init(db: &mut Connection) -> SqlResult<()> {
    #[allow(clippy::missing_transmute_annotations)]
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite3_vec_init as *const (),
        )));
    }
    db.execute_batch("PRAGMA journal_mode = WAL;")?;
    db.execute_batch("PRAGMA foreign_keys = ON;")?;
    db.execute_batch("PRAGMA busy_timeout = 5000;")?;
    db.execute_batch("PRAGMA defer_foreign_keys = ON;")?;
    db.execute_batch("PRAGMA mmap_size = 268435456;")?;
    db.execute_batch("PRAGMA wal_autocheckpoint = 1000;")?;
    db.execute_batch("PRAGMA cache_size = -8000;")?;

    // Always ensure chunks_vec exists — handles dbs created before embed feature
    if let Err(e) = db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(embedding float[768]);",
    ) {
        eprintln!("[schema] chunks_vec creation failed: {e}");
    }

    let version: i64 = db
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if version == 0 {
        db.execute_batch(include_str!("../../../docs/schema.sql"))?;
        db.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(embedding float[768]);",
        )
        .ok();
        // schema.sql covers through v5 — only run migrations beyond that
        if CURRENT_VERSION > 5 {
            // Read actual version from schema.sql insert
            let sql_version: i64 = db
                .query_row(
                    "SELECT COALESCE(MAX(version), 0) FROM schema_version",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(5);
            // Only run migrations newer than what schema.sql provides
            if sql_version < 6 {
                migrate_v6(db)?;
            }
            if sql_version < 7 {
                migrate_v7(db)?;
            }
            if sql_version < 8 {
                migrate_v8(db)?;
            }
        }
        return Ok(());
    }

    if version < 2 {
        migrate_v2(db)?;
    }

    if version < 3 {
        migrate_v3(db)?;
    }

    if version < 4 {
        migrate_v4(db)?;
    }

    if version < 5 {
        migrate_v5(db)?;
    }

    if version < 6 {
        migrate_v6(db)?;
    }

    if version < 7 {
        migrate_v7(db)?;
    }

    if version < 8 {
        migrate_v8(db)?;
    }

    Ok(())
}

fn migrate_v2(db: &mut Connection) -> SqlResult<()> {
    let has_raw: bool = db
        .prepare("SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name='content_raw'")?
        .query_row([], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_raw {
        db.execute_batch("ALTER TABLE chunks ADD COLUMN content_raw TEXT;")?;
        db.execute_batch("UPDATE chunks SET content_raw = '' WHERE content_raw IS NULL;")?;
    }

    let has_importance: bool = db
        .prepare("SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name='importance'")?
        .query_row([], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_importance {
        db.execute_batch("ALTER TABLE chunks ADD COLUMN importance REAL NOT NULL DEFAULT 0.5;")?;
    }

    db.execute_batch("DROP TABLE IF EXISTS chunks_fts;")?;
    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            name, signature, content_raw, file_path,
            content='chunks', content_rowid='id',
            tokenize='unicode61'
        );",
    )?;
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
            VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
        END;",
    )?;
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
            VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
        END;",
    )?;
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
            VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
            INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
            VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
        END;",
    )?;

    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(embedding float[768]);",
    )
    .ok();

    db.execute_batch(&format!(
        "INSERT OR IGNORE INTO schema_version (version) VALUES ({})",
        CURRENT_VERSION
    ))?;

    Ok(())
}

fn migrate_v3(db: &mut Connection) -> SqlResult<()> {
    db.execute_batch("ALTER TABLE chunks ADD COLUMN is_deleted INTEGER NOT NULL DEFAULT 0;")?;
    db.execute_batch("ALTER TABLE chunks ADD COLUMN deleted_at TEXT;")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_deleted ON chunks(is_deleted);")?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entities (
            id           INTEGER PRIMARY KEY,
            name         TEXT NOT NULL,
            canonical_name TEXT NOT NULL,
            entity_type  TEXT NOT NULL DEFAULT 'file',
            mentions     INTEGER NOT NULL DEFAULT 1,
            created_at   TEXT NOT NULL DEFAULT '',
            updated_at   TEXT NOT NULL DEFAULT '',
            UNIQUE(canonical_name)
        );",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);")?;
    db.execute_batch("UPDATE entities SET created_at = datetime('now'), updated_at = datetime('now') WHERE created_at = '';")?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entity_aspects (
            id           INTEGER PRIMARY KEY,
            entity_id    INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            name         TEXT NOT NULL,
            canonical_name TEXT NOT NULL,
            weight       REAL NOT NULL DEFAULT 1.0,
            created_at   TEXT NOT NULL DEFAULT ''
        );",
    )?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entity_attributes (
            id           INTEGER PRIMARY KEY,
            entity_id    INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            aspect_id    INTEGER REFERENCES entity_aspects(id),
            chunk_id     INTEGER REFERENCES chunks(id),
            kind         TEXT NOT NULL DEFAULT 'attribute' CHECK(kind IN ('attribute', 'constraint')),
            content      TEXT NOT NULL,
            created_at   TEXT NOT NULL DEFAULT ''
        );",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_ea_entity ON entity_attributes(entity_id);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_ea_chunk ON entity_attributes(chunk_id);")?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entity_dependencies (
            id             INTEGER PRIMARY KEY,
            source_entity  INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            target_entity  INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            dep_type       TEXT NOT NULL DEFAULT 'imports' CHECK(dep_type IN ('imports', 'calls', 'contains', 'extends', 'implements')),
            strength       REAL NOT NULL DEFAULT 1.0,
            mentions       INTEGER NOT NULL DEFAULT 1,
            created_at     TEXT NOT NULL DEFAULT '',
            UNIQUE(source_entity, target_entity, dep_type)
        );",
    )?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ed_source ON entity_dependencies(source_entity, dep_type);",
    )?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ed_target ON entity_dependencies(target_entity);",
    )?;

    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS hints_fts USING fts5(
            hint_text,
            content='hints',
            content_rowid='id',
            tokenize='unicode61'
        );",
    )?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS hints (
            id         INTEGER PRIMARY KEY,
            chunk_id   INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
            hint_text  TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT ''
        );",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_entities_type ON entities(entity_type);")?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entity_aspects (
            id           INTEGER PRIMARY KEY,
            entity_id    INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            name         TEXT NOT NULL,
            canonical_name TEXT NOT NULL,
            weight       REAL NOT NULL DEFAULT 1.0,
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(entity_id, canonical_name)
        );",
    )?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entity_attributes (
            id           INTEGER PRIMARY KEY,
            entity_id    INTEGER NOT NULL REFERENCES entities(id) ON DELETE CASCADE,
            aspect_id    INTEGER REFERENCES entity_aspects(id),
            chunk_id     INTEGER REFERENCES chunks(id),
            kind         TEXT NOT NULL DEFAULT 'attribute' CHECK(kind IN ('attribute', 'constraint')),
            content      TEXT NOT NULL,
            created_at   TEXT NOT NULL DEFAULT (datetime('now'))
        );",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_ea_entity ON entity_attributes(entity_id);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_ea_chunk ON entity_attributes(chunk_id);")?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ed_source ON entity_dependencies(source_entity, dep_type);",
    )?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ed_target ON entity_dependencies(target_entity);",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_hints_chunk ON hints(chunk_id);")?;

    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS hints_fts_insert AFTER INSERT ON hints BEGIN
            INSERT INTO hints_fts(rowid, hint_text) VALUES (new.id, new.hint_text);
        END;",
    )?;
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS hints_fts_delete AFTER DELETE ON hints BEGIN
            INSERT INTO hints_fts(hints_fts, rowid, hint_text) VALUES ('delete', old.id, old.hint_text);
        END;",
    )?;

    db.execute_batch("INSERT OR IGNORE INTO schema_version (version) VALUES (3)")?;

    Ok(())
}

fn migrate_v4(db: &mut Connection) -> SqlResult<()> {
    // ── Add knowledge columns to chunks ─────────────────────────────
    // source_type: discriminates code vs memory vs transcript vs document vs entity
    let has_source_type: bool = db
        .prepare("SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name='source_type'")?
        .query_row([], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_source_type {
        db.execute_batch(
            "ALTER TABLE chunks ADD COLUMN source_type TEXT NOT NULL DEFAULT 'code';",
        )?;
        db.execute_batch("ALTER TABLE chunks ADD COLUMN agent_id TEXT;")?;
        db.execute_batch("ALTER TABLE chunks ADD COLUMN tags TEXT;")?;
        db.execute_batch("ALTER TABLE chunks ADD COLUMN decay_rate REAL NOT NULL DEFAULT 0.0;")?;
        db.execute_batch("ALTER TABLE chunks ADD COLUMN last_accessed TEXT;")?;
        db.execute_batch("ALTER TABLE chunks ADD COLUMN created_by TEXT;")?;
    }

    // Indexes for knowledge queries
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_source_type ON chunks(source_type);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_agent_id ON chunks(agent_id);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_tags ON chunks(tags);")?;

    // ── Rebuild chunks table to extend chunk_type CHECK constraint ───
    // SQLite cannot ALTER CHECK constraints, so we rebuild the table.
    // We use a staging approach that preserves all data.
    db.execute_batch("PRAGMA foreign_keys = OFF;")?;

    // Drop FTS triggers first (they reference 'chunks')
    db.execute_batch("DROP TRIGGER IF EXISTS chunks_fts_insert;")?;
    db.execute_batch("DROP TRIGGER IF EXISTS chunks_fts_delete;")?;
    db.execute_batch("DROP TRIGGER IF EXISTS chunks_fts_update;")?;

    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS chunks_v4 (
            id           INTEGER PRIMARY KEY,
            file_path    TEXT NOT NULL,
            language     TEXT NOT NULL,
            chunk_type   TEXT NOT NULL CHECK(chunk_type IN (
                'function', 'method', 'class', 'interface', 'type',
                'module', 'section', 'import', 'export', 'macro',
                'trait', 'impl', 'enum', 'struct', 'constant',
                'fact', 'summary', 'entity_description', 'document_section',
                'preference', 'decision'
            )),
            name         TEXT,
            signature    TEXT,
            line_start   INTEGER NOT NULL,
            line_end     INTEGER NOT NULL,
            content_raw  TEXT NOT NULL DEFAULT '',
            content_hash TEXT NOT NULL,
            importance   REAL NOT NULL DEFAULT 0.5 CHECK(importance >= 0.0 AND importance <= 1.0),
            metadata     TEXT,
            created_at   TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at   TEXT NOT NULL DEFAULT (datetime('now')),
            is_deleted   INTEGER NOT NULL DEFAULT 0,
            deleted_at   TEXT,
            source_type  TEXT NOT NULL DEFAULT 'code' CHECK(source_type IN ('code', 'memory', 'transcript', 'document', 'entity')),
            agent_id     TEXT,
            tags         TEXT,
            decay_rate   REAL NOT NULL DEFAULT 0.0,
            last_accessed TEXT,
            created_by   TEXT
        );",
    )?;

    db.execute_batch(
        "INSERT INTO chunks_v4 (id, file_path, language, chunk_type, name, signature,
            line_start, line_end, content_raw, content_hash, importance, metadata,
            created_at, updated_at, is_deleted, deleted_at, source_type, agent_id,
            tags, decay_rate, last_accessed, created_by)
         SELECT id, file_path, language, chunk_type, name, signature,
            line_start, line_end, COALESCE(content_raw, ''), content_hash, importance, metadata,
            COALESCE(created_at, datetime('now')), COALESCE(updated_at, datetime('now')),
            COALESCE(is_deleted, 0), deleted_at,
            COALESCE(source_type, 'code'), agent_id, tags,
            COALESCE(decay_rate, 0.0), last_accessed, created_by
         FROM chunks;",
    )?;

    db.execute_batch("DROP TABLE chunks;")?;
    db.execute_batch("ALTER TABLE chunks_v4 RENAME TO chunks;")?;

    // Recreate indexes
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_file ON chunks(file_path);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_type ON chunks(chunk_type);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_name ON chunks(name);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_hash ON chunks(content_hash);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_importance ON chunks(importance);")?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chunks_lines ON chunks(file_path, line_start, line_end);",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_deleted ON chunks(is_deleted);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_source_type ON chunks(source_type);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_chunks_agent_id ON chunks(agent_id);")?;

    // ── Rebuild relationships table to extend rel_type CHECK ────────
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS relationships_v4 (
            id        INTEGER PRIMARY KEY,
            source_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
            target_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
            rel_type  TEXT NOT NULL CHECK(rel_type IN (
                'imports', 'calls', 'contains', 'implements',
                'overrides', 'extends', 'references',
                'contradicts', 'supersedes', 'elaborates',
                'derived_from', 'mentioned_in', 'relates_to'
            )),
            metadata   TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            UNIQUE(source_id, target_id, rel_type)
        );",
    )?;

    db.execute_batch(
        "INSERT OR IGNORE INTO relationships_v4 (id, source_id, target_id, rel_type, metadata, created_at)
         SELECT id, source_id, target_id, rel_type, metadata, COALESCE(created_at, datetime('now'))
         FROM relationships;",
    )?;

    db.execute_batch("DROP TABLE relationships;")?;
    db.execute_batch("ALTER TABLE relationships_v4 RENAME TO relationships;")?;

    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_rels_source ON relationships(source_id, rel_type);",
    )?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_rels_target ON relationships(target_id, rel_type);",
    )?;

    // ── Rebuild entity_dependencies to extend dep_type CHECK ────────
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS entity_dependencies_v4 (
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
            UNIQUE(source_entity, target_entity, dep_type)
        );",
    )?;

    db.execute_batch(
        "INSERT OR IGNORE INTO entity_dependencies_v4
            (id, source_entity, target_entity, dep_type, strength, mentions, created_at)
         SELECT id, source_entity, target_entity, dep_type, strength, mentions, created_at
         FROM entity_dependencies;",
    )?;

    db.execute_batch("DROP TABLE entity_dependencies;")?;
    db.execute_batch("ALTER TABLE entity_dependencies_v4 RENAME TO entity_dependencies;")?;

    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ed_source ON entity_dependencies(source_entity, dep_type);",
    )?;
    db.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_ed_target ON entity_dependencies(target_entity);",
    )?;

    // ── Recreate FTS triggers ───────────────────────────────────────
    db.execute_batch("DROP TABLE IF EXISTS chunks_fts;")?;
    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
            name, signature, content_raw, file_path,
            content='chunks', content_rowid='id',
            tokenize='unicode61'
        );",
    )?;

    // Repopulate FTS index
    db.execute_batch(
        "INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
         SELECT id, name, signature, content_raw, file_path FROM chunks WHERE is_deleted = 0;",
    )?;

    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
            INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
            VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
        END;",
    )?;
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
            VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
        END;",
    )?;
    db.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
            INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
            VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
            INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
            VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
        END;",
    )?;

    // Recreate vec table
    db.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(embedding float[768]);",
    )
    .ok();

    db.execute_batch("PRAGMA foreign_keys = ON;")?;
    db.execute_batch("INSERT OR IGNORE INTO schema_version (version) VALUES (4);")?;

    Ok(())
}

pub fn open(path: &Path) -> SqlResult<Connection> {
    // Register sqlite-vec BEFORE opening so the connection has it available
    #[allow(clippy::missing_transmute_annotations)]
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite3_vec_init as *const (),
        )));
    }
    let mut db = Connection::open(path)?;
    init(&mut db)?;
    Ok(db)
}

pub fn open_fast(path: &Path) -> SqlResult<Connection> {
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    db.execute_batch("PRAGMA mmap_size = 268435456;")?;
    db.execute_batch("PRAGMA cache_size = -8000;")?;
    db.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(db)
}

fn migrate_v5(db: &mut Connection) -> SqlResult<()> {
    let has_stemmer: bool = db
        .prepare(
            "SELECT sql FROM sqlite_master WHERE type='table' AND name='chunks_fts' AND sql LIKE '%porter%'",
        )?
        .query_row([], |r| r.get::<_, String>(0))
        .ok()
        .is_some_and(|s| !s.is_empty());

    if !has_stemmer {
        db.execute_batch("DROP TRIGGER IF EXISTS chunks_fts_insert;")?;
        db.execute_batch("DROP TRIGGER IF EXISTS chunks_fts_delete;")?;
        db.execute_batch("DROP TRIGGER IF EXISTS chunks_fts_update;")?;
        db.execute_batch("DROP TABLE IF EXISTS chunks_fts;")?;
        db.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
                name, signature, content_raw, file_path,
                content='chunks', content_rowid='id',
                tokenize='porter unicode61'
            );",
        )?;
        db.execute_batch(
            "INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
             SELECT id, name, signature, content_raw, file_path FROM chunks WHERE is_deleted = 0;",
        )?;
        db.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS chunks_fts_insert AFTER INSERT ON chunks BEGIN
                INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
                VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
            END;",
        )?;
        db.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS chunks_fts_delete AFTER DELETE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
                VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
            END;",
        )?;
        db.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS chunks_fts_update AFTER UPDATE ON chunks BEGIN
                INSERT INTO chunks_fts(chunks_fts, rowid, name, signature, content_raw, file_path)
                VALUES ('delete', old.id, old.name, old.signature, old.content_raw, old.file_path);
                INSERT INTO chunks_fts(rowid, name, signature, content_raw, file_path)
                VALUES (new.id, new.name, new.signature, new.content_raw, new.file_path);
            END;",
        )?;

        db.execute_batch("DROP TRIGGER IF EXISTS hints_fts_insert;")?;
        db.execute_batch("DROP TRIGGER IF EXISTS hints_fts_delete;")?;
        db.execute_batch("DROP TABLE IF EXISTS hints_fts;")?;
        db.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS hints_fts USING fts5(
                hint_text,
                content='hints',
                content_rowid='id',
                tokenize='porter unicode61'
            );",
        )?;
        db.execute_batch(
            "INSERT INTO hints_fts(rowid, hint_text) SELECT id, hint_text FROM hints;",
        )?;
        db.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS hints_fts_insert AFTER INSERT ON hints BEGIN
                INSERT INTO hints_fts(rowid, hint_text) VALUES (new.id, new.hint_text);
            END;",
        )?;
        db.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS hints_fts_delete AFTER DELETE ON hints BEGIN
                INSERT INTO hints_fts(hints_fts, rowid, hint_text) VALUES ('delete', old.id, old.hint_text);
            END;",
        )?;
    }

    db.execute_batch("INSERT OR IGNORE INTO schema_version (version) VALUES (5);")?;
    Ok(())
}

fn migrate_v6(db: &mut Connection) -> SqlResult<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS communities (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            depth INTEGER NOT NULL DEFAULT 1,
            name TEXT NOT NULL,
            chunk_count INTEGER NOT NULL DEFAULT 0,
            entity_count INTEGER NOT NULL DEFAULT 0,
            summary TEXT,
            generated_at TEXT,
            created_at TEXT NOT NULL DEFAULT ''
        );",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_communities_path ON communities(path);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_communities_depth ON communities(depth);")?;
    db.execute_batch("INSERT OR IGNORE INTO schema_version (version) VALUES (6);")?;
    Ok(())
}

fn migrate_v7(db: &mut Connection) -> SqlResult<()> {
    let has_valid_from: bool = db
        .prepare(
            "SELECT COUNT(*) FROM pragma_table_info('entity_dependencies') WHERE name='valid_from'",
        )?
        .query_row([], |r| r.get::<_, i64>(0))
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_valid_from {
        db.execute_batch(
            "ALTER TABLE entity_dependencies ADD COLUMN valid_from TEXT NOT NULL DEFAULT '';",
        )?;
        db.execute_batch("ALTER TABLE entity_dependencies ADD COLUMN valid_to TEXT;")?;
        db.execute_batch(
            "UPDATE entity_dependencies SET valid_from = datetime('now') WHERE valid_from = '';",
        )?;
        db.execute_batch("CREATE INDEX IF NOT EXISTS idx_ed_temporal ON entity_dependencies(valid_from, valid_to);")?;
    }

    db.execute_batch("INSERT OR IGNORE INTO schema_version (version) VALUES (7);")?;
    Ok(())
}

fn migrate_v8(db: &mut Connection) -> SqlResult<()> {
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS episodes (
            id               INTEGER PRIMARY KEY,
            file_path        TEXT NOT NULL,
            change_type      TEXT NOT NULL CHECK(change_type IN ('added', 'modified', 'deleted')),
            commit_hash      TEXT,
            author           TEXT,
            summary          TEXT,
            chunks_affected  INTEGER NOT NULL DEFAULT 0,
            created_at       TEXT NOT NULL DEFAULT ''
        );",
    )?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_episodes_file ON episodes(file_path);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_episodes_type ON episodes(change_type);")?;
    db.execute_batch("CREATE INDEX IF NOT EXISTS idx_episodes_time ON episodes(created_at);")?;
    db.execute_batch("INSERT OR IGNORE INTO schema_version (version) VALUES (8);")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creates_tables() {
        #[allow(clippy::missing_transmute_annotations)]
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite3_vec_init as *const (),
            )));
        }
        let mut db = Connection::open_in_memory().unwrap();
        init(&mut db).unwrap();
        let files: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='files'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(files > 0);

        let chunks: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(chunks > 0);

        let fts: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks_fts'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(fts > 0);

        let vec_table: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks_vec'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(vec_table > 0);
    }

    #[test]
    fn test_schema_version() {
        let mut db = Connection::open_in_memory().unwrap();
        init(&mut db).unwrap();
        let version: i64 = db
            .query_row("SELECT MAX(version) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, CURRENT_VERSION);
    }

    #[test]
    fn test_chunks_has_content_raw() -> Result<(), Box<dyn std::error::Error>> {
        let mut db = Connection::open_in_memory().unwrap();
        init(&mut db).unwrap();
        let has_raw: bool = db
            .prepare("SELECT COUNT(*) FROM pragma_table_info('chunks') WHERE name='content_raw'")?
            .query_row([], |r| r.get::<_, i64>(0))
            .map(|c| c > 0)
            .unwrap();
        assert!(has_raw);
        Ok(())
    }

    #[test]
    fn test_vec0_knn() {
        init(&mut Connection::open_in_memory().unwrap()).unwrap();
        let db = Connection::open_in_memory().unwrap();

        db.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS vec_items USING vec0(embedding float[4]);",
        )
        .unwrap();

        db.execute(
            "INSERT INTO vec_items(rowid, embedding) VALUES (1, '[1.0, 0.0, 0.0, 0.0]')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vec_items(rowid, embedding) VALUES (2, '[0.0, 1.0, 0.0, 0.0]')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO vec_items(rowid, embedding) VALUES (3, '[0.0, 0.0, 1.0, 0.0]')",
            [],
        )
        .unwrap();

        let mut stmt = db
            .prepare(
                "SELECT rowid, distance FROM vec_items WHERE embedding MATCH '[1.0, 0.0, 0.0, 0.0]' ORDER BY distance LIMIT 3",
            )
            .unwrap();

        let results: Vec<(i64, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert_eq!(results.len(), 3);
        assert_eq!(results[0].0, 1);
        assert!((results[0].1 - 0.0).abs() < f64::EPSILON);
        assert!(results[1].1 > results[0].1);
    }
}
