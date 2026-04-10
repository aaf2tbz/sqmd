use crate::dampening;
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
    pub exclude_path_prefixes: Vec<String>,
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
            exclude_path_prefixes: Vec::new(),
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
    pub decay_rate: f64,
    pub last_accessed: Option<String>,
    pub importance: f64,
}

pub fn fts_search(
    db: &Connection,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    db.execute_batch("BEGIN")?;
    let result = fts_search_inner(db, query);
    match &result {
        Ok(_) => db.execute_batch("COMMIT")?,
        Err(_) => db.execute_batch("ROLLBACK")?,
    }
    result
}

const SHORT_CIRCUIT_THRESHOLD: f64 = 0.7;
const MIN_SHORT_CIRCUIT_HITS: usize = 3;

pub fn layered_search(
    db: &Connection,
    query: &SearchQuery,
) -> Result<LayeredResult, Box<dyn std::error::Error>> {
    let mut all_results: Vec<SearchResult> = Vec::new();
    let mut layers_hit: Vec<String> = Vec::new();

    // Layer 1: FTS
    let fts_results = fts_search(db, query).unwrap_or_default();
    let fts_top_score = fts_results.first().map(|r| r.score).unwrap_or(0.0);

    if !fts_results.is_empty() {
        layers_hit.push("fts".to_string());
        let high_confidence = fts_results
            .iter()
            .filter(|r| r.score >= SHORT_CIRCUIT_THRESHOLD)
            .count();
        all_results.extend(fts_results);

        if high_confidence >= MIN_SHORT_CIRCUIT_HITS && fts_top_score > 0.85 {
            all_results.sort_by(|a, b| {
                b.score
                    .partial_cmp(&a.score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            all_results.truncate(query.top_k);
            return Ok(LayeredResult {
                results: all_results,
                layers_hit,
            });
        }
    }

    // Layer 2: Graph traversal expansion
    if let Ok(graph_chunks) = graph_expansion_search(db, &query.text, query.top_k) {
        if !graph_chunks.is_empty() {
            layers_hit.push("graph".to_string());
            let existing_ids: std::collections::HashSet<i64> =
                all_results.iter().map(|r| r.chunk_id).collect();
            for mut gc in graph_chunks {
                if !existing_ids.contains(&gc.chunk_id) {
                    gc.score *= 0.7;
                    all_results.push(gc);
                }
            }
        }
    }

    // Layer 3: Community summaries
    if let Ok(community_hits) = community_summary_search(db, &query.text) {
        if !community_hits.is_empty() {
            layers_hit.push("community".to_string());
            let existing_ids: std::collections::HashSet<i64> =
                all_results.iter().map(|r| r.chunk_id).collect();
            for mut ch in community_hits {
                if !existing_ids.contains(&ch.chunk_id) {
                    ch.score *= 0.5;
                    all_results.push(ch);
                }
            }
        }
    }

    all_results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    dampening::importance_boost(&mut all_results);
    dampening::dampen(&mut all_results, 0.8);
    all_results.truncate(query.top_k);

    Ok(LayeredResult {
        results: all_results,
        layers_hit,
    })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LayeredResult {
    pub results: Vec<SearchResult>,
    pub layers_hit: Vec<String>,
}

fn graph_expansion_search(
    db: &Connection,
    query: &str,
    top_k: usize,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let scored = crate::entities::graph_boost_scored(db, query, top_k)?;
    if scored.is_empty() {
        return Ok(Vec::new());
    }

    let chunk_ids: Vec<i64> = scored.iter().map(|(id, _)| *id).collect();
    let score_map: std::collections::HashMap<i64, f64> = scored.into_iter().collect();

    let placeholders: Vec<String> = (0..chunk_ids.len())
        .map(|i| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT id, file_path, name, signature, line_start, line_end, chunk_type, source_type, importance, updated_at \
         FROM chunks WHERE id IN ({}) AND is_deleted = 0",
        placeholders.join(", ")
    );
    let mut stmt = db.prepare(&sql)?;
    let param_vals: Vec<Box<dyn rusqlite::ToSql>> = chunk_ids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
        .collect();
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_vals.iter().map(|p| p.as_ref()).collect();

    type GraphChunkRow = (
        i64,
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        String,
        String,
        f64,
        Option<String>,
    );
    let now = chrono_now();
    let rows: Vec<GraphChunkRow> = stmt
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
                r.get(9)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let results: Vec<SearchResult> = rows
        .into_iter()
        .map(|(id, fp, name, sig, ls, le, ct, st, importance, ua)| {
            let graph_score = score_map.get(&id).copied().unwrap_or(0.0);
            let recency = compute_recency(ua.as_deref(), &now);
            let score = three_factor_score(graph_score, recency, importance);
            SearchResult {
                chunk_id: id,
                file_path: fp,
                name,
                signature: sig,
                line_start: ls,
                line_end: le,
                chunk_type: ct,
                source_type: st,
                score,
                vec_distance: None,
                fts_rank: None,
                snippet: Some("[graph expansion]".to_string()),
                decay_rate: 0.0,
                last_accessed: None,
                importance,
            }
        })
        .collect();

    Ok(results)
}

fn community_summary_search(
    db: &Connection,
    query: &str,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let communities = crate::communities::search_communities(db, query, 3)?;
    if communities.is_empty() {
        return Ok(Vec::new());
    }

    let mut results: Vec<SearchResult> = Vec::new();
    for community in &communities {
        if let Ok(chunks) = crate::communities::get_community_chunks(db, community.id) {
            let _now = chrono_now();
            for (cid, fp, name, ct, ls, le) in chunks {
                if let Ok(importance) = db.query_row(
                    "SELECT importance FROM chunks WHERE id = ?1",
                    params![cid],
                    |r| r.get::<_, f64>(0),
                ) {
                    let score = three_factor_score(0.5, 1.0, importance);
                    results.push(SearchResult {
                        chunk_id: cid,
                        file_path: fp,
                        name: name.clone(),
                        signature: None,
                        line_start: ls,
                        line_end: le,
                        chunk_type: ct,
                        source_type: "code".to_string(),
                        score: score * 0.6,
                        vec_distance: None,
                        fts_rank: None,
                        snippet: Some(format!("[community: {}]", community.path)),
                        decay_rate: 0.0,
                        last_accessed: None,
                        importance,
                    });
                }
            }
        }
    }

    Ok(results)
}

fn fts_search_inner(
    db: &Connection,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let mut results = fts_content_search(db, query)?;

    if let Ok(hint_hits) = crate::entities::search_hints(db, &query.text, query.top_k * 2) {
        let (hint_ids, hint_scores): (Vec<i64>, Vec<f64>) = hint_hits.into_iter().unzip();
        let hint_score_map: std::collections::HashMap<i64, f64> = hint_ids
            .iter()
            .zip(hint_scores.iter())
            .map(|(&id, &s)| (id, s))
            .collect();

        for r in &mut results {
            if let Some(&hs) = hint_score_map.get(&r.chunk_id) {
                r.score = r.score.max(hs);
                if r.snippet.is_none() {
                    r.snippet = Some("[hint match]".to_string());
                }
            }
        }

        let existing_ids: std::collections::HashSet<i64> =
            results.iter().map(|r| r.chunk_id).collect();
        let missing: Vec<(i64, f64)> = hint_ids
            .iter()
            .zip(hint_scores.iter())
            .filter(|(id, _)| !existing_ids.contains(id))
            .map(|(&id, &s)| (id, s))
            .collect();

        if !missing.is_empty() {
            let placeholders: Vec<String> =
                (0..missing.len()).map(|i| format!("?{}", i + 1)).collect();
            let sql = format!(
                "SELECT id, file_path, name, signature, line_start, line_end, chunk_type, source_type, importance \
                 FROM chunks WHERE id IN ({}) AND is_deleted = 0",
                placeholders.join(", ")
            );
            let mut stmt = db.prepare(&sql)?;
            let param_vals: Vec<Box<dyn rusqlite::ToSql>> = missing
                .iter()
                .map(|(id, _)| Box::new(*id) as Box<dyn rusqlite::ToSql>)
                .collect();
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                param_vals.iter().map(|p| p.as_ref()).collect();

            type HintRow = (
                i64,
                String,
                Option<String>,
                Option<String>,
                i64,
                i64,
                String,
                String,
                f64,
            );
            let rows: Vec<HintRow> = stmt
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

            let row_map: std::collections::HashMap<i64, _> =
                rows.into_iter().map(|r| (r.0, r)).collect();
            for (id, score) in &missing {
                if let Some((cid, file_path, name, sig, start, end, ct, st, importance)) =
                    row_map.get(id)
                {
                    results.push(SearchResult {
                        chunk_id: *cid,
                        file_path: file_path.clone(),
                        name: name.clone(),
                        signature: sig.clone(),
                        line_start: *start,
                        line_end: *end,
                        chunk_type: ct.clone(),
                        source_type: st.clone(),
                        score: *score,
                        vec_distance: None,
                        fts_rank: Some(*score),
                        snippet: Some("[hint match]".to_string()),
                        decay_rate: 0.0,
                        last_accessed: None,
                        importance: *importance,
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

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    dampening::importance_boost(&mut results);
    dampening::dampen(&mut results, 0.8);

    results.truncate(query.top_k);

    Ok(results)
}

fn escape_fts_query(input: &str) -> String {
    let tokens: Vec<String> = input
        .split_whitespace()
        .filter(|t| t.len() > 1)
        .map(|t| {
            let escaped: String = t
                .chars()
                .map(|c| if "\"*():^?".contains(c) { ' ' } else { c })
                .collect();
            escaped
        })
        .filter(|t| !t.is_empty())
        .collect();
    tokens.join(" OR ")
}

fn fts_content_search(
    db: &Connection,
    query: &SearchQuery,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let mut extra_clauses = Vec::new();
    let fts_safe = escape_fts_query(&query.text);
    let fts_candidate_limit: usize = 200;

    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(fts_safe), Box::new(fts_candidate_limit as i64)];
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
            let placeholders: Vec<String> = sources
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", next_param + i))
                .collect();
            extra_clauses.push(format!(
                "AND c.source_type IN ({})",
                placeholders.join(", ")
            ));
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
    for prefix in &query.exclude_path_prefixes {
        extra_clauses.push(format!(
            "AND c.file_path NOT LIKE '{}%'",
            prefix.replace('\'', "''")
        ));
    }

    let filter_clause = extra_clauses.join(" ");

    let sql = format!(
        "SELECT c.id, c.file_path, c.name, c.signature, c.line_start, c.line_end, c.chunk_type, f.rank, \
         snippet(chunks_fts, 2, '>>>', '<<<', '...', 32), c.source_type, c.decay_rate, c.last_accessed, c.importance, c.updated_at \
         FROM chunks_fts f JOIN chunks c ON f.rowid = c.id \
         WHERE chunks_fts MATCH ?1 AND c.is_deleted = 0 {} \
         ORDER BY bm25(chunks_fts, 10.0, 5.0, 1.0, 1.0) LIMIT ?2",
        filter_clause
    );

    let mut stmt = db.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    type FtsRow = (
        i64,
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        String,
        f64,
        String,
        String,
        f64,
        Option<String>,
        f64,
        Option<String>,
    );
    let rows: Vec<FtsRow> = stmt
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
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
                r.get(13)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let min_rank = rows.iter().map(|r| r.7).fold(f64::INFINITY, f64::min);
    let max_rank = rows.iter().map(|r| r.7).fold(f64::NEG_INFINITY, f64::max);
    let rank_range = (max_rank - min_rank).abs().max(0.001);

    let now = chrono_now();
    let query_tokens: Vec<String> = query
        .text
        .split_whitespace()
        .filter(|t| t.len() > 2)
        .map(|t| t.to_lowercase())
        .collect();

    struct ScoredChunk {
        idx: usize,
        score: f64,
        #[allow(dead_code)]
        raw: f64,
        #[allow(dead_code)]
        norm: f64,
    }

    let name_hit_ids: std::collections::HashSet<i64> = if !query_tokens.is_empty() {
        let like_clauses: Vec<String> = query_tokens
            .iter()
            .map(|t| format!("LOWER(c.name) LIKE '%{}%'", t.replace('\'', "''")))
            .collect();
        let mut path_clauses = Vec::new();
        for prefix in &query.exclude_path_prefixes {
            path_clauses.push(format!(
                "AND c.file_path NOT LIKE '{}%'",
                prefix.replace('\'', "''")
            ));
        }
        let path_filter = path_clauses.join(" ");
        let sql = format!(
            "SELECT DISTINCT c.id FROM chunks c WHERE ({}) AND c.is_deleted = 0 {} LIMIT 200",
            like_clauses.join(" OR "),
            path_filter
        );
        if let Ok(mut stmt) = db.prepare(&sql) {
            stmt.query_map([], |r| r.get::<_, i64>(0))
                .ok()
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        }
    } else {
        std::collections::HashSet::new()
    };

    let scored: Vec<ScoredChunk> = rows
        .iter()
        .enumerate()
        .map(
            |(i, (_, _, name, sig, _, _, _ct, rank, _, _, _, _, importance, updated_at))| {
                let normalized = 1.0 - ((rank - min_rank) / rank_range);
                let name_bonus = name_match_bonus(name, sig, &query_tokens);
                let recency = compute_recency(updated_at.as_deref(), &now);
                let base_score = (normalized + name_bonus).min(1.0);
                let score = three_factor_score(base_score, recency, *importance);
                let score = if name_hit_ids.contains(&rows[i].0) {
                    score * 1.4
                } else {
                    score
                };
                ScoredChunk {
                    idx: i,
                    score,
                    raw: score,
                    norm: normalized,
                }
            },
        )
        .collect();

    let results: Vec<SearchResult> = scored
        .into_iter()
        .map(|s| {
            let (
                id,
                file_path,
                name,
                sig,
                start,
                end,
                ct,
                _rank,
                snippet,
                source_type,
                decay_rate,
                last_accessed,
                importance,
                _updated_at,
            ) = &rows[s.idx];
            SearchResult {
                chunk_id: *id,
                file_path: file_path.clone(),
                name: name.clone(),
                signature: sig.clone(),
                line_start: *start,
                line_end: *end,
                chunk_type: ct.clone(),
                source_type: source_type.clone(),
                score: s.score,
                vec_distance: None,
                fts_rank: Some(s.norm),
                snippet: Some(snippet.clone()),
                decay_rate: *decay_rate,
                last_accessed: last_accessed.clone(),
                importance: *importance,
            }
        })
        .collect();

    let mut results = results;

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results.truncate(query.top_k * 3);

    Ok(results)
}

fn name_match_bonus(
    name: &Option<String>,
    signature: &Option<String>,
    query_tokens: &[String],
) -> f64 {
    let name_str = match name {
        Some(n) => n.to_lowercase(),
        None => return 0.0,
    };

    let sig_str = signature
        .as_ref()
        .map(|s| s.to_lowercase())
        .unwrap_or_default();

    if query_tokens.is_empty() {
        return 0.0;
    }

    let name_match_count: usize = query_tokens
        .iter()
        .filter(|t| name_str.contains(t.as_str()))
        .count();

    let sig_match_count: usize = query_tokens
        .iter()
        .filter(|t| sig_str.contains(t.as_str()) && !name_str.contains(t.as_str()))
        .count();

    let total_matching = name_match_count + sig_match_count;
    let name_ratio = name_match_count as f64 / query_tokens.len() as f64;
    let total_ratio = total_matching as f64 / query_tokens.len() as f64;

    let camel_bonus: f64 = query_tokens
        .iter()
        .filter(|t| name_str.len() > t.len() + 2 && name_str.contains(t.as_str()))
        .map(|_| 0.05)
        .sum();

    if name_ratio >= 0.8 {
        0.4
    } else if total_ratio >= 0.8 {
        0.3
    } else if name_ratio >= 0.5 {
        0.25
    } else if total_ratio >= 0.5 {
        0.15
    } else if name_match_count > 0 {
        0.12 + camel_bonus
    } else if total_matching > 0 {
        0.06
    } else {
        0.0
    }
}

#[allow(clippy::too_many_arguments)]
pub fn vec_search(
    db: &Connection,
    query_vec: &[f32],
    top_k: usize,
    file_filter: Option<&str>,
    type_filter: Option<&str>,
    source_type_filter: Option<&[String]>,
    agent_id_filter: Option<&str>,
    exclude_path_prefixes: &[String],
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    let vec_count: i64 = db
        .query_row("SELECT COUNT(*) FROM chunks_vec", [], |r| r.get(0))
        .unwrap_or(0);
    if vec_count == 0 {
        return Ok(Vec::new());
    }

    let vec_str = format_vector(query_vec);

    let mut extra_clauses = Vec::new();
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(vec_str)];
    let mut next_param: usize = 2;

    if let Some(f) = file_filter {
        extra_clauses.push(format!("AND c.file_path = ?{}", next_param));
        param_values.push(Box::new(f.to_string()));
        next_param += 1;
    }
    if let Some(t) = type_filter {
        extra_clauses.push(format!("AND c.chunk_type = ?{}", next_param));
        param_values.push(Box::new(t.to_string()));
        next_param += 1;
    }
    if let Some(sources) = source_type_filter {
        if !sources.is_empty() {
            let placeholders: Vec<String> = sources
                .iter()
                .enumerate()
                .map(|(i, _)| format!("?{}", next_param + i))
                .collect();
            extra_clauses.push(format!(
                "AND c.source_type IN ({})",
                placeholders.join(", ")
            ));
            for s in sources {
                param_values.push(Box::new(s.clone()));
            }
            next_param += sources.len();
        }
    }
    if let Some(agent) = agent_id_filter {
        extra_clauses.push(format!("AND c.agent_id = ?{}", next_param));
        param_values.push(Box::new(agent.to_string()));
    }
    for prefix in exclude_path_prefixes {
        extra_clauses.push(format!(
            "AND c.file_path NOT LIKE '{}%'",
            prefix.replace('\'', "''")
        ));
    }

    let filter_clause = extra_clauses.join(" ");

    let sql = format!(
        "SELECT v.rowid, v.distance, c.file_path, c.name, c.signature, c.line_start, c.line_end, c.chunk_type, c.source_type, c.decay_rate, c.last_accessed, c.importance, c.updated_at \
         FROM chunks_vec v JOIN chunks c ON v.rowid = c.id \
         WHERE v.embedding MATCH ?1 AND k = {} AND c.is_deleted = 0 {}",
        top_k, filter_clause
    );

    let mut stmt = db.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    type VecRow = (
        i64,
        f64,
        String,
        Option<String>,
        Option<String>,
        i64,
        i64,
        String,
        String,
        f64,
        Option<String>,
        f64,
        Option<String>,
    );
    let rows: Vec<VecRow> = stmt
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
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
            ))
        })?
        .collect::<Result<_, _>>()?;

    let now = chrono_now();
    let results: Vec<SearchResult> = rows
        .into_iter()
        .map(
            |(
                id,
                distance,
                file_path,
                name,
                sig,
                start,
                end,
                ct,
                source_type,
                decay_rate,
                last_accessed,
                importance,
                updated_at,
            )| {
                let relevance = 1.0 - distance.min(1.0);
                let recency = compute_recency(updated_at.as_deref(), &now);
                let score = three_factor_score(relevance, recency, importance);
                SearchResult {
                    chunk_id: id,
                    file_path,
                    name,
                    signature: sig,
                    line_start: start,
                    line_end: end,
                    chunk_type: ct,
                    source_type,
                    score,
                    vec_distance: Some(distance),
                    fts_rank: None,
                    snippet: None,
                    decay_rate,
                    last_accessed,
                    importance,
                }
            },
        )
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

    let (vec_results, hint_vec_pairs) = if embedder.is_available() {
        let query_vec = embedder.embed_query(&query.text)?;
        let vr = vec_search(
            db,
            &query_vec,
            query.top_k * 2,
            query.file_filter.as_deref(),
            query.type_filter.as_deref(),
            query.source_type_filter.as_deref(),
            query.agent_id_filter.as_deref(),
            &query.exclude_path_prefixes,
        )?;
        let hv = hint_vec_search(db, &query_vec, query.top_k * 2).unwrap_or_default();
        (vr, hv)
    } else {
        (Vec::new(), Vec::new())
    };

    let merged = merge_results(fts_results, vec_results, query.alpha);

    let merged = merge_hint_vec_results(db, merged, hint_vec_pairs)?;

    let filtered: Vec<SearchResult> = merged
        .into_iter()
        .filter(|r| r.score >= query.min_score)
        .take(query.top_k)
        .collect();

    let mut filtered = filtered;
    dampening::importance_boost(&mut filtered);
    dampening::dampen(&mut filtered, 0.8);

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

    let fts_scores: Vec<f64> = results.iter().filter_map(|r| r.fts_rank).collect();
    let fts_max = fts_scores.iter().cloned().fold(0.0_f64, f64::max);
    let fts_scale = if fts_max > 0.0 { 1.0 / fts_max } else { 1.0 };

    let vec_dists: Vec<f64> = results.iter().filter_map(|r| r.vec_distance).collect();
    let vec_min = vec_dists.iter().cloned().fold(1.0_f64, f64::min);
    let vec_max = vec_dists.iter().cloned().fold(0.0_f64, f64::max);
    let vec_range = vec_max - vec_min;
    let vec_scale = if vec_range > 0.0 {
        1.0 / vec_range
    } else {
        1.0
    };

    for r in &mut results {
        let fts_score = r.fts_rank.unwrap_or(0.0) * fts_scale;
        let raw_vec = r.vec_distance.unwrap_or(1.0);
        let vec_score = if vec_range > 0.0 {
            (raw_vec - vec_min) * vec_scale
        } else {
            1.0 - raw_vec
        };

        let has_fts = r.fts_rank.is_some();
        let has_vec = r.vec_distance.is_some();

        if has_fts && has_vec {
            r.score = alpha * (1.0 - vec_score) + (1.0 - alpha) * fts_score;
        } else if has_fts {
            r.score = fts_score * (1.0 - alpha + alpha * 0.3);
        } else if has_vec {
            r.score = (1.0 - vec_score) * (alpha + (1.0 - alpha) * 0.3);
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

#[cfg(feature = "embed")]
fn merge_hint_vec_results(
    db: &Connection,
    mut results: Vec<SearchResult>,
    hint_pairs: Vec<(i64, f64)>,
) -> Result<Vec<SearchResult>, Box<dyn std::error::Error>> {
    if hint_pairs.is_empty() {
        return Ok(results);
    }

    let existing_ids: std::collections::HashSet<i64> = results.iter().map(|r| r.chunk_id).collect();

    let hint_score_map: std::collections::HashMap<i64, f64> = hint_pairs.into_iter().collect();

    let max_existing_score = results.iter().map(|r| r.score).fold(0.0_f64, f64::max);

    for r in &mut results {
        if let Some(&dist) = hint_score_map.get(&r.chunk_id) {
            let hint_relevance = 1.0 - dist.min(1.0);
            let hint_boost = hint_relevance * 0.25 * max_existing_score.max(0.1);
            r.score += hint_boost;
        }
    }

    let missing: Vec<(i64, f64)> = hint_score_map
        .iter()
        .filter(|(id, _)| !existing_ids.contains(id))
        .map(|(&id, &s)| (id, s))
        .collect();

    if !missing.is_empty() {
        let placeholders: Vec<String> = (0..missing.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!(
            "SELECT id, file_path, name, signature, line_start, line_end, chunk_type, source_type, importance \
             FROM chunks WHERE id IN ({}) AND is_deleted = 0",
            placeholders.join(", ")
        );
        let mut stmt = db.prepare(&sql)?;
        let param_vals: Vec<Box<dyn rusqlite::ToSql>> = missing
            .iter()
            .map(|(id, _)| Box::new(*id) as Box<dyn rusqlite::ToSql>)
            .collect();
        let param_refs: Vec<&dyn rusqlite::ToSql> = param_vals.iter().map(|p| p.as_ref()).collect();

        type HVecRow = (
            i64,
            String,
            Option<String>,
            Option<String>,
            i64,
            i64,
            String,
            String,
            f64,
        );
        let rows: Vec<HVecRow> = stmt
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

        let row_map: std::collections::HashMap<i64, HVecRow> =
            rows.into_iter().map(|r| (r.0, r)).collect();

        for (id, dist) in &missing {
            if let Some((cid, file_path, name, sig, start, end, ct, st, importance)) =
                row_map.get(id)
            {
                let score = (1.0 - dist.min(1.0)) * 0.5 * max_existing_score.max(0.1);
                results.push(SearchResult {
                    chunk_id: *cid,
                    file_path: file_path.clone(),
                    name: name.clone(),
                    signature: sig.clone(),
                    line_start: *start,
                    line_end: *end,
                    chunk_type: ct.clone(),
                    source_type: st.clone(),
                    score,
                    vec_distance: Some(*dist),
                    fts_rank: None,
                    snippet: Some("[hint vec match]".to_string()),
                    decay_rate: 0.0,
                    last_accessed: None,
                    importance: *importance,
                });
            }
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(results)
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64
}

fn compute_recency(updated_at: Option<&str>, now: &i64) -> f64 {
    let updated_ts = updated_at
        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(*now);
    let days_since = ((*now - updated_ts).max(0) as f64) / 86400.0;
    let half_life = 90.0;
    (2.0_f64).powf(-days_since / half_life)
}

fn three_factor_score(relevance: f64, recency: f64, importance: f64) -> f64 {
    relevance * recency * importance
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

#[cfg(feature = "embed")]
pub fn store_hint_embedding(
    db: &Connection,
    chunk_id: i64,
    vector: &[f32],
) -> Result<(), Box<dyn std::error::Error>> {
    db.execute(
        "INSERT OR REPLACE INTO hints_vec(rowid, embedding) VALUES (?1, ?2)",
        params![chunk_id, format_vector(vector)],
    )?;
    Ok(())
}

#[cfg(feature = "embed")]
fn hint_vec_search(
    db: &Connection,
    query_vec: &[f32],
    top_k: usize,
) -> Result<Vec<(i64, f64)>, Box<dyn std::error::Error>> {
    let count: i64 = db
        .query_row("SELECT COUNT(*) FROM hints_vec", [], |r| r.get(0))
        .unwrap_or(0);
    if count == 0 {
        return Ok(Vec::new());
    }

    let vec_str = format_vector(query_vec);
    let sql = format!(
        "SELECT rowid, distance FROM hints_vec WHERE embedding MATCH ?1 AND k = {}",
        top_k
    );
    let mut stmt = db.prepare(&sql)?;
    let rows: Vec<(i64, f64)> = stmt
        .query_map(params![vec_str], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    Ok(rows)
}

pub fn get_unembedded_chunk_ids(
    db: &Connection,
    limit: usize,
) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT c.id FROM chunks c LEFT JOIN embeddings e ON c.id = e.chunk_id WHERE e.chunk_id IS NULL ORDER BY LENGTH(c.content_raw) ASC, c.importance DESC LIMIT ?1"
    )?;
    let ids: Vec<i64> = stmt
        .query_map(params![limit as i64], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

#[cfg(feature = "embed")]
const EMBED_MAX_CHARS: usize = 6000;

#[cfg(feature = "embed")]
pub fn embed_unembedded(
    db: &mut Connection,
    embedder: &mut Embedder,
) -> Result<usize, Box<dyn std::error::Error>> {
    let ids = get_unembedded_chunk_ids(db, 32)?;
    if ids.is_empty() {
        return Ok(0);
    }

    let placeholders: Vec<String> = ids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT id, COALESCE(name || ' ', '') || COALESCE(signature || ' ', '') || COALESCE(content_raw, '') FROM chunks WHERE id IN ({})",
        placeholders.join(", ")
    );
    let params: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let mut stmt = db.prepare(&sql)?;

    let texts: Vec<(i64, String)> = stmt
        .query_map(params.as_slice(), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    let truncated: Vec<(i64, String)> = texts
        .into_iter()
        .filter(|(_, text)| !text.trim().is_empty())
        .map(|(id, text)| {
            if text.len() > EMBED_MAX_CHARS {
                (id, text[..EMBED_MAX_CHARS].to_string())
            } else {
                (id, text)
            }
        })
        .collect();

    if truncated.is_empty() {
        db.execute_batch("COMMIT")?;
        return Ok(0);
    }

    let text_refs: Vec<&str> = truncated.iter().map(|(_, t)| t.as_str()).collect();
    let vectors = match embedder.embed_batch_documents(&text_refs) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[embed] batch failed: {e}");
            db.execute_batch("COMMIT")?;
            return Ok(0);
        }
    };

    db.execute_batch("BEGIN")?;
    for ((chunk_id, _), vector) in truncated.iter().zip(vectors.iter()) {
        store_embedding(db, *chunk_id, vector, embedder.model_name())?;
    }

    let hint_texts: Vec<(i64, String)> = truncated
        .iter()
        .filter_map(|(chunk_id, _)| {
            crate::entities::get_concatenated_hints_for_chunk(db, *chunk_id)
                .ok()
                .flatten()
                .map(|h| (*chunk_id, h))
        })
        .collect();

    if !hint_texts.is_empty() {
        let hint_refs: Vec<&str> = hint_texts.iter().map(|(_, t)| t.as_str()).collect();
        let hint_vectors = embedder.embed_batch_documents(&hint_refs)?;
        for ((chunk_id, _), vector) in hint_texts.iter().zip(hint_vectors.iter()) {
            store_hint_embedding(db, *chunk_id, vector)?;
        }
    }

    db.execute_batch("COMMIT")?;

    Ok(truncated.len())
}

pub fn render_search_markdown(
    db: &Connection,
    results: &[SearchResult],
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    if results.is_empty() {
        return Ok(Vec::new());
    }

    let ids: Vec<i64> = results.iter().map(|r| r.chunk_id).collect();
    let placeholders: Vec<String> = (0..ids.len()).map(|i| format!("?{}", i + 1)).collect();
    let ph = placeholders.join(", ");

    let sql = format!(
        "SELECT id, content_raw, language, source_type, importance, tags FROM chunks WHERE id IN ({ph})"
    );
    let mut stmt = db.prepare(&sql)?;
    let rows: Vec<(i64, String, String, String, f64, Option<String>)> = stmt
        .query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    drop(stmt);

    let row_map: std::collections::HashMap<i64, _> = rows.into_iter().map(|r| (r.0, r)).collect();

    let rendered: Vec<String> = results
        .iter()
        .map(|r| {
            if let Some((_, content_raw, language, source_type, importance, tags_json)) =
                row_map.get(&r.chunk_id)
            {
                let tags: Option<Vec<String>> = tags_json
                    .as_ref()
                    .and_then(|t| serde_json::from_str(t).ok());
                let chunk = crate::chunk::Chunk {
                    file_path: r.file_path.clone(),
                    language: language.clone(),
                    chunk_type: crate::chunk::ChunkType::from_str_name(&r.chunk_type)
                        .unwrap_or(crate::chunk::ChunkType::Fact),
                    name: r.name.clone(),
                    signature: r.signature.clone(),
                    line_start: r.line_start as usize,
                    line_end: r.line_end as usize,
                    content_raw: content_raw.clone(),
                    content_hash: String::new(),
                    importance: *importance,
                    source_type: crate::chunk::SourceType::from_str_name(source_type)
                        .unwrap_or(crate::chunk::SourceType::Code),
                    metadata: serde_json::Map::new(),
                    agent_id: None,
                    tags,
                    decay_rate: r.decay_rate,
                    created_by: None,
                };
                chunk.render_md()
            } else {
                String::new()
            }
        })
        .collect();

    Ok(rendered)
}

#[cfg(feature = "ollama")]
pub fn generate_ollama_hints_batch(
    db: &mut Connection,
    limit: usize,
    min_importance: f64,
) -> Result<usize, Box<dyn std::error::Error>> {
    let client = crate::ollama::OllamaClient::new();

    let ids: Vec<i64> = {
        let mut stmt = db.prepare(
            "SELECT c.id FROM chunks c
             WHERE c.is_deleted = 0 AND c.importance >= ?1
             AND c.id NOT IN (SELECT DISTINCT chunk_id FROM hints WHERE hint_type = 'prospective')
             ORDER BY c.importance DESC LIMIT ?2",
        )?;
        let rows = stmt
            .query_map(params![min_importance, limit as i64], |r| r.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    if ids.is_empty() {
        return Ok(0);
    }

    let mut processed = 0usize;
    for chunk_id in &ids {
        let content: String = db.query_row(
            "SELECT COALESCE(name || ' ', '') || COALESCE(signature || ' ', '') || COALESCE(content_raw, '') FROM chunks WHERE id = ?1",
            params![chunk_id],
            |r| r.get(0),
        )?;

        let truncated = if content.len() > 3000 {
            &content[..3000]
        } else {
            &content
        };

        match client.generate_prospective_hints(truncated) {
            Ok(hints) if !hints.is_empty() => {
                let typed: Vec<(String, &str)> =
                    hints.into_iter().map(|h| (h, "prospective")).collect();
                if let Err(e) = crate::entities::insert_hints_typed(db, *chunk_id, &typed) {
                    eprintln!("[hints] failed to insert for chunk {}: {e}", chunk_id);
                }
                processed += 1;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("[hints] ollama failed for chunk {}: {e}", chunk_id);
            }
        }
    }

    Ok(processed)
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
        let results = vec_search(&db, &query_vec, 5, None, None, None, None, &[]).unwrap();
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
            .query_row(
                "SELECT vector FROM embeddings WHERE chunk_id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let restored = embed::blob_to_vector(&blob);
        assert_eq!(restored.len(), 3);
        assert!((restored[0] - 1.0).abs() < f32::EPSILON);
    }
}
