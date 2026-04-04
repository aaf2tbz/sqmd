#[cfg(feature = "embed")]
use crate::embed::{self, Embedder};
use rusqlite::{params, Connection};

#[derive(Debug, Clone)]
pub struct SearchQuery {
    pub text: String,
    pub top_k: usize,
    pub alpha: f64,
    pub file_filter: Option<String>,
    pub type_filter: Option<String>,
    pub source_type_filter: Option<Vec<String>>,
    pub agent_id_filter: Option<String>,
    pub min_score: f64,
}

impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            top_k: 10,
            alpha: 0.7,
            file_filter: None,
            type_filter: None,
            source_type_filter: None,
            agent_id_filter: None,
            min_score: 0.1,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub chunk_id: i64,
    pub file_path: String,
    pub name: Option<String>,
    pub signature: Option<String>,
    pub line_start: i64,
    pub line_end: i64,
    pub chunk_type: String,
    pub source_type: String,
    pub score: f64,
    pub vec_distance: Option<f64>,
    pub fts_rank: Option<f64>,
    pub snippet: Option<String>,
}

pub fn fts_search(db: &Connection, query: &SearchQuery) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let mut results = fts_content_search(db, query)?;

    if let Ok(hint_hits) = crate::entities::search_hints(db, &query.text, query.top_k * 2) {
        let (hint_ids, hint_scores): (Vec<i64>, Vec<f64>) = hint_hits.into_iter().unzip();
        let hint_score_map: std::collections::HashMap<i64, f64> = hint_ids.iter().zip(hint_scores.iter()).map(|(&id, &s)| (id, s)).collect();

        for r in &mut results {
            if let Some(&hs) = hint_score_map.get(&r.chunk_id) {
                r.score = r.score.max(hs);
                if r.snippet.is_none() {
                    r.snippet = Some("[hint match]".to_string());
                }
            }
        }

        for (id, score) in hint_ids.iter().zip(hint_scores.iter()) {
            if !results.iter().any(|r| r.chunk_id == *id) {
                let row = db.query_row(
                    "SELECT id, file_path, name, signature, line_start, line_end, chunk_type, source_type FROM chunks WHERE id = ?1 AND is_deleted = 0",
                    params![id],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, Option<String>>(2)?,
                           r.get::<_, Option<String>>(3)?, r.get::<_, i64>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?, r.get::<_, String>(7)?)),
                ).ok();
                if let Some((cid, file_path, name, sig, start, end, ct, st)) = row {
                    results.push(SearchResult {
                        chunk_id: cid,
                        file_path,
                        name,
                        signature: sig,
                        line_start: start,
                        line_end: end,
                        chunk_type: ct,
                        source_type: st,
                        score: *score,
                        vec_distance: None,
                        fts_rank: Some(*score),
                        snippet: Some("[hint match]".to_string()),
                    });
                }
            }
        }
    }

    if let Ok(boost_ids) = crate::entities::graph_boost_ids(db, &query.text, 50) {
        let boost_set: std::collections::HashSet<i64> = boost_ids.iter().copied().collect();
        for r in &mut results {
            if boost_set.contains(&r.chunk_id) {
                r.score = (r.score + 0.15).min(1.0);
            }
        }
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(query.top_k);

    Ok(results)
}

