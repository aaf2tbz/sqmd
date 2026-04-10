use rusqlite::{params, Connection};

type ChunkRow = (i64, String, Option<String>, String, i64, i64);

#[derive(Debug, Clone, serde::Serialize)]
pub struct Community {
    pub id: i64,
    pub path: String,
    pub depth: i64,
    pub name: String,
    pub chunk_count: i64,
    pub entity_count: i64,
    pub summary: Option<String>,
    pub generated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub community_type: Option<String>,
}

pub fn ensure_communities(db: &Connection) -> Result<usize, Box<dyn std::error::Error>> {
    let existing: i64 = db
        .query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))
        .unwrap_or(0);

    if existing > 0 {
        return Ok(existing as usize);
    }

    let paths: Vec<String> = db
        .prepare(
            "SELECT DISTINCT file_path FROM chunks WHERE is_deleted = 0 AND file_path != '' ORDER BY file_path",
        )?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;

    let mut community_paths: Vec<(String, i64, String)> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for path in &paths {
        let parts: Vec<&str> = path.split('/').collect();
        for depth in 1..=parts.len() {
            let prefix = parts[..depth].join("/");
            if !seen.contains(&prefix) {
                seen.insert(prefix.clone());
                let name = parts[depth - 1];
                community_paths.push((prefix, depth as i64, name.to_string()));
            }
        }
    }

    community_paths.sort();
    community_paths.dedup();

    for (path, depth, name) in &community_paths {
        let chunk_count: i64 = db
            .prepare(
                "SELECT COUNT(*) FROM chunks WHERE file_path LIKE ?1 || '%' AND is_deleted = 0",
            )?
            .query_row(params![path], |r| r.get(0))
            .unwrap_or(0);

        let entity_count: i64 = db
            .prepare("SELECT COUNT(*) FROM entities WHERE canonical_name LIKE ?1 || '%'")?
            .query_row(params![path], |r| r.get(0))
            .unwrap_or(0);

        if chunk_count == 0 && entity_count == 0 {
            continue;
        }

        db.execute(
            "INSERT INTO communities (path, depth, name, chunk_count, entity_count, generated_at) VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))",
            params![path, depth, name, chunk_count, entity_count],
        )?;
    }

    let total: i64 = db
        .query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))
        .unwrap_or(0);

    Ok(total as usize)
}

pub fn ensure_graph_communities(db: &Connection) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;

    let module_rows: Vec<(String, String)> = {
        let mut stmt = db.prepare(
            "SELECT c.file_path, t.file_path
             FROM relationships r
             JOIN chunks c ON r.source_id = c.id
             JOIN chunks t ON r.target_id = t.id
             WHERE r.rel_type = 'imports' AND c.is_deleted = 0 AND t.is_deleted = 0
               AND c.file_path != t.file_path
             GROUP BY c.file_path, t.file_path",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    if !module_rows.is_empty() {
        let mut parent: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut rank: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for (src, _tgt) in &module_rows {
            let parent_dir = if let Some(pos) = src.rfind('/') {
                &src[..pos]
            } else {
                src.as_str()
            };
            parent.insert(src.clone(), parent_dir.to_string());
            *rank.entry(parent_dir.to_string()).or_insert(0) += 1;
        }

        let mut ranked: Vec<_> = rank.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));

        for (module_path, _conn_count) in ranked.iter().take(20) {
            let name = module_path.rsplit('/').next().unwrap_or(module_path);

            let chunk_count: i64 = db
                .prepare(
                    "SELECT COUNT(*) FROM chunks WHERE file_path LIKE ?1 || '%' AND is_deleted = 0",
                )?
                .query_row(params![module_path], |r| r.get(0))
                .unwrap_or(0);

            let entity_count: i64 = db
                .prepare(
                    "SELECT COUNT(*) FROM entities WHERE file_path LIKE ?1 || '%' AND entity_type != 'file'",
                )?
                .query_row(params![module_path], |r| r.get(0))
                .unwrap_or(0);

            if chunk_count == 0 {
                continue;
            }

            db.execute(
                "INSERT OR IGNORE INTO communities (path, depth, name, chunk_count, entity_count, generated_at, community_type)
                 VALUES (?1, 1, ?2, ?3, ?4, datetime('now'), 'module')",
                params![module_path, name, chunk_count, entity_count],
            )?;
            count += 1;
        }
    }

    let hierarchy_rows: Vec<(String, String, String)> = {
        let mut stmt = db.prepare(
            "SELECT e1.name, e2.name, ed.dep_type
             FROM entity_dependencies ed
             JOIN entities e1 ON ed.source_entity = e1.id
             JOIN entities e2 ON ed.target_entity = e2.id
             WHERE ed.valid_to IS NULL
               AND ed.dep_type IN ('extends', 'implements')
             GROUP BY e1.name, e2.name, ed.dep_type",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    if !hierarchy_rows.is_empty() {
        let mut parent_map: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut child_count: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for (child, parent, _dep_type) in &hierarchy_rows {
            *child_count.entry(parent.clone()).or_insert(0) += 1;
            parent_map.insert(child.clone(), parent.clone());
        }

        let mut sorted: Vec<_> = child_count.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));

        for (base, member_count) in sorted.iter().take(10) {
            if *member_count < 2 {
                continue;
            }

            let name = format!("{} hierarchy", base);
            let entity_count = *member_count as i64;

            db.execute(
                "INSERT OR IGNORE INTO communities (path, depth, name, chunk_count, entity_count, generated_at, community_type)
                 VALUES (?1, 2, ?2, 0, ?3, datetime('now'), 'type_hierarchy')",
                params![base, name, entity_count],
            )?;
            count += 1;
        }
    }

    Ok(count)
}

