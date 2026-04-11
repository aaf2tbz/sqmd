use rusqlite::params;

type DepFactAt = (i64, String, f64, i64, String, Option<String>);
type CurrentDep = (i64, String, f64, i64);
type FactHistoryEntry = (i64, String, Option<String>, i64);

#[derive(Debug, Clone, serde::Serialize)]
pub struct Entity {
    pub id: i64,
    pub name: String,
    pub canonical_name: String,
    pub entity_type: String,
    pub mentions: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_start: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_end: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunk_id: Option<i64>,
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

pub fn ensure_entity(
    db: &rusqlite::Connection,
    name: &str,
    entity_type: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    let canon = canonicalize(name);
    let existing: Option<i64> = db
        .query_row(
            "SELECT id FROM entities WHERE canonical_name = ?1",
            params![canon],
            |r| r.get(0),
        )
        .ok();

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

pub struct SymbolEntityInput<'a> {
    pub name: &'a str,
    pub entity_type: &'a str,
    pub file_path: &'a str,
    pub language: &'a str,
    pub line_start: i64,
    pub line_end: i64,
    pub signature: Option<&'a str>,
    pub chunk_id: Option<i64>,
}

pub fn ensure_symbol_entity(
    db: &rusqlite::Connection,
    input: &SymbolEntityInput,
) -> Result<i64, Box<dyn std::error::Error>> {
    let canon = canonicalize(input.name);
    let existing: Option<i64> = db
        .query_row(
            "SELECT id FROM entities WHERE canonical_name = ?1",
            params![canon],
            |r| r.get(0),
        )
        .ok();

    if let Some(id) = existing {
        db.execute(
            "UPDATE entities SET mentions = mentions + 1, entity_type = ?1, file_path = COALESCE(?2, file_path), language = COALESCE(?3, language), line_start = COALESCE(?4, line_start), line_end = COALESCE(?5, line_end), signature = COALESCE(?6, signature), chunk_id = COALESCE(?7, chunk_id), updated_at = datetime('now') WHERE id = ?8",
            params![input.entity_type, input.file_path, input.language, input.line_start, input.line_end, input.signature, input.chunk_id, id],
        )?;
        Ok(id)
    } else {
        db.execute(
            "INSERT INTO entities (name, canonical_name, entity_type, file_path, language, line_start, line_end, signature, chunk_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, datetime('now'), datetime('now'))",
            params![input.name, canon, input.entity_type, input.file_path, input.language, input.line_start, input.line_end, input.signature, input.chunk_id],
        )?;
        Ok(db.last_insert_rowid())
    }
}

pub fn ensure_aspect(
    db: &rusqlite::Connection,
    entity_id: i64,
    aspect_name: &str,
) -> Result<i64, Box<dyn std::error::Error>> {
    let canon = canonicalize(aspect_name);
    let existing: Option<i64> = db
        .query_row(
            "SELECT id FROM entity_aspects WHERE entity_id = ?1 AND canonical_name = ?2",
            params![entity_id, canon],
            |r| r.get(0),
        )
        .ok();

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
        "INSERT INTO entity_dependencies (source_entity, target_entity, dep_type, valid_from) VALUES (?1, ?2, ?3, datetime('now'))
         ON CONFLICT(source_entity, target_entity, dep_type) DO UPDATE SET mentions = mentions + 1",
        params![source_entity, target_entity, dep_type],
    )?;
    Ok(())
}

pub fn supersede_dependency(
    db: &rusqlite::Connection,
    source_entity: i64,
    target_entity: i64,
    dep_type: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let count = db.execute(
        "UPDATE entity_dependencies SET valid_to = datetime('now') WHERE source_entity = ?1 AND target_entity = ?2 AND dep_type = ?3 AND valid_to IS NULL",
        params![source_entity, target_entity, dep_type],
    )?;
    Ok(count)
}