fn fts_content_search(db: &Connection, query: &SearchQuery) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let mut extra_clauses = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(query.text.clone()),
        Box::new(query.top_k as i64),
    ];
    let mut next_param = 3;

    if let Some(ref f) = query.file_filter {
        extra_clauses.push(format!("AND c.file_path = ?{}", next_param));
        param_values.push(Box::new(f.clone()));
        next_param += 1;
    }
    if let Some(ref t) = query.type_filter {
        extra_clauses.push(format!("AND c.chunk_type = ?{}", next_param));
        param_values.push(Box::new(t.clone()));
        next_param += 1;
    }
    if let Some(ref sources) = query.source_type_filter {
        if !sources.is_empty() {
            let placeholders: Vec<String> = sources.iter().enumerate().map(|(i, _)| format!("?{}", next_param + i)).collect();
            extra_clauses.push(format!("AND c.source_type IN ({})", placeholders.join(", ")));
            for s in sources {
                param_values.push(Box::new(s.clone()));
            }
            next_param += sources.len();
        }
    }
    if let Some(ref agent) = query.agent_id_filter {
        extra_clauses.push(format!("AND c.agent_id = ?{}", next_param));
        param_values.push(Box::new(agent.clone()));
        let _ = next_param;
    }

    let filter_clause = extra_clauses.join(" ");

    let sql = format!(
        "SELECT c.id, c.file_path, c.name, c.signature, c.line_start, c.line_end, c.chunk_type, f.rank, \
         snippet(chunks_fts, 2, '>>>', '<<<', '...', 32), c.source_type \
         FROM chunks_fts f JOIN chunks c ON f.rowid = c.id \
         WHERE chunks_fts MATCH ?1 AND c.is_deleted = 0 {} \
         ORDER BY f.rank LIMIT ?2",
        filter_clause
    );

    let mut stmt = db.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    type FtsRow = (i64, String, Option<String>, Option<String>, i64, i64, String, f64, String, String);
    let rows: Vec<FtsRow> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?))
        })?
        .collect::<Result<_, _>>()?;

    let min_rank = rows.iter().map(|r| r.7).fold(f64::INFINITY, f64::min);
    let max_rank = rows.iter().map(|r| r.7).fold(f64::NEG_INFINITY, f64::max);
    let rank_range = (max_rank - min_rank).abs().max(0.001);

    let results: Vec<SearchResult> = rows
        .into_iter()
        .map(|(id, file_path, name, sig, start, end, ct, rank, snippet, source_type)| {
            let normalized = 1.0 - ((rank - min_rank) / rank_range);
            SearchResult {
                chunk_id: id,
                file_path,
                name,
                signature: sig,
                line_start: start,
                line_end: end,
                chunk_type: ct,
                source_type,
                score: normalized,
                vec_distance: None,
                fts_rank: Some(normalized),
                snippet: Some(snippet),
            }
        })
        .collect();

    Ok(results)
}

pub fn vec_search(
    db: &Connection,
    query_vec: &[f32],
    top_k: usize,
    file_filter: Option<&str>,
    type_filter: Option<&str>,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let vec_count: i64 = db
        .query_row("SELECT COUNT(*) FROM chunks_vec", [], |r| r.get(0))
        .unwrap_or(0);
    if vec_count == 0 {
        return Ok(Vec::new());
    }

    let vec_str = format_vector(query_vec);
    let filter_clause = build_filter_clause_vec(file_filter, type_filter);

    let sql = format!(
        "SELECT v.rowid, v.distance, c.file_path, c.name, c.signature, c.line_start, c.line_end, c.chunk_type, c.source_type \
         FROM chunks_vec v JOIN chunks c ON v.rowid = c.id \
         WHERE v.embedding MATCH ?1 AND c.is_deleted = 0 {} ORDER BY v.distance LIMIT ?2",
        filter_clause
    );

    let mut stmt = db.prepare(&sql)?;

    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = vec![
        Box::new(vec_str),
        Box::new(top_k as i64),
    ];
    if let Some(f) = file_filter {
        param_values.insert(1, Box::new(f.to_string()));
    }
    if let Some(t) = type_filter {
        param_values.insert(param_values.len().saturating_sub(1), Box::new(t.to_string()));
    }
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    type VecRow = (i64, f64, String, Option<String>, Option<String>, i64, i64, String, String);
    let rows: Vec<VecRow> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?))
        })?
        .collect::<Result<_, _>>()?;

    let results: Vec<SearchResult> = rows
        .into_iter()
        .map(|(id, distance, file_path, name, sig, start, end, ct, source_type)| {
            SearchResult {
                chunk_id: id,
                file_path,
                name,
                signature: sig,
                line_start: start,
                line_end: end,
                chunk_type: ct,
                source_type,
                score: 1.0 - distance.min(1.0),
                vec_distance: Some(distance),
                fts_rank: None,
                snippet: None,
            }
        })
        .collect();

    Ok(results)
}

#[cfg(feature = "embed")]
pub fn hybrid_search(
    db: &Connection,
    query: &SearchQuery,
    embedder: &mut Embedder,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let fts_results = fts_search(db, query)?;

    let vec_results = if embedder.is_available() {
        let query_vec = embedder.embed_one(&query.text)?;
        vec_search(
            db,
            &query_vec,
            query.top_k * 2,
            query.file_filter.as_deref(),
            query.type_filter.as_deref(),
        )?
    } else {
        Vec::new()
    };

    let merged = merge_results(fts_results, vec_results, query.alpha);
    let filtered: Vec<SearchResult> = merged
        .into_iter()
        .filter(|r| r.score >= query.min_score)
        .take(query.top_k)
        .collect();

    Ok(filtered)
}

