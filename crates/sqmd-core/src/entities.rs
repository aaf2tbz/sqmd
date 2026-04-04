use rusqlite::params;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Entity {
    pub id: i64,
    pub name: String,
    pub canonical_name: String,
    pub entity_type: String,
    pub mentions: i64,
}

#[derive(Debug, Clone)]
pub struct Aspect {
    pub id: i64,
    pub entity_id: i64,
    pub name: String,
    pub weight: f64,
}

pub fn canonicalize(name: &str) -> String {
    name.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string()
}

pub fn ensure_entity(db: &rusqlite::Connection, name: &str, entity_type: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let canon = canonicalize(name);
    let existing: Option<i64> = db.query_row(
        "SELECT id FROM entities WHERE canonical_name = ?1",
        params![canon],
        |r| r.get(0),
    ).ok();

    if let Some(id) = existing {
        db.execute("UPDATE entities SET mentions = mentions + 1, updated_at = datetime('now') WHERE id = ?1", params![id])?;
        Ok(id)
    } else {
        db.execute(
            "INSERT INTO entities (name, canonical_name, entity_type, created_at, updated_at) VALUES (?1, ?2, ?3, datetime('now'), datetime('now'))",
            params![name, canon, entity_type],
        )?;
        Ok(db.last_insert_rowid())
    }
}

pub fn ensure_aspect(db: &rusqlite::Connection, entity_id: i64, aspect_name: &str) -> Result<i64, Box<dyn std::error::Error>> {
    let canon = canonicalize(aspect_name);
    let existing: Option<i64> = db.query_row(
        "SELECT id FROM entity_aspects WHERE entity_id = ?1 AND canonical_name = ?2",
        params![entity_id, canon],
        |r| r.get(0),
    ).ok();

    if let Some(id) = existing {
        Ok(id)
    } else {
        db.execute(
            "INSERT INTO entity_aspects (entity_id, name, canonical_name) VALUES (?1, ?2, ?3)",
            params![entity_id, aspect_name, canon],
        )?;
        Ok(db.last_insert_rowid())
    }
}

pub fn add_attribute(
    db: &rusqlite::Connection,
    entity_id: i64,
    aspect_id: Option<i64>,
    chunk_id: i64,
    kind: &str,
    content: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    db.execute(
        "INSERT OR IGNORE INTO entity_attributes (entity_id, aspect_id, chunk_id, kind, content) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![entity_id, aspect_id, chunk_id, kind, content],
    )?;
    Ok(())
}

pub fn ensure_dependency(
    db: &rusqlite::Connection,
    source_entity: i64,
    target_entity: i64,
    dep_type: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    db.execute(
        "INSERT INTO entity_dependencies (source_entity, target_entity, dep_type) VALUES (?1, ?2, ?3)
         ON CONFLICT(source_entity, target_entity, dep_type) DO UPDATE SET mentions = mentions + 1",
        params![source_entity, target_entity, dep_type],
    )?;
    Ok(())
}

pub fn get_entity(db: &rusqlite::Connection, name: &str) -> Result<Option<Entity>, Box<dyn std::error::Error>> {
    let canon = canonicalize(name);
    let result = db.query_row(
        "SELECT id, name, canonical_name, entity_type, mentions FROM entities WHERE canonical_name = ?1",
        params![canon],
        |r| Ok(Entity {
            id: r.get(0)?,
            name: r.get(1)?,
            canonical_name: r.get(2)?,
            entity_type: r.get(3)?,
            mentions: r.get(4)?,
        }),
    ).ok();
    Ok(result)
}

