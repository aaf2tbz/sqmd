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
    pub source_types: Option<Vec<String>>,
    #[serde(default = "default_max_dep_chunks")]
    pub max_dep_chunks: usize,
    #[serde(default = "default_community_boost")]
    pub community_boost: f64,
}

fn default_max_dep_chunks() -> usize {
    50
}

fn default_community_boost() -> f64 {
    0.1
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
    pub source_type: String,
}

type SelectedChunk = (
    i64,
    String,
    Option<String>,
    String,
    i64,
    i64,
    String,
    String,
    f64,
    String,
    String,
);

pub struct ContextAssembler;

impl ContextAssembler {
    pub fn build(
        db: &Connection,
        request: &ContextRequest,
    ) -> Result<ContextResponse, Box<dyn std::error::Error>> {
        let mut selected: Vec<SelectedChunk> = Vec::new();
        let mut seen_ids: std::collections::HashSet<i64> = std::collections::HashSet::new();

        if !request.query.is_empty() {
            let search_query = crate::search::SearchQuery {
                text: request.query.clone(),
                top_k: request.top_k,
                source_type_filter: request.source_types.clone(),
                ..Default::default()
            };
            #[cfg(feature = "native")]
            let results = {
                match crate::embed::make_provider() {
                    Ok(mut provider) => {
                        crate::search::layered_search(db, &search_query, Some(&mut *provider))
                            .map(|lr| lr.results)?
                    }
                    Err(_) => crate::search::fts_search(db, &search_query)?,
                }
            };
            #[cfg(not(feature = "native"))]
            let results = crate::search::fts_search(db, &search_query)?;
            for r in &results {
                if seen_ids.insert(r.chunk_id) {
                    let (content, language, source_type) =
                        get_chunk_content_and_lang(db, r.chunk_id)?;
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
                        language,
                        source_type,
                    ));
                }
            }
        }

        for file_path in &request.files {
            let mut stmt = db.prepare(
                "SELECT id, file_path, name, chunk_type, line_start, line_end, importance, content_raw, language, COALESCE(source_type, '')
                 FROM chunks WHERE file_path = ?1 AND importance >= 0.5
                 ORDER BY importance DESC",
            )?;
            #[allow(clippy::type_complexity)]
            let rows: Vec<(
                i64,
                String,
                Option<String>,
                String,
                i64,
                i64,
                f64,
                String,
                String,
                String,
            )> = stmt
                .query_map(rusqlite::params![file_path], |r| {
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
                        r.get(9)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;

            for (id, fp, name, ct, ls, le, imp, content, lang, st) in rows {
                if seen_ids.insert(id) {
                    selected.push((
                        id,
                        fp,
                        name,
                        ct,
                        ls,
                        le,
                        format!("{:.2}", imp),
                        content,
                        imp,
                        lang,
                        st,
                    ));
                }
            }
        }

        if request.include_deps {
            let chunk_ids: Vec<i64> = selected.iter().map(|(id, ..)| *id).collect();
            if !chunk_ids.is_empty() {
                let seed_community_paths = get_seed_community_paths(db, &chunk_ids);
                let effective_depth = if request.dep_depth == 0 {
                    2
                } else {
                    request.dep_depth
                };
                let remaining_budget = request.max_tokens.saturating_sub(
                    selected
                        .iter()
                        .map(|(_, _, _, _, _, _, _, content, _, _, _)| estimate_tokens(content))
                        .sum::<usize>(),
                );
                let estimated_chunks = (remaining_budget / 200).max(5).min(request.max_dep_chunks);
                let deps = get_related_chunks(
                    db,
                    &chunk_ids,
                    effective_depth,
                    estimated_chunks,
                    &seed_community_paths,
                    request.community_boost,
                )?;
                for (id, fp, name, ct, ls, le, content, lang, st) in &deps {
                    if seen_ids.insert(*id) {
                        selected.push((
                            *id,
                            fp.clone(),
                            name.clone(),
                            ct.clone(),
                            *ls,
                            *le,
                            "0.5".to_string(),
                            content.clone(),
                            0.5,
                            lang.clone(),
                            st.clone(),
                        ));
                    }
                }
            }
        }

        // 4. Filter by source_types if specified
        if let Some(ref allowed) = request.source_types {
            selected.retain(|chunk| allowed.contains(&chunk.10));
        }

        // 5. Sort by score descending, then render with token budget
        selected.sort_by(|a, b| b.8.partial_cmp(&a.8).unwrap_or(std::cmp::Ordering::Equal));

        let mut markdown = String::new();
        let mut token_count = 0;
        let mut sources = Vec::new();

        for (
            _id,
            file_path,
            name,
            chunk_type,
            line_start,
            line_end,
            _score,
            content,
            score,
            language,
            source_type,
        ) in &selected
        {
            let rendered = render_chunk(
                file_path,
                name,
                chunk_type,
                *line_start,
                *line_end,
                content,
                language,
            );
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
                source_type: source_type.clone(),
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
    language: &str,
) -> String {
    let name = name.as_deref().unwrap_or("(unnamed)");
    format!(
        "### `{}`\n\n**File:** `{}` | **Lines:** {}-{} | **Type:** {}\n\n```{}\n{}\n```\n",
        name,
        file_path,
        line_start + 1,
        line_end + 1,
        chunk_type,
        language,
        content.trim(),
    )
}

fn get_chunk_content_and_lang(
    db: &Connection,
    chunk_id: i64,
) -> Result<(String, String, String), Box<dyn std::error::Error>> {
    let result: (String, String, String) = db
        .query_row(
            "SELECT content_raw, COALESCE(language, ''), COALESCE(source_type, '') FROM chunks WHERE id = ?1",
            rusqlite::params![chunk_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap_or_default();
    Ok(result)
}

fn get_seed_community_paths(db: &Connection, chunk_ids: &[i64]) -> Vec<String> {
    if chunk_ids.is_empty() {
        return Vec::new();
    }
    let placeholders: Vec<String> = (0..chunk_ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect();
    let ph = placeholders.join(", ");

    let sql = format!(
        "SELECT DISTINCT cm.path FROM communities cm \
         WHERE EXISTS (SELECT 1 FROM chunks c WHERE c.id IN ({ph}) AND (c.file_path = cm.path OR c.file_path LIKE cm.path || '/%')) \
         ORDER BY cm.depth ASC LIMIT 10",
        ph = ph
    );

    let mut stmt = match db.prepare(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let params: Vec<&dyn rusqlite::ToSql> = chunk_ids
        .iter()
        .map(|id| id as &dyn rusqlite::ToSql)
        .collect();
    let result = match stmt.query_map(params.as_slice(), |r| r.get::<_, String>(0)) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect::<Vec<String>>(),
        Err(_) => Vec::new(),
    };
    result
}

#[allow(clippy::type_complexity)]
fn get_related_chunks(
    db: &Connection,
    chunk_ids: &[i64],
    depth: usize,
    max_chunks: usize,
    seed_community_paths: &[String],
    community_boost: f64,
) -> Result<
    Vec<(
        i64,
        String,
        Option<String>,
        String,
        i64,
        i64,
        String,
        String,
        String,
    )>,
    Box<dyn std::error::Error>,
> {
    if chunk_ids.is_empty() || depth == 0 {
        return Ok(Vec::new());
    }

    let placeholders: Vec<String> = (0..chunk_ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect();
    let ph = placeholders.join(", ");

    let community_clause = if seed_community_paths.is_empty() {
        String::new()
    } else {
        let path_placeholders: Vec<String> = (0..seed_community_paths.len())
            .map(|i| format!("?{}", chunk_ids.len() + i + 1))
            .collect();
        let pph = path_placeholders.join(", ");
        format!(
            ", community_bonus AS (SELECT c.id, {boost} AS bonus FROM chunks c WHERE (c.file_path IN ({pph})) )",
            boost = community_boost,
            pph = pph,
        )
    };

    let community_join = if seed_community_paths.is_empty() {
        String::new()
    } else {
        "LEFT JOIN community_bonus cb ON c.id = cb.id".to_string()
    };

    let community_score = if seed_community_paths.is_empty() {
        "s.rel_score".to_string()
    } else {
        "s.rel_score + COALESCE(cb.bonus, 0.0)".to_string()
    };

    let sql = format!(
        "WITH rel_graph(id, depth, rel_type) AS (
            SELECT target_id, 1, r.rel_type FROM relationships r
            WHERE r.source_id IN ({ph}) AND r.rel_type IN ('imports','calls','contains','extends','implements')
            UNION
            SELECT target_id, rg.depth + 1, r.rel_type FROM relationships r
            JOIN rel_graph rg ON r.source_id = rg.id
            WHERE rg.depth < {depth}
              AND r.rel_type IN ('imports','calls','contains','extends','implements')
            UNION
            SELECT source_id, rg.depth + 1, r.rel_type FROM relationships r
            JOIN rel_graph rg ON r.target_id = rg.id
            WHERE rg.depth < {depth}
              AND r.rel_type IN ('calls','contains')
        ),
        ent_graph(id, depth) AS (
            SELECT DISTINCT e2.chunk_id, 1
            FROM entities e1
            JOIN entity_dependencies ed ON e1.id = ed.source_entity AND ed.valid_to IS NULL
            JOIN entities e2 ON ed.target_entity = e2.id
            WHERE e1.chunk_id IN ({ph}) AND e2.chunk_id IS NOT NULL AND e2.chunk_id != e1.chunk_id
            UNION
            SELECT DISTINCT e1.chunk_id, 1
            FROM entities e1
            JOIN entity_dependencies ed ON e1.id = ed.target_entity AND ed.valid_to IS NULL
            JOIN entities e2 ON ed.source_entity = e2.id
            WHERE e1.chunk_id IN ({ph}) AND e2.chunk_id IS NOT NULL AND e1.chunk_id != e2.chunk_id
        ),
        all_graph(id, depth, rel_type) AS (
            SELECT id, depth, rel_type FROM rel_graph
            UNION
            SELECT id, depth, 'entity' FROM ent_graph WHERE depth <= {depth}
        ),
        scored AS (
            SELECT ag.id,
                   ag.depth,
                   ag.rel_type,
                   CASE ag.rel_type
                       WHEN 'calls' THEN 0.9
                       WHEN 'contains' THEN 0.4
                       WHEN 'imports' THEN 0.7
                       WHEN 'extends' THEN 0.6
                       WHEN 'implements' THEN 0.6
                       WHEN 'entity' THEN 0.3
                       ELSE 0.5
                   END as rel_score
            FROM all_graph ag
        ){community_clause}
        SELECT DISTINCT c.id, c.file_path, c.name, c.chunk_type, c.line_start, c.line_end, c.content_raw, COALESCE(c.language, ''), COALESCE(c.source_type, '')
        FROM scored s
        JOIN chunks c ON s.id = c.id
        {community_join}
        WHERE c.id NOT IN ({ph}) AND c.is_deleted = 0
        ORDER BY {community_score} DESC, c.importance DESC
        LIMIT {max_chunks}",
        ph = ph,
        community_clause = community_clause,
        community_join = community_join,
        community_score = community_score,
        depth = depth,
        max_chunks = max_chunks
    );

    let mut stmt = db.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = chunk_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
        .collect();
    for path in seed_community_paths {
        params.push(Box::new(path.clone()));
    }
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows: Vec<(
        i64,
        String,
        Option<String>,
        String,
        i64,
        i64,
        String,
        String,
        String,
    )> = stmt
        .query_map(param_refs.as_slice(), |r| {
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
        .collect::<Result<_, _>>()?;

    Ok(rows)
}

#[allow(clippy::integer_division)]
pub fn estimate_tokens(text: &str) -> usize {
    // cl100k_base approximation: ~3.4 chars/token for code
    let chars = text.len();
    (chars * 10).div_ceil(36)
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
        db.execute(
            "INSERT INTO relationships (source_id, target_id, rel_type) VALUES (1, 3, 'imports')",
            [],
        )
        .unwrap();
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
            source_types: None,
            max_dep_chunks: 50,
            community_boost: 0.1,
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
            source_types: None,
            max_dep_chunks: 50,
            community_boost: 0.1,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        // Should include db.connect since auth.login imports it
        assert!(resp.chunk_count >= 3);
        let has_connect = resp
            .sources
            .iter()
            .any(|s| s.name.as_deref() == Some("connect"));
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
            source_types: None,
            max_dep_chunks: 50,
            community_boost: 0.1,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        assert!(resp.token_count <= 50 + 100); // some tolerance for chunk boundary
    }

    #[test]
    fn test_estimate_tokens() {
        assert!(estimate_tokens("hello") >= 1);
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
            source_types: None,
            max_dep_chunks: 50,
            community_boost: 0.1,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        assert!(resp.chunk_count >= 1);
        let has_login = resp
            .sources
            .iter()
            .any(|s| s.name.as_deref() == Some("login"));
        assert!(has_login);
    }

    #[test]
    fn test_context_source_type_filter() {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute(
            "INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/app.ts', 'typescript', 200, 0.0, 'a')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, source_type, importance)
             VALUES (1, 'src/app.ts', 'typescript', 'function', 'login', 'login(u: string)', 0, 5, 'async function login(u) { return db.find(u); }', 'x', 'code', 0.9)",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash, source_type, importance)
             VALUES (2, 'src/app.ts', 'typescript', 'fact', 'note', '()', 7, 10, 'Users prefer dark mode', 'y', 'memory', 0.8)",
            [],
        )
        .unwrap();

        let req = ContextRequest {
            query: "login user".to_string(),
            files: vec![],
            max_tokens: 10000,
            include_deps: false,
            dep_depth: 0,
            top_k: 10,
            source_types: Some(vec!["code".to_string()]),
            max_dep_chunks: 50,
            community_boost: 0.1,
        };
        let resp = ContextAssembler::build(&db, &req).unwrap();
        assert!(
            resp.sources.iter().all(|s| s.source_type == "code"),
            "should only contain code chunks, got: {:?}",
            resp.sources
                .iter()
                .map(|s| &s.source_type)
                .collect::<Vec<_>>()
        );
        assert!(resp.chunk_count >= 1);
        let has_login = resp
            .sources
            .iter()
            .any(|s| s.name.as_deref() == Some("login"));
        assert!(has_login, "should include login function");
    }
}