#[cfg(feature = "embed")]
fn merge_results(
    mut fts: Vec<SearchResult>,
    mut vec: Vec<SearchResult>,
    alpha: f64,
) -> Vec<SearchResult> {
    let mut by_id: std::collections::HashMap<i64, SearchResult> = std::collections::HashMap::new();

    for r in fts.drain(..) {
        by_id.entry(r.chunk_id).or_insert(r);
    }

    for r in vec.drain(..) {
        let entry = by_id.entry(r.chunk_id).or_insert(r.clone());
        if entry.vec_distance.is_none() && r.vec_distance.is_some() {
            entry.vec_distance = r.vec_distance;
        }
    }

    let mut results: Vec<SearchResult> = by_id.into_values().collect();

    for r in &mut results {
        let fts_score = r.fts_rank.unwrap_or(0.0);
        let vec_score = 1.0 - r.vec_distance.unwrap_or(1.0);

        let has_both = r.fts_rank.is_some() && r.vec_distance.is_some();
        let penalty = if has_both { 1.0 } else { 0.8 };

        r.score = (alpha * vec_score + (1.0 - alpha) * fts_score) * penalty;
    }

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn build_filter_clause_vec(file_filter: Option<&str>, type_filter: Option<&str>) -> String {
    match (file_filter, type_filter) {
        (Some(_), Some(_)) => "AND c.file_path = ?2 AND c.chunk_type = ?3".to_string(),
        (Some(_), None) => "AND c.file_path = ?2".to_string(),
        (None, Some(_)) => "AND c.chunk_type = ?2".to_string(),
        (None, None) => String::new(),
    }
}

fn format_vector(vec: &[f32]) -> String {
    let items: Vec<String> = vec.iter().map(|v| format!("{:.6}", v)).collect();
    format!("[{}]", items.join(", "))
}

#[cfg(feature = "embed")]
pub fn store_embedding(
    db: &Connection,
    chunk_id: i64,
    vector: &[f32],
    model_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let blob = embed::vector_to_blob(vector);
    db.execute(
        "INSERT OR REPLACE INTO embeddings (chunk_id, vector, dimensions, model) VALUES (?1, ?2, ?3, ?4)",
        params![chunk_id, blob, vector.len() as i64, model_name],
    )?;

    db.execute(
        "INSERT OR REPLACE INTO chunks_vec(rowid, embedding) VALUES (?1, ?2)",
        params![chunk_id, format_vector(vector)],
    )?;

    Ok(())
}

pub fn get_unembedded_chunk_ids(db: &Connection, limit: usize) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT c.id FROM chunks c LEFT JOIN embeddings e ON c.id = e.chunk_id WHERE e.chunk_id IS NULL ORDER BY c.importance DESC LIMIT ?1"
    )?;
    let ids: Vec<i64> = stmt.query_map(params![limit as i64], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

#[cfg(feature = "embed")]
pub fn embed_unembedded(
    db: &mut Connection,
    embedder: &mut Embedder,
) -> Result<usize, Box<dyn std::error::Error>> {
    let ids = get_unembedded_chunk_ids(db, 64)?;
    if ids.is_empty() {
        return Ok(0);
    }

    let placeholders: Vec<String> = ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
    let sql = format!(
        "SELECT id, COALESCE(name || ' ', '') || COALESCE(signature || ' ', '') || COALESCE(content_raw, '') FROM chunks WHERE id IN ({})",
        placeholders.join(", ")
    );
    let params: Vec<&dyn rusqlite::ToSql> = ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let mut stmt = db.prepare(&sql)?;

    let texts: Vec<(i64, String)> = stmt
        .query_map(params.as_slice(), |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<Result<_, _>>()?;

    let text_refs: Vec<&str> = texts.iter().map(|(_, t)| t.as_str()).collect();
    let vectors = embedder.embed_batch(&text_refs)?;

    db.execute_batch("BEGIN")?;
    for ((chunk_id, _), vector) in texts.iter().zip(vectors.iter()) {
        store_embedding(db, *chunk_id, vector, embedder.model_name())?;
    }
    db.execute_batch("COMMIT")?;

    Ok(texts.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db() -> Connection {
        let mut db = Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute(
            "INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/auth.ts', 'typescript', 100, 0.0, 'abc')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/db.ts', 'typescript', 50, 0.0, 'def')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash)
             VALUES (1, 'src/auth.ts', 'typescript', 'function', 'login', 'login(user: string)', 0, 5, 'async function login(user) { return db.find(user); }', 'xyz')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash)
             VALUES (2, 'src/auth.ts', 'typescript', 'function', 'logout', 'logout()', 7, 10, 'function logout() { session.clear(); }', 'uvw')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO chunks (id, file_path, language, chunk_type, name, signature, line_start, line_end, content_raw, content_hash)
             VALUES (3, 'src/db.ts', 'typescript', 'function', 'connect', 'connect(url: string)', 0, 3, 'async function connect(url) { return pool(url); }', 'qrs')",
            [],
        )
        .unwrap();
        db
    }

    #[test]
    fn test_fts_search() {
        let db = make_test_db();
        let query = SearchQuery {
            text: "login user".to_string(),
            top_k: 5,
            ..Default::default()
        };
        let results = fts_search(&db, &query).unwrap();
        assert!(!results.is_empty());
        assert!(results[0].fts_rank.is_some());
        assert!(results[0].snippet.is_some());
    }

    #[test]
    fn test_fts_search_with_file_filter() {
        let db = make_test_db();
        let query = SearchQuery {
            text: "function".to_string(),
            top_k: 10,
            file_filter: Some("src/db.ts".to_string()),
            ..Default::default()
        };
        let results = fts_search(&db, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].file_path, "src/db.ts");
    }

    #[test]
    fn test_vec_search_with_fallback() {
        let db = make_test_db();
        let query_vec = vec![0.1f32; 768];
        let results = vec_search(&db, &query_vec, 5, None, None).unwrap();
        // chunks_vec exists but is empty — should return empty, not error
        assert!(results.is_empty());
    }

    #[cfg(feature = "embed")]
    #[test]
    fn test_hybrid_search_fts_only() {
        let db = make_test_db();
        let mut embedder = Embedder::new().unwrap();
        // Don't load model — test FTS-only fallback path
        let query = SearchQuery {
            text: "login".to_string(),
            top_k: 5,
            alpha: 0.7,
            ..Default::default()
        };
        let results = hybrid_search(&db, &query, &mut embedder).unwrap();
        // With no model, we get FTS results only (penalized by 0.8)
        assert!(!results.is_empty());
    }

    #[cfg(feature = "embed")]
    #[test]
    fn test_get_unembedded_chunk_ids() {
        let db = make_test_db();
        let ids = get_unembedded_chunk_ids(&db, 10).unwrap();
        assert_eq!(ids.len(), 3);

        // Embed one
        let vec = vec![0.5f32; 768];
        store_embedding(&db, 1, &vec, "test-model").unwrap();

        let ids = get_unembedded_chunk_ids(&db, 10).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(!ids.contains(&1));
    }

    #[cfg(feature = "embed")]
    #[test]
    fn test_store_and_search_vector() {
        let db = make_test_db();
        let v1 = vec![1.0f32, 0.0, 0.0];
        let v2 = vec![0.0, 1.0, 0.0];
        // These won't match our schema (768 dims) but let's test the insert path

        // Insert embeddings
        let blob1 = embed::vector_to_blob(&v1);
        let blob2 = embed::vector_to_blob(&v2);
        db.execute(
            "INSERT INTO embeddings (chunk_id, vector, dimensions, model) VALUES (1, ?1, 3, 'test')",
            params![blob1],
        )
        .unwrap();
        db.execute(
            "INSERT INTO embeddings (chunk_id, vector, dimensions, model) VALUES (2, ?1, 3, 'test')",
            params![blob2],
        )
        .unwrap();

        // Verify roundtrip
        let blob: Vec<u8> = db
            .query_row("SELECT vector FROM embeddings WHERE chunk_id = 1", [], |r| r.get(0))
            .unwrap();
        let restored = embed::blob_to_vector(&blob);
        assert_eq!(restored.len(), 3);
        assert!((restored[0] - 1.0).abs() < f32::EPSILON);
    }
}
