use rusqlite::{Connection, Result as SqlResult};
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;

const CURRENT_VERSION: i64 = 2;

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

    let version: i64 = db
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if version == 0 {
        db.execute_batch(include_str!("../../../docs/schema.sql"))?;
    }

    if version < 2 {
        migrate_v2(db)?;
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

    db.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(embedding float[768]);").ok();

    db.execute_batch(&format!(
        "INSERT INTO schema_version (version) VALUES ({})",
        CURRENT_VERSION
    ))?;

    Ok(())
}

pub fn open(path: &Path) -> SqlResult<Connection> {
    let mut db = Connection::open(path)?;
    init(&mut db)?;
    Ok(db)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_creates_tables() {
        let mut db = Connection::open_in_memory().unwrap();
        init(&mut db).unwrap();
        let files: i64 = db
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='files'", [], |r| r.get(0))
            .unwrap();
        assert!(files > 0);

        let chunks: i64 = db
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks'", [], |r| r.get(0))
            .unwrap();
        assert!(chunks > 0);

        let fts: i64 = db
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks_fts'", [], |r| r.get(0))
            .unwrap();
        assert!(fts > 0);

        let vec_table: i64 = db
            .query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='chunks_vec'", [], |r| r.get(0))
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