pub fn regenerate_summaries(db: &Connection) -> Result<usize, Box<dyn std::error::Error>> {
    let communities: Vec<(i64, String, i64, Option<String>)> = {
        let mut stmt = db.prepare(
            "SELECT id, path, depth, community_type FROM communities WHERE depth >= 1 ORDER BY depth ASC, path ASC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let mut updated = 0;

    for (id, path, depth, community_type) in &communities {
        let summary = generate_community_summary(
            db,
            path,
            *depth,
            community_type.as_deref().unwrap_or("directory"),
        )?;
        if !summary.is_empty() {
            db.execute(
                "UPDATE communities SET summary = ?1, generated_at = datetime('now') WHERE id = ?2",
                params![summary, id],
            )?;
            updated += 1;
        }
    }

    Ok(updated)
}

fn generate_community_summary(
    db: &Connection,
    path: &str,
    _depth: i64,
    community_type: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let like_pattern = format!("{}/%", path);

    let chunks: Vec<(Option<String>, String, String, String)> = {
        let mut stmt = db.prepare(
            "SELECT name, chunk_type, file_path, signature
             FROM chunks
             WHERE (file_path = ?1 OR file_path LIKE ?2) AND is_deleted = 0 AND importance >= 0.5
             ORDER BY importance DESC",
        )?;
        let rows = stmt
            .query_map(params![path, like_pattern], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    if chunks.is_empty() {
        return Ok(String::new());
    }

    let name = path.rsplit('/').next().unwrap_or(path);
    let type_counts = count_by_type(&chunks);
    let top_exports: Vec<String> = chunks
        .iter()
        .filter(|(_, ct, _, _)| {
            ct == "function"
                || ct == "struct"
                || ct == "class"
                || ct == "trait"
                || ct == "interface"
                || ct == "enum"
                || ct == "impl"
                || ct == "method"
        })
        .take(8)
        .map(|(n, _, _, _)| n.as_deref().unwrap_or("(unnamed)").to_string())
        .collect::<Vec<_>>();

    let mut summary = format!("**{}** — ", name);
    let parts: Vec<String> = type_counts
        .into_iter()
        .map(|(t, c)| format!("{} {}", c, t))
        .collect();
    summary.push_str(&parts.join(", "));

    if !top_exports.is_empty() {
        summary.push_str(". Key items: ");
        summary.push_str(&top_exports.join(", "));
    }

    match community_type {
        "module" => {
            let conn_count: i64 = db
                .prepare(
                    "SELECT COUNT(*) FROM relationships r
                     JOIN chunks c ON r.source_id = c.id AND c.is_deleted = 0
                     WHERE (c.file_path = ?1 OR c.file_path LIKE ?2) AND r.rel_type = 'imports'",
                )?
                .query_row(params![path, like_pattern], |r| r.get(0))
                .unwrap_or(0);

            let ext_count: i64 = db
                .prepare(
                    "SELECT COUNT(DISTINCT CASE
                        WHEN r.source_id IN (SELECT id FROM chunks WHERE file_path = ?1 OR file_path LIKE ?2)
                        THEN r.target_id ELSE NULL
                     END) FROM relationships r
                     JOIN chunks t ON r.target_id = t.id
                     WHERE r.rel_type = 'imports'",
                )?
                .query_row(params![path, like_pattern], |r| r.get(0))
                .unwrap_or(0);

            summary.push_str(&format!(
                ". {} import connections ({} imported from {} distinct modules).",
                conn_count, conn_count, ext_count
            ));
        }
        "type_hierarchy" => {
            let has_deps: bool = db
                .prepare(
                    "SELECT COUNT(*) FROM entity_dependencies ed
                     JOIN entities e ON ed.source_entity = e.id
                     WHERE e.name = ?1 AND ed.valid_to IS NULL",
                )?
                .query_row(params![name], |r| r.get::<_, i64>(0))
                .unwrap_or(0)
                > 0;

            if has_deps {
                summary.push_str(&format!(". {} structural relationships.", has_deps));
            }
        }
        _ => {}
    }

    Ok(summary)
}

fn count_by_type(chunks: &[(Option<String>, String, String, String)]) -> Vec<(String, usize)> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for (_, ct, _, _) in chunks {
        *counts.entry(ct.clone()).or_insert(0) += 1;
    }
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted
}

pub fn search_communities(
    db: &Connection,
    query: &str,
    top_k: usize,
) -> Result<Vec<Community>, Box<dyn std::error::Error>> {
    let like_pattern = format!("%{}%", query);

    let mut stmt = db.prepare(
        "SELECT id, path, depth, name, chunk_count, entity_count, summary, generated_at, community_type
         FROM communities
         WHERE path LIKE ?1 OR name LIKE ?1 OR summary LIKE ?1
         ORDER BY chunk_count DESC
         LIMIT ?2",
    )?;

    let rows = stmt
        .query_map(params![like_pattern, top_k as i64], |r| {
            Ok(Community {
                id: r.get(0)?,
                path: r.get(1)?,
                depth: r.get(2)?,
                name: r.get(3)?,
                chunk_count: r.get(4)?,
                entity_count: r.get(5)?,
                summary: r.get(6)?,
                generated_at: r.get(7)?,
                community_type: r.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_community_chunks(
    db: &Connection,
    community_id: i64,
) -> Result<Vec<ChunkRow>, Box<dyn std::error::Error>> {
    let path: String = db
        .query_row(
            "SELECT path FROM communities WHERE id = ?1",
            params![community_id],
            |r| r.get(0),
        )
        .map_err(|e| -> Box<dyn std::error::Error> { e.to_string().into() })?;

    let like_pattern = format!("{}/%", path);

    let mut stmt = db.prepare(
        "SELECT id, file_path, name, chunk_type, line_start, line_end
         FROM chunks
         WHERE (file_path = ?1 OR file_path LIKE ?2) AND is_deleted = 0
         ORDER BY importance DESC",
    )?;

    let rows = stmt
        .query_map(params![path, like_pattern], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(rows)
}

pub fn get_project_summary(db: &Connection) -> Result<String, Box<dyn std::error::Error>> {
    let total_chunks: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let total_files: i64 = db
        .query_row(
            "SELECT COUNT(DISTINCT file_path) FROM chunks WHERE is_deleted = 0 AND file_path != ''",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let total_entities: i64 = db
        .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
        .unwrap_or(0);

    let top_communities: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT name, chunk_count FROM communities WHERE depth = 1 AND chunk_count > 0 ORDER BY chunk_count DESC LIMIT 10",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let type_dist: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT chunk_type, COUNT(*) FROM chunks WHERE is_deleted = 0 GROUP BY chunk_type ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let lang_dist: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT language, COUNT(*) FROM chunks WHERE is_deleted = 0 AND language != '' GROUP BY language ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let mut summary = format!(
        "Project overview: {} chunks across {} files, {} entities.\n\n",
        total_chunks, total_files, total_entities
    );

    summary.push_str("Languages: ");
    summary.push_str(
        &lang_dist
            .iter()
            .map(|(l, c)| format!("{} ({})", l, c))
            .collect::<Vec<_>>()
            .join(", "),
    );
    summary.push_str(".\n");

    summary.push_str("Chunk types: ");
    summary.push_str(
        &type_dist
            .iter()
            .map(|(t, c)| format!("{} {}", c, t))
            .collect::<Vec<_>>()
            .join(", "),
    );
    summary.push_str(".\n");

    if !top_communities.is_empty() {
        summary.push_str("Top modules: ");
        summary.push_str(
            &top_communities
                .iter()
                .map(|(n, c)| format!("{} ({})", n, c))
                .collect::<Vec<_>>()
                .join(", "),
        );
        summary.push_str(".\n");
    }

    Ok(summary)
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
    fn test_ensure_communities() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/auth.ts', 'ts', 100, 0.0, 'a')", [])
            .unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/db.ts', 'ts', 50, 0.0, 'b')", [])
            .unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/utils/helper.ts', 'ts', 30, 0.0, 'c')", [])
            .unwrap();
        db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash, importance)
             VALUES ('src/auth.ts', 'ts', 'function', 'login', 0, 5, 'fn login() {}', 'x', 0.9)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash, importance)
             VALUES ('src/db.ts', 'ts', 'function', 'connect', 0, 3, 'fn connect() {}', 'y', 0.8)",
            [],
        )
        .unwrap();

        let count = ensure_communities(&db).unwrap();
        assert!(count >= 2, "should have at least 2 communities");
    }

    #[test]
    fn test_search_communities() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/auth.ts', 'ts', 100, 0.0, 'a')", [])
            .unwrap();
        db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash, importance)
             VALUES ('src/auth.ts', 'ts', 'function', 'login', 0, 5, 'fn login() {}', 'x', 0.9)",
            [],
        )
        .unwrap();
        ensure_communities(&db).unwrap();

        let results = search_communities(&db, "src", 5).unwrap();
        assert!(!results.is_empty());
    }

    #[test]
    fn test_get_project_summary() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/main.rs', 'rust', 100, 0.0, 'a')", [])
            .unwrap();
        db.execute(
            "INSERT INTO chunks (file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash, importance)
             VALUES ('src/main.rs', 'rust', 'function', 'main', 0, 5, 'fn main() {}', 'x', 0.9)",
            [],
        )
        .unwrap();

        let summary = get_project_summary(&db).unwrap();
        assert!(summary.contains("1 chunks"));
        assert!(summary.contains("1 files"));
    }
}
