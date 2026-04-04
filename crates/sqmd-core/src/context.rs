use rusqlite::Connection;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextRequest {
    pub query: String,
    pub files: Vec<String>,
    pub max_tokens: usize,
    pub include_deps: bool,
    pub dep_depth: usize,
    pub top_k: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextResponse {
    pub markdown: String,
    pub token_count: usize,
    pub chunk_count: usize,
    pub sources: Vec<ChunkSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkSource {
    pub file_path: String,
    pub name: Option<String>,
    pub chunk_type: String,
    pub line_start: i64,
    pub line_end: i64,
    pub score: f64,
}

type SelectedChunk = (i64, String, Option<String>, String, i64, i64, String, String, f64);

pub struct ContextAssembler;

impl ContextAssembler {
    pub fn build(
        db: &Connection,
        request: &ContextRequest,
    ) -> Result<ContextResponse, Box<dyn std::error::Error>> {
        let mut selected: Vec<SelectedChunk> = Vec::new();
        let mut seen_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

        // 1. Search by query (hybrid)
        if !request.query.is_empty() {
            let search_query = crate::search::SearchQuery {
                text: request.query.clone(),
                top_k: request.top_k,
                ..Default::default()
            };
            let mut embedder = crate::embed::Embedder::new()?;
            let results = crate::search::hybrid_search(db, &search_query, &mut embedder)?;
            for r in &results {
                if seen_ids.insert(r.chunk_id) {
                    let content = get_chunk_content(db, r.chunk_id)?;
                    selected.push((
                        r.chunk_id,
                        r.file_path.clone(),
                        r.name.clone(),
                        r.chunk_type.clone(),
                        r.line_start,
                        r.line_end,
                        r.score.to_string(),
                        content,
                        r.score,
                    ));
                }
            }
        }

        // 2. Fetch specific files
        for file_path in &request.files {
            let mut stmt = db.prepare(
                "SELECT id, file_path, name, chunk_type, line_start, line_end, importance, content_raw
                 FROM chunks WHERE file_path = ?1 AND importance >= 0.5
                 ORDER BY importance DESC",
            )?;
            #[allow(clippy::type_complexity)]
            let rows: Vec<(i64, String, Option<String>, String, i64, i64, f64, String)> = stmt
                .query_map(rusqlite::params![file_path], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            for (id, fp, name, ct, ls, le, imp, content) in rows {
                if seen_ids.insert(id) {
                    selected.push((id, fp, name, ct, ls, le, format!("{:.2}", imp), content, imp));
                }
            }
        }

        // 3. Include dependencies
        if request.include_deps {
            let chunk_ids: Vec<i64> = selected.iter().map(|(id, ..)| *id).collect();
            if !chunk_ids.is_empty() {
                let deps = get_related_chunks(db, &chunk_ids, request.dep_depth)?;
                for (id, fp, name, ct, ls, le, content) in &deps {
                    if seen_ids.insert(*id) {
                        selected.push((*id, fp.clone(), name.clone(), ct.clone(), *ls, *le, "0.5".to_string(), content.clone(), 0.5));
                    }
                }
            }
        }

        // 4. Sort by score descending, then render with token budget
        selected.sort_by(|a, b| b.8.partial_cmp(&a.8).unwrap_or(std::cmp::Ordering::Equal));

        let mut markdown = String::new();
        let mut token_count = 0;
        let mut sources = Vec::new();

        for (_id, file_path, name, chunk_type, line_start, line_end, _score, content, score) in &selected {
            let rendered = render_chunk(file_path, name, chunk_type, *line_start, *line_end, content);
            let chunk_tokens = estimate_tokens(&rendered);

            if token_count + chunk_tokens > request.max_tokens && token_count > 0 {
                break;
            }

            sources.push(ChunkSource {
                file_path: file_path.clone(),
                name: name.clone(),
                chunk_type: chunk_type.clone(),
                line_start: *line_start,
                line_end: *line_end,
                score: *score,
            });

            markdown.push_str(&rendered);
            markdown.push('\n');
            token_count += chunk_tokens;
        }

        Ok(ContextResponse {
            markdown,
            token_count,
            chunk_count: sources.len(),
            sources,
        })
    }
}

fn render_chunk(
    file_path: &str,
    name: &Option<String>,
    chunk_type: &str,
    line_start: i64,
    line_end: i64,
    content: &str,
) -> String {
    let name = name.as_deref().unwrap_or("(unnamed)");
    format!(
        "### `{}`\n\n**File:** `{}` | **Lines:** {}-{} | **Type:** {}\n\n```{}\n{}\n```\n",
        name,
        file_path,
        line_start + 1,
        line_end + 1,
        chunk_type,
        "", // language omitted for context (agent doesn't need it)
        content.trim(),
    )
}

fn get_chunk_content(db: &Connection, chunk_id: i64) -> Result<String, Box<dyn std::error::Error>> {
    let content: String = db
        .query_row("SELECT content_raw FROM chunks WHERE id = ?1", rusqlite::params![chunk_id], |r| {
            r.get(0)
        })
        .unwrap_or_default();
    Ok(content)
}

#[allow(clippy::type_complexity)]
fn get_related_chunks(
    db: &Connection,
    chunk_ids: &[i64],
    depth: usize,
) -> Result<Vec<(i64, String, Option<String>, String, i64, i64, String)>, Box<dyn std::error::Error>> {
    if chunk_ids.is_empty() || depth == 0 {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (0..chunk_ids.len()).map(|i| format!("?{}", i + 1)).collect();
    let sql = format!(
        "WITH RECURSIVE rel_graph(id, depth) AS (
            SELECT target_id, 1 FROM relationships WHERE source_id IN ({0}) AND rel_type = 'imports'
            UNION
            SELECT target_id, rg.depth + 1 FROM relationships r
            JOIN rel_graph rg ON r.source_id = rg.id
            WHERE rg.depth < {1} AND r.rel_type = 'imports'
        )
        SELECT DISTINCT c.id, c.file_path, c.name, c.chunk_type, c.line_start, c.line_end, c.content_raw
        FROM rel_graph rg
        JOIN chunks c ON rg.id = c.id
        WHERE c.id NOT IN ({0})
        ORDER BY c.importance DESC
        LIMIT 50",
        placeholders.join(", "),
        depth
    );

    let mut stmt = db.prepare(&sql)?;
    let params: Vec<&dyn rusqlite::ToSql> = chunk_ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let rows: Vec<(i64, String, Option<String>, String, i64, i64, String)> = stmt
        .query_map(params.as_slice(), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })?
        .collect::<Result<_, _>>()?;

    Ok(rows)
}

pub fn estimate_tokens(text: &str) -> usize {
    // cl100k_base approximation: ~4 chars per token for code
    text.len() / 4
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_assembly_db() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/auth.ts', 'typescript', 100, 0.0, 'a')", []).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/db.ts', 'typescript', 50, 0.0, 'b')", []).unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance)
             VALUES (1, 'src/auth.ts', 'typescript', 'function', 'login', 'login(u: string)', 0, 5, 'async function login(u) { return db.find(u); }', 'x', 0.9)",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance)
             VALUES (2, 'src/auth.ts', 'typescript', 'function', 'logout', 'logout()', 7, 10, 'function logout() { session.clear(); }', 'y', 0.9)",
            [],
        ).unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, importance)
             VALUES (3, 'src/db.ts', 'typescript', 'function', 'connect', 'connect(url: string)', 0, 3, 'async function connect(url) { return pool(url); }', 'z', 0.9)",
            [],
        ).unwrap();
        // Import relationship: auth.login imports db.connect
        db.execute("INSERT INTO relationships (source_id, target_id, rel_type) VALUES (1, 3, 'imports')", []).unwrap();
        db
    }

    #[test]
    fn test_context_from_files() {
        let db = make_assembly_db();
        let req = ContextRequest {
            query: String::new(),
            files: vec!["src/auth.ts".to_string()],
            max_tokens: 10000,
            include_deps: false,
            dep_depth: 0,
            top_k: 10,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        assert!(resp.chunk_count >= 2); // at least login + logout
        assert!(resp.markdown.contains("login"));
    }

    #[test]
    fn test_context_with_deps() {
        let db = make_assembly_db();
        let req = ContextRequest {
            query: String::new(),
            files: vec!["src/auth.ts".to_string()],
            max_tokens: 10000,
            include_deps: true,
            dep_depth: 1,
            top_k: 10,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        // Should include db.connect since auth.login imports it
        assert!(resp.chunk_count >= 3);
        let has_connect = resp.sources.iter().any(|s| s.name.as_deref() == Some("connect"));
        assert!(has_connect, "should include connect dependency");
    }

    #[test]
    fn test_context_token_budget() {
        let db = make_assembly_db();
        let req = ContextRequest {
            query: String::new(),
            files: vec!["src/auth.ts".to_string(), "src/db.ts".to_string()],
            max_tokens: 50,
            include_deps: false,
            dep_depth: 0,
            top_k: 10,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        assert!(resp.token_count <= 50 + 100); // some tolerance for chunk boundary
    }

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(estimate_tokens("hello"), 1);
        assert!(estimate_tokens("fn foo() { bar(); }") > 0);
    }

    #[test]
    fn test_context_from_search() {
        let db = make_assembly_db();
        let req = ContextRequest {
            query: "login".to_string(),
            files: vec![],
            max_tokens: 10000,
            include_deps: false,
            dep_depth: 0,
            top_k: 5,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        assert!(resp.chunk_count >= 1);
        let has_login = resp.sources.iter().any(|s| s.name.as_deref() == Some("login"));
        assert!(has_login);
    }
}
