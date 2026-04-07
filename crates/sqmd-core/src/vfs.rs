use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VfsEntry {
    pub id: i64,
    pub file_path: String,
    pub language: String,
    pub chunk_type: String,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub line_start: i64,
    pub line_end: i64,
    pub importance: f64,
    pub children: Vec<VfsEntry>,
}

pub fn list_chunks(
    db: &Connection,
    file_pattern: Option<&str>,
    type_filter: Option<&str>,
    max_depth: usize,
) -> Result<Vec<VfsEntry>, Box<dyn std::error::Error>> {
    let mut sql = String::from(
        "SELECT id, file_path, language, chunk_type, name, signature, line_start, line_end, importance
         FROM chunks WHERE 1=1"
    );

    if let Some(f) = file_pattern {
        sql.push_str(&format!(
            " AND file_path LIKE '%{}%'",
            f.replace('\'', "''")
        ));
    }
    if let Some(t) = type_filter {
        sql.push_str(&format!(" AND chunk_type = '{}'", t.replace('\'', "''")));
    }

    sql.push_str(" ORDER BY file_path, line_start");

    let mut stmt = db.prepare(&sql)?;

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        f64,
    )> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let entries: Vec<VfsEntry> = rows
        .into_iter()
        .map(
            |(
                id,
                file_path,
                language,
                chunk_type,
                name,
                signature,
                line_start,
                line_end,
                importance,
            )| {
                VfsEntry {
                    id,
                    file_path,
                    language,
                    chunk_type,
                    name,
                    signature,
                    line_start,
                    line_end,
                    importance,
                    children: Vec::new(),
                }
            },
        )
        .collect();

    if max_depth > 0 {
        let entry_ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
        let children = build_contains_tree(db, &entry_ids, max_depth)?;
        let child_map: std::collections::HashMap<i64, Vec<VfsEntry>> =
            children
                .into_iter()
                .fold(std::collections::HashMap::new(), |mut map, entry| {
                    map.entry(entry.id).or_default();
                    if let Some(parent_id) = find_parent_id(db, entry.id) {
                        map.entry(parent_id).or_default().push(entry);
                    }
                    map
                });

        let mut result = entries;
        attach_children(&mut result, &child_map);
        Ok(result)
    } else {
        Ok(entries)
    }
}

fn find_parent_id(db: &Connection, child_id: i64) -> Option<i64> {
    db.query_row(
        "SELECT source_id FROM relationships WHERE target_id = ?1 AND rel_type = 'contains' LIMIT 1",
        params![child_id],
        |r| r.get(0),
    )
    .ok()
}