pub fn get_aspects(db: &rusqlite::Connection, entity_id: i64) -> Result<Vec<Aspect>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT id, entity_id, name, weight FROM entity_aspects WHERE entity_id = ?1 ORDER BY weight DESC"
    )?;
    let rows = stmt.query_map(params![entity_id], |r| {
        Ok(Aspect {
            id: r.get(0)?,
            entity_id: r.get(1)?,
            name: r.get(2)?,
            weight: r.get(3)?,
        })
    })?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_dependency_ids(
    db: &rusqlite::Connection,
    entity_id: i64,
    depth: usize,
) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    if depth == 0 {
        return Ok(Vec::new());
    }
    let sql = format!(
        "WITH RECURSIVE ent_deps(target_entity, d) AS (
            SELECT target_entity, 1 FROM entity_dependencies WHERE source_entity = ?1
            UNION
            SELECT ed.target_entity, ed2.d + 1 FROM entity_dependencies ed
            JOIN ent_deps ed2 ON ed.source_entity = ed2.target_entity
            WHERE ed2.d < {0}
        )
        SELECT DISTINCT target_entity FROM ent_deps WHERE target_entity != ?1",
        depth,
    );
    let mut stmt = db.prepare(&sql)?;
    let ids: Vec<i64> = stmt.query_map(params![entity_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

pub fn graph_boost_ids(
    db: &rusqlite::Connection,
    query: &str,
    max_ids: usize,
) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_lowercase())
        .collect();

    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    let mut entity_ids: Vec<i64> = Vec::new();
    for token in &tokens {
        let mut stmt = db.prepare(
            "SELECT id FROM entities WHERE canonical_name LIKE ?1 ORDER BY mentions DESC LIMIT 5"
        )?;
        let pattern = format!("%{}%", token);
        let ids: Vec<i64> = stmt.query_map(params![pattern], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        entity_ids.extend(ids);
    }

    entity_ids.sort();
    entity_ids.dedup();
    entity_ids.truncate(20);

    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expanded: Vec<i64> = if !entity_ids.is_empty() {
        let placeholders: Vec<String> = entity_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT DISTINCT target_entity FROM entity_dependencies WHERE source_entity IN ({})
             UNION
             SELECT DISTINCT source_entity FROM entity_dependencies WHERE target_entity IN ({})",
            placeholders.join(", "),
            placeholders.join(", "),
        );
        let mut stmt = db.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = entity_ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
        let ids: Vec<i64> = stmt.query_map(params.as_slice(), |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        entity_ids.extend(ids);
        entity_ids.sort();
        entity_ids.dedup();
        entity_ids
    } else {
        entity_ids
    };

    if expanded.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = expanded.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
    let sql = format!(
        "SELECT DISTINCT chunk_id FROM entity_attributes WHERE entity_id IN ({}) AND chunk_id IS NOT NULL LIMIT ?{}",
        placeholders.join(", "),
        placeholders.len() + 1,
    );

    let mut stmt = db.prepare(&sql)?;
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = expanded.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect();
    param_values.push(Box::new(max_ids as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let chunk_ids = stmt.query_map(param_refs.as_slice(), |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    Ok(chunk_ids)
}

pub fn list_entities(
    db: &rusqlite::Connection,
    entity_type_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<Entity>, Box<dyn std::error::Error>> {
    let sql = match entity_type_filter {
        Some(_) => format!("SELECT id, name, canonical_name, entity_type, mentions FROM entities WHERE entity_type = ?1 ORDER BY mentions DESC LIMIT {}", limit),
        None => format!("SELECT id, name, canonical_name, entity_type, mentions FROM entities ORDER BY mentions DESC LIMIT {}", limit),
    };
    let mut stmt = db.prepare(&sql)?;
    let rows: Vec<Entity> = if let Some(et) = entity_type_filter {
        stmt.query_map(params![et], |r| {
            Ok(Entity { id: r.get(0)?, name: r.get(1)?, canonical_name: r.get(2)?, entity_type: r.get(3)?, mentions: r.get(4)? })
        })?.collect::<Result<_, _>>()?
    } else {
        stmt.query_map([], |r| {
            Ok(Entity { id: r.get(0)?, name: r.get(1)?, canonical_name: r.get(2)?, entity_type: r.get(3)?, mentions: r.get(4)? })
        })?.collect::<Result<_, _>>()?
    };
    Ok(rows)
}

pub fn compute_structural_importance(
    db: &rusqlite::Connection,
    chunk_id: i64,
    base_importance: f64,
) -> Result<f64, Box<dyn std::error::Error>> {
    let in_degree: i64 = db.query_row(
        "SELECT COUNT(*) FROM relationships WHERE target_id = ?1 AND rel_type IN ('imports', 'calls')",
        params![chunk_id],
        |r| r.get(0),
    ).unwrap_or(0);

    let contains_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM relationships WHERE source_id = ?1 AND rel_type = 'contains'",
        params![chunk_id],
        |r| r.get(0),
    ).unwrap_or(0);

    let constraint_count: i64 = db.query_row(
        "SELECT COUNT(*) FROM entity_attributes ea
         JOIN relationships r ON ea.chunk_id = r.target_id
         WHERE r.target_id = ?1 AND ea.kind = 'constraint'",
        params![chunk_id],
        |r| r.get(0),
    ).unwrap_or(0);

    let in_factor = 0.3 * (1.0 + in_degree as f64).log10().max(0.0);
    let contains_factor = 0.2 * (1.0 + contains_count as f64).log10().max(0.0);
    let constraint_factor = if constraint_count > 0 { 0.2 } else { 0.0 };
    let base_factor = 0.3;

    let computed = base_importance * (in_factor + contains_factor + constraint_factor + base_factor);
    Ok(computed.clamp(0.1, 1.0))
}

pub fn generate_hints(
    chunk_name: Option<&str>,
    chunk_type: &str,
    content: &str,
    file_path: &str,
) -> Vec<String> {
    let mut hints = Vec::new();

    if let Some(name) = chunk_name {
        let snake = name.replace('_', " ");
        hints.push(format!("how does {} work", snake));
        hints.push(format!("where is {} defined", snake));
        hints.push(format!("{} implementation", snake));

        if matches!(chunk_type, "function" | "method") {
            hints.push(format!("{} function", snake));
            hints.push(format!("calling {} from", snake));
        } else if matches!(chunk_type, "struct" | "class" | "interface") {
            hints.push(format!("{} type definition", snake));
            hints.push(format!("{} methods", snake));
        }
    }

    let first_line = content.lines().next().unwrap_or("");
    let words: Vec<&str> = first_line.split_whitespace().collect();
    if words.len() >= 3 {
        let phrase = words[..words.len().min(6)].join(" ");
        hints.push(phrase);
    }

    if let Some(file_name) = file_path.rsplit('/').next() {
        hints.push(format!("code in {}", file_name));
    }

    hints.truncate(5);
    hints
}

pub fn insert_hints(
    db: &rusqlite::Connection,
    chunk_id: i64,
    hints: &[String],
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut stmt = db.prepare(
        "INSERT INTO hints (chunk_id, hint_text) VALUES (?1, ?2)"
    )?;
    for hint in hints {
        stmt.execute(params![chunk_id, hint])?;
        count += 1;
    }
    Ok(count)
}

pub fn search_hints(
    db: &rusqlite::Connection,
    query: &str,
    top_k: usize,
) -> Result<Vec<(i64, f64)>, Box<dyn std::error::Error>> {
    let sql = "SELECT h.chunk_id, f.rank FROM hints_fts f JOIN hints h ON f.rowid = h.id
               JOIN chunks c ON h.chunk_id = c.id WHERE c.is_deleted = 0
               AND hints_fts MATCH ?1 ORDER BY f.rank LIMIT ?2";
    let mut stmt = db.prepare(sql)?;
    let rows: Vec<(i64, f64)> = stmt.query_map(params![query, top_k as i64], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?.collect::<Result<_, _>>()?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let min_rank = rows.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
    let max_rank = rows.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
    let range = (max_rank - min_rank).abs().max(0.001);

    let scored: Vec<(i64, f64)> = rows.into_iter().map(|(id, rank)| {
        (id, 1.0 - ((rank - min_rank) / range))
    }).collect();

    Ok(scored)
}

pub fn tombstone_chunks(db: &rusqlite::Connection, file_path: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let count: i64 = db.query_row(
        "SELECT COUNT(*) FROM chunks WHERE file_path = ?1 AND is_deleted = 0",
        params![file_path],
        |r| r.get(0),
    ).unwrap_or(0);

    if count == 0 {
        return Ok(0);
    }

    db.execute(
        "UPDATE chunks SET is_deleted = 1, deleted_at = datetime('now') WHERE file_path = ?1 AND is_deleted = 0",
        params![file_path],
    )?;
    Ok(count as usize)
}

pub fn purge_tombstones(db: &rusqlite::Connection, max_age_days: i64) -> Result<usize, Box<dyn std::error::Error>> {
    let count = db.execute(
        "DELETE FROM chunks WHERE is_deleted = 1 AND deleted_at <= datetime('now', ?1)",
        params![format!("-{} days", max_age_days)],
    )?;
    Ok(count)
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
    fn test_ensure_entity_dedup() {
        let db = make_db();
        let id1 = ensure_entity(&db, "AuthModule", "file").unwrap();
        let id2 = ensure_entity(&db, "authmodule", "file").unwrap();
        assert_eq!(id1, id2);

        let entity = get_entity(&db, "AuthModule").unwrap().unwrap();
        assert_eq!(entity.mentions, 2);
    }

    #[test]
    fn test_ensure_aspect() {
        let db = make_db();
        let eid = ensure_entity(&db, "Config", "struct").unwrap();
        let aid1 = ensure_aspect(&db, eid, "exports").unwrap();
        let aid2 = ensure_aspect(&db, eid, "exports").unwrap();
        assert_eq!(aid1, aid2);

        let aspects = get_aspects(&db, eid).unwrap();
        assert_eq!(aspects.len(), 1);
    }

    #[test]
    fn test_generate_hints_for_function() {
        let hints = generate_hints(
            Some("authenticate"),
            "function",
            "fn authenticate(token: &str) -> Result<User> { ... }",
            "src/auth.ts",
        );
        assert!(hints.iter().any(|h| h.contains("how does authenticate")));
        assert!(hints.iter().any(|h| h.contains("where is authenticate")));
        assert!(hints.len() <= 5);
    }

    #[test]
    fn test_generate_hints_for_struct() {
        let hints = generate_hints(
            Some("DatabasePool"),
            "struct",
            "pub struct DatabasePool { ... }",
            "src/db.rs",
        );
        assert!(hints.iter().any(|h| h.contains("DatabasePool type definition")));
        assert!(hints.iter().any(|h| h.contains("DatabasePool methods")));
    }

    #[test]
    fn test_generate_hints_no_name() {
        let hints = generate_hints(None, "section", "use std::collections;", "src/lib.rs");
        assert!(!hints.is_empty());
    }

    #[test]
    fn test_compute_structural_importance() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('a.ts', 'ts', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'a.ts', 'ts', 'function', 'helper', 0, 1, 'fn helper()', 'x')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (2, 'a.ts', 'ts', 'function', 'caller', 2, 3, 'fn caller() { helper(); }', 'y')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (3, 'a.ts', 'ts', 'function', 'caller2', 4, 5, 'fn caller2() { helper(); }', 'z')", []).unwrap();
        db.execute("INSERT INTO relationships (source_id, target_id, rel_type) VALUES (2, 1, 'calls')", []).unwrap();
        db.execute("INSERT INTO relationships (source_id, target_id, rel_type) VALUES (3, 1, 'calls')", []).unwrap();

        let importance = compute_structural_importance(&db, 1, 0.9).unwrap();
        let isolated = compute_structural_importance(&db, 2, 0.9).unwrap();
        assert!(importance > isolated);
    }

    #[test]
    fn test_tombstone_and_purge() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('old.rs', 'rust', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'old.rs', 'rust', 'function', 'dead', 0, 1, 'fn dead()', 'x')", []).unwrap();

        let tombstoned = tombstone_chunks(&db, "old.rs").unwrap();
        assert_eq!(tombstoned, 1);

        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = 'old.rs' AND is_deleted = 1",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 1);

        let purged = purge_tombstones(&db, 0).unwrap();
        assert_eq!(purged, 1);

        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = 'old.rs'",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_insert_and_search_hints() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('auth.ts', 'ts', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'auth.ts', 'ts', 'function', 'authenticate', 0, 5, 'fn auth()', 'x')", []).unwrap();

        let hints = vec!["how does authentication work".to_string(), "auth function".to_string()];
        insert_hints(&db, 1, &hints).unwrap();

        let results = search_hints(&db, "authentication", 5).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].0, 1);
    }

    #[test]
    fn test_graph_boost_ids() {
        let db = make_db();
        let eid = ensure_entity(&db, "AuthModule", "file").unwrap();
        let target_eid = ensure_entity(&db, "DatabasePool", "file").unwrap();
        ensure_dependency(&db, eid, target_eid, "imports").unwrap();

        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('auth.ts', 'ts', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'auth.ts', 'ts', 'function', 'login', 0, 5, 'fn login()', 'x')", []).unwrap();
        add_attribute(&db, eid, None, 1, "attribute", "auth module chunk").unwrap();

        let ids = graph_boost_ids(&db, "auth", 10).unwrap();
        assert_eq!(ids.len(), 1);
        assert_eq!(ids[0], 1);
    }
}
