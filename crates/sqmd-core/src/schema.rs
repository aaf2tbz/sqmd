use rusqlite::{Connection, Result as SqlResult};
use sqlite_vec::sqlite3_vec_init;
use std::path::Path;

pub const SCHEMA_SQL: &str = include_str!("../../../docs/schema.sql");

#[allow(clippy::missing_transmute_annotations)]
pub fn init(db: &mut Connection) -> SqlResult<()> {
    unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite3_vec_init as *const (),
        )));
    }
    db.execute_batch("SELECT 1;")?;
    db.execute_batch("PRAGMA journal_mode = WAL;")?;
    db.execute_batch("PRAGMA foreign_keys = ON;")?;
    db.execute_batch("PRAGMA busy_timeout = 5000;")?;

    let has_schema: bool = db
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='files'",
            [],
            |r| r.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !has_schema {
        db.execute_batch(SCHEMA_SQL)?;
    }

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
    }

    #[test]
    fn test_vec0_knn() {
        init(&mut Connection::open_in_memory().unwrap()).unwrap();
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("PRAGMA journal_mode = WAL;").unwrap();

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