fn build_contains_tree(
    db: &Connection,
    parent_ids: &[i64],
    max_depth: usize,
) -> Result<Vec<VfsEntry>, Box<dyn std::error::Error>> {
    if max_depth == 0 || parent_ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = parent_ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT c.id, c.file_path, c.language, c.chunk_type, c.name, c.signature, c.line_start, c.line_end, c.importance
         FROM relationships r
         JOIN chunks c ON r.target_id = c.id
         WHERE r.source_id IN ({}) AND r.rel_type = 'contains'
         ORDER BY c.line_start",
        placeholders.join(", ")
    );

    let mut stmt = db.prepare(&sql)?;
    let p: Vec<&dyn rusqlite::ToSql> = parent_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();

    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        i64,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        f64,
    )> = stmt
        .query_map(p.as_slice(), |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
                r.get(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let entries: Vec<VfsEntry> = rows
        .into_iter()
        .map(
            |(
                id,
                file_path,
                language,
                chunk_type,
                name,
                signature,
                line_start,
                line_end,
                importance,
            )| {
                VfsEntry {
                    id,
                    file_path,
                    language,
                    chunk_type,
                    name,
                    signature,
                    line_start,
                    line_end,
                    importance,
                    children: Vec::new(),
                }
            },
        )
        .collect();

    let child_ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
    let mut grandchildren = build_contains_tree(db, &child_ids, max_depth - 1)?;

    let mut child_map: std::collections::HashMap<i64, Vec<VfsEntry>> =
        std::collections::HashMap::new();
    for entry in grandchildren.drain(..) {
        if let Some(parent_id) = find_parent_id(db, entry.id) {
            child_map.entry(parent_id).or_default().push(entry);
        }
    }

    let mut result = entries;
    attach_children(&mut result, &child_map);
    Ok(result)
}

fn attach_children(
    entries: &mut [VfsEntry],
    child_map: &std::collections::HashMap<i64, Vec<VfsEntry>>,
) {
    for entry in entries.iter_mut() {
        if let Some(children) = child_map.get(&entry.id) {
            entry.children = children.clone();
        }
    }
}

pub fn get_chunk_by_id(
    db: &Connection,
    chunk_id: i64,
) -> Result<Option<VfsEntry>, Box<dyn std::error::Error>> {
    let result = db.query_row(
        "SELECT id, file_path, language, chunk_type, name, signature, line_start, line_end, importance
         FROM chunks WHERE id = ?1",
        params![chunk_id],
        |r| {
            Ok(VfsEntry {
                id: r.get(0)?,
                file_path: r.get(1)?,
                language: r.get(2)?,
                chunk_type: r.get(3)?,
                name: r.get(4)?,
                signature: r.get(5)?,
                line_start: r.get(6)?,
                line_end: r.get(7)?,
                importance: r.get(8)?,
                children: Vec::new(),
            })
        },
    ).ok();
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkDiff {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub change: String,
    pub old_content: Option<String>,
    pub new_content: Option<String>,
}

pub fn diff_snapshots(
    db: &Connection,
    since: &str,
) -> Result<Vec<ChunkDiff>, Box<dyn std::error::Error>> {
    let sql = "
        SELECT c.file_path, c.name, c.chunk_type, c.content_raw, c.updated_at
        FROM chunks c
        WHERE c.updated_at > ?1
        ORDER BY c.file_path, c.line_start
    ";

    let mut stmt = db.prepare(sql)?;
    let rows: Vec<(String, Option<String>, String, String, String)> = stmt
        .query_map(params![since], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let diffs: Vec<ChunkDiff> = rows
        .into_iter()
        .map(
            |(file_path, name, chunk_type, content, updated_at)| ChunkDiff {
                file_path,
                name,
                chunk_type,
                change: format!("modified at {}", updated_at),
                old_content: None,
                new_content: Some(content),
            },
        )
        .collect();

    Ok(diffs)
}

pub fn render_tree(entries: &[VfsEntry], indent: usize) -> String {
    let mut out = String::new();
    let prefix = " ".repeat(indent);
    for entry in entries {
        let name = entry.name.as_deref().unwrap_or("(unnamed)");
        out.push_str(&format!(
            "{}{} {}:{}-{} [{}]\n",
            prefix,
            name,
            entry.file_path,
            entry.line_start + 1,
            entry.line_end + 1,
            entry.chunk_type,
        ));
        if !entry.children.is_empty() {
            out.push_str(&render_tree(&entry.children, indent + 2));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute(
            "INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/main.rs', 'rust', 100, 0.0, 'abc')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance)
             VALUES (1, 'src/main.rs', 'rust', 'function', 'main', 'main()', 0, 5, 'fn main() {}', 'x', 0.9)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance)
             VALUES (2, 'src/main.rs', 'rust', 'struct', 'Config', '', 7, 10, 'struct Config { port: u16 }', 'y', 0.8)",
            [],
        )
        .unwrap();
        db
    }

    #[test]
    fn test_list_chunks() {
        let db = make_test_db();
        let entries = list_chunks(&db, None, None, 0).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].name.as_deref(), Some("main"));
        assert_eq!(entries[1].name.as_deref(), Some("Config"));
    }

    #[test]
    fn test_list_chunks_with_file_filter() {
        let db = make_test_db();
        let entries = list_chunks(&db, Some("main.rs"), None, 0).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_get_chunk_by_id() {
        let db = make_test_db();
        let entry = get_chunk_by_id(&db, 1).unwrap().unwrap();
        assert_eq!(entry.name.as_deref(), Some("main"));
        assert_eq!(entry.chunk_type, "function");
    }

    #[test]
    fn test_render_tree() {
        let db = make_test_db();
        let entries = list_chunks(&db, None, None, 0).unwrap();
        let tree = render_tree(&entries, 0);
        assert!(tree.contains("main"));
        assert!(tree.contains("Config"));
    }
}
