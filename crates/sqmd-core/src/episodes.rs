use rusqlite::{params, Connection};

#[derive(Debug, Clone, serde::Serialize)]
pub struct Episode {
    pub id: i64,
    pub file_path: String,
    pub change_type: String,
    pub commit_hash: Option<String>,
    pub author: Option<String>,
    pub summary: Option<String>,
    pub chunks_affected: i64,
    pub created_at: String,
}

pub fn record_episode(
    db: &Connection,
    file_path: &str,
    change_type: &str,
    commit_hash: Option<&str>,
    author: Option<&str>,
    chunks_affected: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    db.execute(
        "INSERT INTO episodes (file_path, change_type, commit_hash, author, chunks_affected, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
        params![file_path, change_type, commit_hash, author, chunks_affected],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn record_episode_with_summary(
    db: &Connection,
    file_path: &str,
    change_type: &str,
    commit_hash: Option<&str>,
    author: Option<&str>,
    chunks_affected: i64,
    summary: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    db.execute(
        "INSERT INTO episodes (file_path, change_type, commit_hash, author, chunks_affected, summary, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
        params![file_path, change_type, commit_hash, author, chunks_affected, summary],
    )?;
    Ok(db.last_insert_rowid())
}

pub fn get_recent_episodes(
    db: &Connection,
    limit: usize,
) -> Result<Vec<Episode>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT id, file_path, change_type, commit_hash, author, summary, chunks_affected, created_at
         FROM episodes ORDER BY created_at DESC LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit as i64], |r| {
            Ok(Episode {
                id: r.get(0)?,
                file_path: r.get(1)?,
                change_type: r.get(2)?,
                commit_hash: r.get(3)?,
                author: r.get(4)?,
                summary: r.get(5)?,
                chunks_affected: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_file_episodes(
    db: &Connection,
    file_path: &str,
    limit: usize,
) -> Result<Vec<Episode>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT id, file_path, change_type, commit_hash, author, summary, chunks_affected, created_at
         FROM episodes WHERE file_path = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![file_path, limit as i64], |r| {
            Ok(Episode {
                id: r.get(0)?,
                file_path: r.get(1)?,
                change_type: r.get(2)?,
                commit_hash: r.get(3)?,
                author: r.get(4)?,
                summary: r.get(5)?,
                chunks_affected: r.get(6)?,
                created_at: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_episode_stats(db: &Connection) -> Result<EpisodeStats, Box<dyn std::error::Error>> {
    let total: i64 = db
        .query_row("SELECT COUNT(*) FROM episodes", [], |r| r.get(0))
        .unwrap_or(0);
    let added: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM episodes WHERE change_type = 'added'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let modified: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM episodes WHERE change_type = 'modified'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let deleted: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM episodes WHERE change_type = 'deleted'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    Ok(EpisodeStats {
        total,
        added,
        modified,
        deleted,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EpisodeStats {
    pub total: i64,
    pub added: i64,
    pub modified: i64,
    pub deleted: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_db() -> Connection {
        #[allow(clippy::missing_transmute_annotations)]
        unsafe {
            rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
                sqlite_vec::sqlite3_vec_init as *const (),
            )));
        }
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db
    }

    #[test]
    fn test_record_and_get_episodes() {
        let db = make_db();
        let id1 = record_episode(&db, "src/main.rs", "added", None, None, 3).unwrap();
        let id2 = record_episode_with_summary(
            &db,
            "src/main.rs",
            "modified",
            Some("abc123"),
            Some("alex"),
            2,
            "refactored main fn",
        )
        .unwrap();

        assert!(id1 > 0);
        assert!(id2 > id1);

        let episodes = get_recent_episodes(&db, 10).unwrap();
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].change_type, "modified");
        assert_eq!(episodes[1].change_type, "added");
    }

    #[test]
    fn test_get_file_episodes() {
        let db = make_db();
        record_episode(&db, "src/a.rs", "added", None, None, 1).unwrap();
        record_episode(&db, "src/b.rs", "added", None, None, 2).unwrap();
        record_episode(&db, "src/a.rs", "modified", None, None, 1).unwrap();

        let file_eps = get_file_episodes(&db, "src/a.rs", 10).unwrap();
        assert_eq!(file_eps.len(), 2);
    }

    #[test]
    fn test_episode_stats() {
        let db = make_db();
        record_episode(&db, "a.rs", "added", None, None, 1).unwrap();
        record_episode(&db, "b.rs", "added", None, None, 1).unwrap();
        record_episode(&db, "a.rs", "modified", None, None, 1).unwrap();
        record_episode(&db, "c.rs", "deleted", None, None, 0).unwrap();

        let stats = get_episode_stats(&db).unwrap();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.added, 2);
        assert_eq!(stats.modified, 1);
        assert_eq!(stats.deleted, 1);
    }
}