pub fn query_dependencies_at(
    db: &rusqlite::Connection,
    entity_id: i64,
    as_of: &str,
) -> Result<Vec<DepFactAt>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT source_entity, dep_type, strength, mentions, valid_from, valid_to
         FROM entity_dependencies
         WHERE (source_entity = ?1 OR target_entity = ?1)
           AND datetime(?2) >= valid_from
           AND (valid_to IS NULL OR datetime(?2) < valid_to)",
    )?;
    let rows = stmt
        .query_map(params![entity_id, as_of], |r| {
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

pub fn get_current_dependencies(
    db: &rusqlite::Connection,
    entity_id: i64,
) -> Result<Vec<CurrentDep>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT target_entity, dep_type, strength, mentions
         FROM entity_dependencies
         WHERE source_entity = ?1 AND valid_to IS NULL",
    )?;
    let rows = stmt
        .query_map(params![entity_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_fact_history(
    db: &rusqlite::Connection,
    source_entity: i64,
    target_entity: i64,
    dep_type: &str,
) -> Result<Vec<FactHistoryEntry>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT id, valid_from, valid_to, mentions FROM entity_dependencies
         WHERE source_entity = ?1 AND target_entity = ?2 AND dep_type = ?3
         ORDER BY valid_from DESC",
    )?;
    let rows = stmt
        .query_map(params![source_entity, target_entity, dep_type], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_entity(
    db: &rusqlite::Connection,
    name: &str,
) -> Result<Option<Entity>, Box<dyn std::error::Error>> {
    let canon = canonicalize(name);
    let result = db.query_row(
        "SELECT id, name, canonical_name, entity_type, mentions, file_path, language, line_start, line_end, signature, chunk_id FROM entities WHERE canonical_name = ?1",
        params![canon],
        |r| Ok(Entity {
            id: r.get(0)?,
            name: r.get(1)?,
            canonical_name: r.get(2)?,
            entity_type: r.get(3)?,
            mentions: r.get(4)?,
            file_path: r.get(5)?,
            language: r.get(6)?,
            line_start: r.get(7)?,
            line_end: r.get(8)?,
            signature: r.get(9)?,
            chunk_id: r.get(10)?,
        }),
    ).ok();
    Ok(result)
}

pub fn get_aspects(
    db: &rusqlite::Connection,
    entity_id: i64,
) -> Result<Vec<Aspect>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT id, entity_id, name, weight FROM entity_aspects WHERE entity_id = ?1 ORDER BY weight DESC"
    )?;
    let rows = stmt
        .query_map(params![entity_id], |r| {
            Ok(Aspect {
                id: r.get(0)?,
                entity_id: r.get(1)?,
                name: r.get(2)?,
                weight: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
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
            SELECT target_entity, 1 FROM entity_dependencies WHERE source_entity = ?1 AND valid_to IS NULL
            UNION
            SELECT ed.target_entity, ed2.d + 1 FROM entity_dependencies ed
            JOIN ent_deps ed2 ON ed.source_entity = ed2.target_entity
            WHERE ed2.d < {0} AND ed.valid_to IS NULL
        )
        SELECT DISTINCT target_entity FROM ent_deps WHERE target_entity != ?1",
        depth,
    );
    let mut stmt = db.prepare(&sql)?;
    let ids: Vec<i64> = stmt
        .query_map(params![entity_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

/// Returns (chunk_id, boost_score) pairs with distance-decayed boost.
/// base_boost=0.20, decay_per_hop=0.5: hop 1=0.20, hop 2=0.10, hop 3=0.05
pub fn graph_boost_scored(
    db: &rusqlite::Connection,
    query: &str,
    max_ids: usize,
) -> Result<Vec<(i64, f64)>, Box<dyn std::error::Error>> {
    let base_boost: f64 = 0.20;
    let decay_per_hop: f64 = 0.5;

    let tokens: Vec<String> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(|t| t.to_lowercase())
        .collect();

    if tokens.is_empty() {
        return Ok(Vec::new());
    }

    // Find matching entities
    let mut entity_ids: Vec<i64> = Vec::new();
    for token in &tokens {
        let mut stmt = db.prepare(
            "SELECT id FROM entities WHERE canonical_name LIKE ?1 ORDER BY mentions DESC LIMIT 5",
        )?;
        let pattern = format!("%{}%", token);
        let ids: Vec<i64> = stmt
            .query_map(params![pattern], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        entity_ids.extend(ids);
    }
    entity_ids.sort();
    entity_ids.dedup();
    entity_ids.truncate(20);

    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Multi-hop expansion with distance tracking
    let mut entity_distances: std::collections::HashMap<i64, usize> =
        std::collections::HashMap::new();
    for &eid in &entity_ids {
        entity_distances.insert(eid, 0);
    }

    // 3-hop expansion via recursive CTE with depth
    for &seed in &entity_ids {
        let sql = "WITH RECURSIVE deps(eid, depth) AS (
            SELECT target_entity, 1 FROM entity_dependencies WHERE source_entity = ?1 AND valid_to IS NULL
            UNION
            SELECT source_entity, 1 FROM entity_dependencies WHERE target_entity = ?1 AND valid_to IS NULL
            UNION ALL
            SELECT ed.target_entity, d.depth + 1 FROM entity_dependencies ed
            JOIN deps d ON ed.source_entity = d.eid WHERE d.depth < 3 AND ed.valid_to IS NULL
            UNION ALL
            SELECT ed.source_entity, d.depth + 1 FROM entity_dependencies ed
            JOIN deps d ON ed.target_entity = d.eid WHERE d.depth < 3 AND ed.valid_to IS NULL
        ) SELECT DISTINCT eid, MIN(depth) FROM deps GROUP BY eid";

        let mut stmt = db.prepare(sql)?;
        let rows: Vec<(i64, usize)> = stmt
            .query_map(params![seed], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, usize>(1)?))
            })?
            .collect::<Result<_, _>>()?;

        for (eid, depth) in rows {
            let existing = entity_distances.entry(eid).or_insert(depth);
            if depth < *existing {
                *existing = depth;
            }
        }
    }

    if entity_distances.is_empty() {
        return Ok(Vec::new());
    }

    // Get chunk_ids for all expanded entities, with their minimum hop distance
    let all_eids: Vec<i64> = entity_distances.keys().copied().collect();
    let placeholders: Vec<String> = all_eids
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT DISTINCT chunk_id, id FROM entities WHERE id IN ({}) AND chunk_id IS NOT NULL",
        placeholders.join(", "),
    );
    let mut stmt = db.prepare(&sql)?;
    let param_values: Vec<Box<dyn rusqlite::ToSql>> = all_eids
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
        .collect();
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let rows: Vec<(i64, i64)> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
        })?
        .collect::<Result<_, _>>()?;

    // For each chunk, use the minimum hop distance of its entity
    let mut chunk_scores: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    for (chunk_id, entity_id) in rows {
        let hops = entity_distances.get(&entity_id).copied().unwrap_or(3);
        let boost = base_boost * decay_per_hop.powi(hops as i32);
        let entry = chunk_scores.entry(chunk_id).or_insert(0.0);
        if boost > *entry {
            *entry = boost;
        }
    }

    let mut result: Vec<(i64, f64)> = chunk_scores.into_iter().collect();
    result.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    result.truncate(max_ids);
    Ok(result)
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
    if !tokens.is_empty() {
        let like_clauses: Vec<String> = tokens
            .iter()
            .enumerate()
            .map(|(i, _)| format!("canonical_name LIKE ?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT id FROM entities WHERE {} ORDER BY mentions DESC LIMIT 100",
            like_clauses.join(" OR ")
        );
        let mut stmt = db.prepare(&sql)?;
        let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        for token in &tokens {
            param_values.push(Box::new(format!("%{}%", token)));
        }
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let ids: Vec<i64> = stmt
            .query_map(param_refs.as_slice(), |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        entity_ids = ids;
    }

    entity_ids.sort();
    entity_ids.dedup();
    entity_ids.truncate(20);

    if entity_ids.is_empty() {
        return Ok(Vec::new());
    }

    let expanded: Vec<i64> = if !entity_ids.is_empty() {
        let placeholders: Vec<String> = entity_ids
            .iter()
            .enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect();
        let sql = format!(
            "SELECT DISTINCT target_entity FROM entity_dependencies WHERE source_entity IN ({})
             UNION
             SELECT DISTINCT source_entity FROM entity_dependencies WHERE target_entity IN ({})",
            placeholders.join(", "),
            placeholders.join(", "),
        );
        let mut stmt = db.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::ToSql> = entity_ids
            .iter()
            .map(|id| id as &dyn rusqlite::ToSql)
            .collect();
        let ids: Vec<i64> = stmt
            .query_map(params.as_slice(), |r| r.get(0))?
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

    let placeholders: Vec<String> = expanded
        .iter()
        .enumerate()
        .map(|(i, _)| format!("?{}", i + 1))
        .collect();
    let sql = format!(
        "SELECT DISTINCT chunk_id FROM entity_attributes WHERE entity_id IN ({}) AND chunk_id IS NOT NULL LIMIT ?{}",
        placeholders.join(", "),
        placeholders.len() + 1,
    );

    let mut stmt = db.prepare(&sql)?;
    let mut param_values: Vec<Box<dyn rusqlite::ToSql>> = expanded
        .iter()
        .map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>)
        .collect();
    param_values.push(Box::new(max_ids as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();

    let chunk_ids = stmt
        .query_map(param_refs.as_slice(), |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    Ok(chunk_ids)
}

pub fn list_entities(
    db: &rusqlite::Connection,
    entity_type_filter: Option<&str>,
    limit: usize,
) -> Result<Vec<Entity>, Box<dyn std::error::Error>> {
    let sql = match entity_type_filter {
        Some(_) => format!("SELECT id, name, canonical_name, entity_type, mentions, file_path, language, line_start, line_end, signature, chunk_id FROM entities WHERE entity_type = ?1 ORDER BY mentions DESC LIMIT {}", limit),
        None => format!("SELECT id, name, canonical_name, entity_type, mentions, file_path, language, line_start, line_end, signature, chunk_id FROM entities ORDER BY mentions DESC LIMIT {}", limit),
    };
    let mut stmt = db.prepare(&sql)?;
    let rows: Vec<Entity> = if let Some(et) = entity_type_filter {
        stmt.query_map(params![et], |r| {
            Ok(Entity {
                id: r.get(0)?,
                name: r.get(1)?,
                canonical_name: r.get(2)?,
                entity_type: r.get(3)?,
                mentions: r.get(4)?,
                file_path: r.get(5)?,
                language: r.get(6)?,
                line_start: r.get(7)?,
                line_end: r.get(8)?,
                signature: r.get(9)?,
                chunk_id: r.get(10)?,
            })
        })?
        .collect::<Result<_, _>>()?
    } else {
        stmt.query_map([], |r| {
            Ok(Entity {
                id: r.get(0)?,
                name: r.get(1)?,
                canonical_name: r.get(2)?,
                entity_type: r.get(3)?,
                mentions: r.get(4)?,
                file_path: r.get(5)?,
                language: r.get(6)?,
                line_start: r.get(7)?,
                line_end: r.get(8)?,
                signature: r.get(9)?,
                chunk_id: r.get(10)?,
            })
        })?
        .collect::<Result<_, _>>()?
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

    let contains_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM relationships WHERE source_id = ?1 AND rel_type = 'contains'",
            params![chunk_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let constraint_count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM entity_attributes ea
         JOIN relationships r ON ea.chunk_id = r.target_id
         WHERE r.target_id = ?1 AND ea.kind = 'constraint'",
            params![chunk_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let in_factor = 0.3 * (1.0 + in_degree as f64).log10().max(0.0);
    let contains_factor = 0.2 * (1.0 + contains_count as f64).log10().max(0.0);
    let constraint_factor = if constraint_count > 0 { 0.2 } else { 0.0 };
    let base_factor = 0.3;

    let computed =
        base_importance * (in_factor + contains_factor + constraint_factor + base_factor);
    Ok(computed.clamp(0.1, 1.0))
}

pub fn generate_hints(
    chunk_name: Option<&str>,
    chunk_type: &str,
    content: &str,
    file_path: &str,
    source_type: &str,
) -> Vec<String> {
    match source_type {
        "memory" | "transcript" => generate_memory_hints(content),
        _ => generate_code_hints(chunk_name, chunk_type, content, file_path),
    }
}

fn generate_code_hints(
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

fn generate_memory_hints(content: &str) -> Vec<String> {
    let mut hints = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Extract capitalized names (likely person/place names)
    let names: Vec<&str> = content
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|w| {
            w.len() >= 2
                && w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                && !matches!(
                    *w,
                    "The"
                        | "This"
                        | "That"
                        | "What"
                        | "When"
                        | "Where"
                        | "How"
                        | "Why"
                        | "Yes"
                        | "No"
                        | "Hey"
                        | "Hi"
                        | "Oh"
                        | "Well"
                        | "And"
                        | "But"
                        | "So"
                        | "Session"
                        | "Speaker"
                        | "Conversation"
                )
        })
        .collect();

    // Unique names
    let mut unique_names: Vec<&str> = Vec::new();
    for name in &names {
        let lower = name.to_lowercase();
        if seen.insert(lower) {
            unique_names.push(name);
        }
    }

    // Generate multi-word person+topic pairs — never single names alone (too noisy)
    if unique_names.len() >= 2 {
        hints.push(format!("{} {}", unique_names[0], unique_names[1]));
    }
    if unique_names.len() >= 3 {
        hints.push(format!("{} {}", unique_names[0], unique_names[2]));
    }

    // Extract quoted strings as hints
    for cap in content.match_indices('"') {
        let start = cap.0 + 1;
        if let Some(end) = content[start..].find('"') {
            let quoted = &content[start..start + end];
            if quoted.len() >= 3 && quoted.len() <= 50 {
                hints.push(quoted.to_string());
            }
        }
    }

    // Extract date-like patterns
    for word in content.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| !c.is_alphanumeric());
        // Match patterns like "2024", "March", "January", month names
        if matches!(
            trimmed.to_lowercase().as_str(),
            "january"
                | "february"
                | "march"
                | "april"
                | "may"
                | "june"
                | "july"
                | "august"
                | "september"
                | "october"
                | "november"
                | "december"
                | "monday"
                | "tuesday"
                | "wednesday"
                | "thursday"
                | "friday"
                | "saturday"
                | "sunday"
        ) {
            hints.push(trimmed.to_string());
        }
    }

    // Key noun phrases: look for "about X", "named X", "called X" patterns
    for pattern in &["about ", "named ", "called ", "from ", "in "] {
        for (idx, _) in content.to_lowercase().match_indices(pattern) {
            let after = &content[idx + pattern.len()..];
            let phrase: String = after
                .split(['.', ',', '!', '?', '\n'])
                .next()
                .unwrap_or("")
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            if phrase.len() >= 3 {
                hints.push(phrase);
            }
        }
    }

    // Deduplicate, filter short/noisy hints, and limit
    let mut final_hints = Vec::new();
    let mut hint_set = std::collections::HashSet::new();
    for h in hints {
        // Skip single-word hints — they're too noisy for memory content
        if !h.contains(' ') && h.len() < 8 {
            continue;
        }
        let key = h.to_lowercase();
        if hint_set.insert(key) {
            final_hints.push(h);
        }
    }

    final_hints.truncate(8);
    final_hints
}

pub fn insert_hints(
    db: &rusqlite::Connection,
    chunk_id: i64,
    hints: &[String],
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut stmt = db.prepare("INSERT INTO hints (chunk_id, hint_text) VALUES (?1, ?2)")?;
    for hint in hints {
        stmt.execute(params![chunk_id, hint])?;
        count += 1;
    }
    Ok(count)
}

pub fn insert_hints_typed(
    db: &rusqlite::Connection,
    chunk_id: i64,
    hints: &[(String, &str)],
) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut stmt = db.prepare(
        "INSERT OR IGNORE INTO hints (chunk_id, hint_text, hint_type) VALUES (?1, ?2, ?3)",
    )?;
    for (hint, hint_type) in hints {
        stmt.execute(params![chunk_id, hint, hint_type])?;
        count += 1;
    }
    Ok(count)
}

pub fn generate_relational_hints(
    db: &rusqlite::Connection,
) -> Result<usize, Box<dyn std::error::Error>> {
    let entities: Vec<(i64, String)> = {
        let mut stmt = db.prepare(
            "SELECT id, name FROM entities WHERE chunk_id IS NOT NULL AND entity_type IN ('function', 'method', 'class', 'struct', 'interface', 'trait', 'enum')",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    let mut total = 0;
    for (entity_id, name) in &entities {
        let mut hints: Vec<(String, &str)> = Vec::new();

        let targets: Vec<(String, String)> = {
            let mut stmt = db.prepare(
                "SELECT e2.name, ed.dep_type FROM entity_dependencies ed
                 JOIN entities e2 ON ed.target_entity = e2.id
                 WHERE ed.source_entity = ?1 AND ed.valid_to IS NULL",
            )?;
            let rows = stmt
                .query_map(params![entity_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        let sources: Vec<(String, String)> = {
            let mut stmt = db.prepare(
                "SELECT e2.name, ed.dep_type FROM entity_dependencies ed
                 JOIN entities e2 ON ed.source_entity = e2.id
                 WHERE ed.target_entity = ?1 AND ed.valid_to IS NULL",
            )?;
            let rows = stmt
                .query_map(params![entity_id], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };

        for (target_name, dep_type) in &targets {
            match dep_type.as_str() {
                "implements" => {
                    hints.push((format!("{} implements {}", name, target_name), "heir"));
                }
                "extends" => {
                    hints.push((format!("{} extends {}", name, target_name), "heir"));
                }
                "contains" => {
                    hints.push((format!("{} contains {}", name, target_name), "member"));
                }
                "calls" => {
                    hints.push((format!("{} calls {}", name, target_name), "callee"));
                }
                "imports" => {
                    hints.push((format!("{} imports {}", name, target_name), "importer"));
                }
                _ => {}
            }
        }

        for (_source_name, dep_type) in &sources {
            match dep_type.as_str() {
                "implements" => {
                    hints.push((format!("implementations of {}", name), "heir"));
                }
                "extends" => {
                    hints.push((format!("subclasses of {}", name), "heir"));
                }
                "contains" => {
                    hints.push((format!("{} members", name), "member"));
                }
                "calls" => {
                    hints.push((format!("callers of {}", name), "caller"));
                }
                "imports" => {
                    hints.push((format!("files that import {}", name), "importer"));
                }
                _ => {}
            }
        }

        if hints.is_empty() {
            continue;
        }

        hints.truncate(6);
        hints.dedup_by(|a, b| a.0 == b.0);

        let chunk_id: Option<i64> = db
            .query_row(
                "SELECT chunk_id FROM entities WHERE id = ?1",
                params![entity_id],
                |r| r.get(0),
            )
            .ok();

        if let Some(cid) = chunk_id {
            total += insert_hints_typed(db, cid, &hints)?;
        }
    }

    Ok(total)
}

pub fn search_hints(
    db: &rusqlite::Connection,
    query: &str,
    top_k: usize,
) -> Result<Vec<(i64, f64)>, Box<dyn std::error::Error>> {
    let sql =
        "SELECT h.chunk_id, f.rank, h.hint_type FROM hints_fts f JOIN hints h ON f.rowid = h.id
               JOIN chunks c ON h.chunk_id = c.id WHERE c.is_deleted = 0
               AND hints_fts MATCH ?1 ORDER BY f.rank LIMIT ?2";
    let mut stmt = db.prepare(sql)?;
    let rows: Vec<(i64, f64, String)> = stmt
        .query_map(params![query, top_k as i64], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get::<_, String>(2)?))
        })?
        .collect::<Result<_, _>>()?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let min_rank = rows.iter().map(|r| r.1).fold(f64::INFINITY, f64::min);
    let max_rank = rows.iter().map(|r| r.1).fold(f64::NEG_INFINITY, f64::max);
    let range = (max_rank - min_rank).abs().max(0.001);

    let scored: Vec<(i64, f64)> = rows
        .into_iter()
        .map(|(id, rank, hint_type)| {
            let base = 1.0 - ((rank - min_rank) / range);
            let boost = match hint_type.as_str() {
                "caller" | "callee" | "heir" | "member" | "importer" => 0.15,
                _ => 0.0,
            };
            (id, (base + boost).min(1.0))
        })
        .collect();

    Ok(scored)
}

pub fn get_concatenated_hints_for_chunk(
    db: &rusqlite::Connection,
    chunk_id: i64,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare("SELECT hint_text FROM hints WHERE chunk_id = ?1")?;
    let hints: Vec<String> = stmt
        .query_map(params![chunk_id], |r| r.get::<_, String>(0))?
        .collect::<Result<_, _>>()?;
    if hints.is_empty() {
        Ok(None)
    } else {
        Ok(Some(hints.join(" ")))
    }
}

pub fn tombstone_chunks(
    db: &rusqlite::Connection,
    file_path: &str,
) -> Result<usize, Box<dyn std::error::Error>> {
    let count: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE file_path = ?1 AND is_deleted = 0",
            params![file_path],
            |r| r.get(0),
        )
        .unwrap_or(0);

    if count == 0 {
        return Ok(0);
    }

    db.execute(
        "UPDATE chunks SET is_deleted = 1, deleted_at = datetime('now') WHERE file_path = ?1 AND is_deleted = 0",
        params![file_path],
    )?;
    Ok(count as usize)
}

pub fn purge_tombstones(
    db: &rusqlite::Connection,
    max_age_days: i64,
) -> Result<usize, Box<dyn std::error::Error>> {
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
            "code",
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
            "code",
        );
        assert!(hints
            .iter()
            .any(|h| h.contains("DatabasePool type definition")));
        assert!(hints.iter().any(|h| h.contains("DatabasePool methods")));
    }

    #[test]
    fn test_generate_hints_no_name() {
        let hints = generate_hints(
            None,
            "section",
            "use std::collections;",
            "src/lib.rs",
            "code",
        );
        assert!(!hints.is_empty());
    }

    #[test]
    fn test_generate_memory_hints() {
        let content = "Caroline: I just adopted a golden retriever puppy named Max from the shelter last Saturday.\nMelanie: That's so exciting! How old is Max?";
        let hints = generate_hints(None, "Fact", content, "", "memory");
        assert!(hints.iter().any(|h| h.contains("Caroline")));
        assert!(hints.iter().any(|h| h.contains("Max")));
        assert!(hints.len() <= 8);
    }

    #[test]
    fn test_generate_memory_hints_with_dates() {
        let content = "[Session 5, March 2024] Tim: I'm running the marathon on Saturday.";
        let hints = generate_hints(None, "Fact", content, "", "memory");
        assert!(hints.iter().any(|h| h.to_lowercase().contains("march")
            || h.contains("Saturday")
            || h.contains("Tim")));
    }

    #[test]
    fn test_compute_structural_importance() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('a.ts', 'ts', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'a.ts', 'ts', 'function', 'helper', 0, 1, 'fn helper()', 'x')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (2, 'a.ts', 'ts', 'function', 'caller', 2, 3, 'fn caller() { helper(); }', 'y')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (3, 'a.ts', 'ts', 'function', 'caller2', 4, 5, 'fn caller2() { helper(); }', 'z')", []).unwrap();
        db.execute(
            "INSERT INTO relationships (source_id, target_id, rel_type) VALUES (2, 1, 'calls')",
            [],
        )
        .unwrap();
        db.execute(
            "INSERT INTO relationships (source_id, target_id, rel_type) VALUES (3, 1, 'calls')",
            [],
        )
        .unwrap();

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

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_path = 'old.rs' AND is_deleted = 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);

        let purged = purge_tombstones(&db, 0).unwrap();
        assert_eq!(purged, 1);

        let count: i64 = db
            .query_row(
                "SELECT COUNT(*) FROM chunks WHERE file_path = 'old.rs'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_insert_and_search_hints() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('auth.ts', 'ts', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'auth.ts', 'ts', 'function', 'authenticate', 0, 5, 'fn auth()', 'x')", []).unwrap();

        let hints = vec![
            "how does authentication work".to_string(),
            "auth function".to_string(),
        ];
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

    #[test]
    fn test_supersede_dependency() {
        let db = make_db();
        let eid = ensure_entity(&db, "AuthService", "file").unwrap();
        let target = ensure_entity(&db, "OldDatabase", "file").unwrap();

        ensure_dependency(&db, eid, target, "imports").unwrap();

        let current = get_current_dependencies(&db, eid).unwrap();
        assert_eq!(current.len(), 1);

        let count = supersede_dependency(&db, eid, target, "imports").unwrap();
        assert_eq!(count, 1);

        let current = get_current_dependencies(&db, eid).unwrap();
        assert!(
            current.is_empty(),
            "superseded dep should not appear in current"
        );

        let history = get_fact_history(&db, eid, target, "imports").unwrap();
        assert_eq!(history.len(), 1);
        assert!(
            history[0].2.is_some(),
            "valid_to should be set after supersede"
        );
    }

    #[test]
    fn test_query_dependencies_at() {
        let db = make_db();
        let eid = ensure_entity(&db, "AuthService", "file").unwrap();
        let target = ensure_entity(&db, "OldDB", "file").unwrap();

        ensure_dependency(&db, eid, target, "imports").unwrap();

        let supersede_count = supersede_dependency(&db, eid, target, "imports").unwrap();
        assert_eq!(supersede_count, 1);

        let current = get_current_dependencies(&db, eid).unwrap();
        assert!(
            current.is_empty(),
            "superseded dep should not appear in current"
        );

        let future_facts = query_dependencies_at(&db, eid, "2100-01-01").unwrap();
        assert!(
            future_facts.is_empty(),
            "future point should see superseded fact as inactive"
        );

        let recent_facts = query_dependencies_at(&db, eid, "2020-01-01").unwrap();
        assert!(recent_facts.is_empty(), "before creation should be empty");

        let now_facts = query_dependencies_at(&db, eid, "now").unwrap();
        assert!(
            now_facts.is_empty(),
            "now should see superseded fact as inactive"
        );
    }

    #[test]
    fn test_ensure_symbol_entity_creates_with_metadata() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('lib.rs', 'rust', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'lib.rs', 'rust', 'function', 'my_func', 5, 10, 'fn my_func() {}', 'x')", []).unwrap();

        let input = SymbolEntityInput {
            name: "my_func",
            entity_type: "function",
            file_path: "lib.rs",
            language: "rust",
            line_start: 5,
            line_end: 10,
            signature: Some("fn my_func()"),
            chunk_id: Some(1),
        };
        let id = ensure_symbol_entity(&db, &input).unwrap();

        let entity = get_entity(&db, "my_func").unwrap().unwrap();
        assert_eq!(entity.id, id);
        assert_eq!(entity.entity_type, "function");
        assert_eq!(entity.file_path.as_deref(), Some("lib.rs"));
        assert_eq!(entity.language.as_deref(), Some("rust"));
        assert_eq!(entity.line_start, Some(5));
        assert_eq!(entity.line_end, Some(10));
        assert_eq!(entity.signature.as_deref(), Some("fn my_func()"));
        assert_eq!(entity.chunk_id, Some(1));
    }

    #[test]
    fn test_ensure_symbol_entity_dedup_and_update() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('lib.rs', 'rust', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'lib.rs', 'rust', 'function', 'my_func', 5, 10, 'fn my_func() {}', 'x')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (2, 'lib.rs', 'rust', 'function', 'my_func', 15, 20, 'fn my_func() { new }', 'y')", []).unwrap();

        let input1 = SymbolEntityInput {
            name: "my_func",
            entity_type: "function",
            file_path: "lib.rs",
            language: "rust",
            line_start: 5,
            line_end: 10,
            signature: Some("fn my_func()"),
            chunk_id: Some(1),
        };
        let id1 = ensure_symbol_entity(&db, &input1).unwrap();

        let input2 = SymbolEntityInput {
            name: "my_func",
            entity_type: "function",
            file_path: "lib.rs",
            language: "rust",
            line_start: 15,
            line_end: 20,
            signature: Some("fn my_func() { new }"),
            chunk_id: Some(2),
        };
        let id2 = ensure_symbol_entity(&db, &input2).unwrap();
        assert_eq!(id1, id2, "same canonical name should return same entity id");

        let entity = get_entity(&db, "my_func").unwrap().unwrap();
        assert_eq!(entity.mentions, 2);
        assert_eq!(
            entity.line_end,
            Some(20),
            "should update to latest metadata"
        );
        assert_eq!(entity.chunk_id, Some(2), "should update chunk_id");
    }

    #[test]
    fn test_graph_boost_scored_uses_entity_chunk_id() {
        let db = make_db();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('lib.rs', 'rust', 10, 0, 'a')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'lib.rs', 'rust', 'function', 'database_pool', 0, 5, 'fn database_pool()', 'x')", []).unwrap();

        let input = SymbolEntityInput {
            name: "DatabasePool",
            entity_type: "function",
            file_path: "lib.rs",
            language: "rust",
            line_start: 0,
            line_end: 5,
            signature: None,
            chunk_id: Some(1),
        };
        let eid = ensure_symbol_entity(&db, &input).unwrap();

        let entity = get_entity(&db, "DatabasePool").unwrap().unwrap();
        assert_eq!(entity.chunk_id, Some(1));

        let scored = graph_boost_scored(&db, "DatabasePool", 10).unwrap();
        assert_eq!(
            scored.len(),
            1,
            "entity with chunk_id should produce a scored result, got {:?}",
            scored
        );
        assert_eq!(scored[0].0, 1);
    }
}
