use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    pub integrity_ok: bool,
    pub integrity_error: Option<String>,
    pub total_chunks: i64,
    pub live_chunks: i64,
    pub tombstoned_chunks: i64,
    pub total_relationships: i64,
    pub total_embeddings: i64,
    pub total_entities: i64,
    pub orphan_hints: i64,
    pub orphan_relationships: i64,
    pub orphan_entity_attributes: i64,
    pub orphan_embeddings: i64,
    pub orphan_entity_deps: i64,
    pub index_size_bytes: u64,
    pub wal_size_bytes: u64,
    pub fts_entries: i64,
    pub vec_entries: i64,
    pub hints_vec_entries: i64,
}

pub fn run_health_check(db: &Connection) -> Result<HealthReport, Box<dyn std::error::Error>> {
    let integrity_result: String =
        db.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))?;
    let integrity_ok = integrity_result == "ok";

    let total_chunks: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let live_chunks: i64 = db.query_row(
        "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )?;
    let tombstoned_chunks: i64 = total_chunks - live_chunks;

    let total_relationships: i64 =
        db.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))?;
    let total_embeddings: i64 =
        db.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
    let total_entities: i64 = db.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;

    let orphan_hints: i64 = db.query_row(
        "SELECT COUNT(*) FROM hints h WHERE h.chunk_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
        |r| r.get(0),
    )?;

    let orphan_relationships: i64 = db.query_row(
        "SELECT COUNT(*) FROM relationships r WHERE r.source_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0) OR r.target_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
        |r| r.get(0),
    )?;

    let orphan_entity_attributes: i64 = db.query_row(
        "SELECT COUNT(*) FROM entity_attributes ea WHERE ea.chunk_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
        |r| r.get(0),
    )?;

    let orphan_embeddings: i64 = db.query_row(
        "SELECT COUNT(*) FROM embeddings e WHERE e.chunk_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
        |r| r.get(0),
    )?;

    let orphan_entity_deps: i64 = db.query_row(
        "SELECT COUNT(*) FROM entity_dependencies ed WHERE ed.source_entity NOT IN (SELECT id FROM entities) OR ed.target_entity NOT IN (SELECT id FROM entities)",
        [],
        |r| r.get(0),
    )?;

    let fts_entries: i64 = db
        .query_row("SELECT COUNT(*) FROM chunks_fts", [], |r| r.get(0))
        .unwrap_or(0);
    let vec_entries: i64 = db
        .query_row("SELECT COUNT(*) FROM chunks_vec", [], |r| r.get(0))
        .unwrap_or(0);
    let hints_vec_entries: i64 = db
        .query_row("SELECT COUNT(*) FROM hints_vec", [], |r| r.get(0))
        .unwrap_or(0);

    let index_path = db.path().map(|p| p.to_string()).unwrap_or_default();
    let index_size = std::fs::metadata(&index_path).map(|m| m.len()).unwrap_or(0);
    let wal_path = format!("{}-wal", index_path);
    let wal_size = std::fs::metadata(&wal_path).map(|m| m.len()).unwrap_or(0);

    Ok(HealthReport {
        integrity_ok,
        integrity_error: if integrity_ok {
            None
        } else {
            Some(integrity_result)
        },
        total_chunks,
        live_chunks,
        tombstoned_chunks,
        total_relationships,
        total_embeddings,
        total_entities,
        orphan_hints,
        orphan_relationships,
        orphan_entity_attributes,
        orphan_embeddings,
        orphan_entity_deps,
        index_size_bytes: index_size,
        wal_size_bytes: wal_size,
        fts_entries,
        vec_entries,
        hints_vec_entries,
    })
}

pub fn clean_orphans(
    db: &mut Connection,
) -> Result<OrphanCleanupResult, Box<dyn std::error::Error>> {
    let hints = db.execute(
        "DELETE FROM hints WHERE chunk_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
    )?;
    let relationships = db.execute(
        "DELETE FROM relationships WHERE source_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0) OR target_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
    )?;
    let entity_attributes = db.execute(
        "DELETE FROM entity_attributes WHERE chunk_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
    )?;
    let embeddings = db.execute(
        "DELETE FROM embeddings WHERE chunk_id NOT IN (SELECT id FROM chunks WHERE is_deleted = 0)",
        [],
    )?;
    let entity_deps = db.execute(
        "DELETE FROM entity_dependencies WHERE source_entity NOT IN (SELECT id FROM entities) OR target_entity NOT IN (SELECT id FROM entities)",
        [],
    )?;

    Ok(OrphanCleanupResult {
        hints,
        relationships,
        entity_attributes,
        embeddings,
        entity_deps,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct OrphanCleanupResult {
    pub hints: usize,
    pub relationships: usize,
    pub entity_attributes: usize,
    pub embeddings: usize,
    pub entity_deps: usize,
}

pub fn vacuum(db: &mut Connection) -> Result<(), Box<dyn std::error::Error>> {
    db.execute_batch("VACUUM;")?;
    Ok(())
}

pub fn analyze(db: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    db.execute_batch("ANALYZE;")?;
    Ok(())
}

pub fn compact(db: &mut Connection) -> Result<CompactResult, Box<dyn std::error::Error>> {
    let orphans = clean_orphans(db)?;
    vacuum(db)?;
    analyze(db)?;
    Ok(CompactResult { orphans })
}

#[derive(Debug, Clone, Serialize)]
pub struct CompactResult {
    pub orphans: OrphanCleanupResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db() -> rusqlite::Connection {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db
    }

    #[test]
    fn test_health_check_clean_db() {
        let db = make_db();
        let report = run_health_check(&db).unwrap();
        assert!(report.integrity_ok);
        assert_eq!(report.total_chunks, 0);
        assert_eq!(report.orphan_hints, 0);
        assert_eq!(report.orphan_relationships, 0);
    }

    #[test]
    fn test_clean_orphans_no_op() {
        let mut db = make_db();
        let result = clean_orphans(&mut db).unwrap();
        assert_eq!(result.hints, 0);
        assert_eq!(result.relationships, 0);
    }
}
