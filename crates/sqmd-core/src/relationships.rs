use rusqlite::params;

#[derive(Debug, Clone)]
pub struct ImportInfo {
    pub source_file: String,
    pub module_path: String,
    pub names: Vec<String>,
}

#[allow(clippy::type_complexity)]
pub fn resolve_imports(db: &rusqlite::Connection, imports: &[ImportInfo]) -> Result<Vec<(i64, i64, String)>, Box<dyn std::error::Error>> {
    let mut relationships = Vec::new();

    for imp in imports {
        let resolved_names = resolve_module_path(db, &imp.source_file, &imp.module_path);

        for resolved_path in &resolved_names {
            let name_matches: Vec<String> = if imp.names.is_empty() {
                vec!["*".to_string()]
            } else {
                imp.names.clone()
            };

            for name in &name_matches {
                let mut stmt = if name == "*" {
                    db.prepare(
                        "SELECT id FROM chunks WHERE file_path = ?1 AND chunk_type IN ('function', 'class', 'interface', 'struct', 'enum', 'trait', 'constant', 'type', 'method', 'impl') AND (name IS NOT NULL)"
                    )?
                } else {
                    db.prepare(
                        "SELECT id FROM chunks WHERE file_path = ?1 AND name = ?2 AND chunk_type IN ('function', 'class', 'interface', 'struct', 'enum', 'trait', 'constant', 'type', 'method', 'impl')"
                    )?
                };

                let rows: Vec<i64> = if name == "*" {
                    stmt.query_map(params![resolved_path], |r| r.get(0))?
                        .collect::<Result<_, _>>()?
                } else {
                    stmt.query_map(params![resolved_path, name], |r| r.get(0))?
                        .collect::<Result<_, _>>()?
                };

                for target_id in rows {
                    let source_chunk_id: Option<i64> = db.query_row(
                        "SELECT id FROM chunks WHERE file_path = ?1 AND chunk_type = 'import' LIMIT 1",
                        params![imp.source_file],
                        |r| r.get(0),
                    ).ok();

                    if let Some(sid) = source_chunk_id {
                        relationships.push((sid, target_id, "imports".to_string()));
                    }
                }
            }
        }
    }

    relationships.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    relationships.dedup();
    Ok(relationships)
}

pub fn insert_relationships(db: &rusqlite::Connection, relationships: &[(i64, i64, String)]) -> Result<usize, Box<dyn std::error::Error>> {
    let mut count = 0;
    let mut stmt = db.prepare(
        "INSERT OR IGNORE INTO relationships (source_id, target_id, rel_type) VALUES (?1, ?2, ?3)"
    )?;

    for &(source_id, target_id, ref rel_type) in relationships {
        stmt.execute(params![source_id, target_id, rel_type])?;
        count += 1;
    }

    Ok(count)
}

fn resolve_module_path(db: &rusqlite::Connection, source_file: &str, module_path: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    let is_relative = module_path.starts_with('.') || module_path.contains('/') || module_path.contains('\\');
    let is_rust_crate_path = module_path.contains("::");

    if is_rust_crate_path {
        let source_ext = if source_file.ends_with(".rs") { "rs" } else { "" };

        if source_ext == "rs" {
            candidates.extend(resolve_rust_path(db, source_file, module_path));
        }

        if candidates.is_empty() {
            return candidates;
        }
    } else if is_relative {
        let mut source_dir = source_file.to_string();
        if let Some(idx) = source_dir.rfind('/') {
            source_dir.truncate(idx);
        } else {
            source_dir.clear();
        }

        let mut resolved_dir = source_dir;
        let mut name_parts: Vec<&str> = Vec::new();

        for segment in module_path.split('/') {
            match segment {
                "" => continue,
                "." => continue,
                ".." => {
                    if let Some(idx) = resolved_dir.rfind('/') {
                        resolved_dir.truncate(idx);
                    } else {
                        resolved_dir.clear();
                    }
                }
            s => {
                    name_parts.push(s);
                }
            }
        }

        if name_parts.is_empty() {
            return Vec::new();
        }

        let extensions = [
            "ts", "tsx", "js", "jsx", "rs", "py", "go", "java", "rb", "c", "cpp", "h",
        ];

        let base = name_parts.join("/");

        for ext in &extensions {
            if resolved_dir.is_empty() {
                candidates.push(format!("{}.{}", base, ext));
            } else {
                candidates.push(format!("{}/{}.{}", resolved_dir, base, ext));
            }
        }

        if resolved_dir.is_empty() {
            candidates.push(format!("{}/mod.rs", base));
            candidates.push(format!("{}/__init__.py", base));
            candidates.push(format!("{}/index.ts", base));
        } else {
            candidates.push(format!("{}/{}/mod.rs", resolved_dir, base));
            candidates.push(format!("{}/{}/__init__.py", resolved_dir, base));
            candidates.push(format!("{}/{}/index.ts", resolved_dir, base));
        }
    } else {
        let extensions = ["ts", "tsx", "js", "rs", "py", "go", "java"];
        for ext in &extensions {
            candidates.push(format!("{}.{}", module_path, ext));
        }
    }

    let existing: Vec<String> = {
        let placeholders: Vec<String> = candidates.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let sql = format!("SELECT DISTINCT path FROM files WHERE path IN ({})", placeholders.join(", "));
        let mut stmt = match db.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let params: Vec<&dyn rusqlite::ToSql> = candidates.iter().map(|c| c as &dyn rusqlite::ToSql).collect();
        stmt.query_map(params.as_slice(), |r| r.get(0))
            .ok()
            .and_then(|rows| rows.collect::<Result<_, _>>().ok())
            .unwrap_or_default()
    };

    existing
}

fn resolve_rust_path(db: &rusqlite::Connection, source_file: &str, module_path: &str) -> Vec<String> {
    let mut candidates = Vec::new();

    let mut source_dir = source_file.to_string();
    if let Some(idx) = source_dir.rfind('/') {
        source_dir.truncate(idx);
    }

    let segments: Vec<&str> = module_path.split("::").collect();

    let mut resolved_dir = source_dir.clone();
    let mut name_idx = 0;

    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            "crate" => {
                resolved_dir = find_crate_root(source_file);
                name_idx = i + 1;
            }
            "super" => {
                if let Some(idx) = resolved_dir.rfind('/') {
                    resolved_dir.truncate(idx);
                }
                name_idx = i + 1;
            }
            "self" => {
                name_idx = i + 1;
            }
            _s => {
                name_idx = i;
                break;
            }
        }
    }

    let path_parts: Vec<&str> = segments[name_idx..].to_vec();
    let is_mod = path_parts.is_empty();

    if is_mod {
        candidates.push(format!("{}/mod.rs", resolved_dir));
        candidates.push(format!("{}/lib.rs", resolved_dir));
    } else {
        let mut dir = resolved_dir.clone();
        for &part in &path_parts[..path_parts.len().saturating_sub(1)] {
            dir = format!("{}/{}", dir, to_snake_case(part));
        }
        let last = path_parts.last().copied().unwrap_or("");
        let snake = to_snake_case(last);

        candidates.push(format!("{}/{}.rs", dir, snake));
        candidates.push(format!("{}/{}/mod.rs", dir, snake));
    }

    let existing: Vec<String> = {
        let placeholders: Vec<String> = candidates.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect();
        let sql = format!("SELECT DISTINCT path FROM files WHERE path IN ({})", placeholders.join(", "));
        let mut stmt = match db.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let params: Vec<&dyn rusqlite::ToSql> = candidates.iter().map(|c| c as &dyn rusqlite::ToSql).collect();
        stmt.query_map(params.as_slice(), |r| r.get(0))
            .ok()
            .and_then(|rows| rows.collect::<Result<_, _>>().ok())
            .unwrap_or_default()
    };

    existing
}

fn find_crate_root(file_path: &str) -> String {
    let mut dir = file_path.to_string();
    loop {
        if dir.contains("src/") {
            if let Some(idx) = dir.find("src/") {
                dir.truncate(idx + 4);
                let trimmed = dir.trim_end_matches('/');
                return trimmed.to_string();
            }
        }
        if let Some(idx) = dir.rfind('/') {
            dir.truncate(idx);
        } else {
            return String::new();
        }
        if dir.is_empty() {
            return String::new();
        }
    }
}

fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

pub fn get_dependencies(db: &rusqlite::Connection, file_path: &str) -> Result<Vec<DepInfo>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT c.id, c.name, c.chunk_type, c.file_path, c.line_start, c.line_end, r.rel_type, t.name as target_name, t.file_path as target_file, t.line_start as target_start
         FROM relationships r
         JOIN chunks c ON r.source_id = c.id
         JOIN chunks t ON r.target_id = t.id
         WHERE c.file_path = ?1
         ORDER BY c.line_start, t.file_path"
    )?;

    let rows = stmt.query_map(params![file_path], |r| {
        Ok(DepInfo {
            source_chunk_id: r.get(0)?,
            source_name: r.get(1)?,
            source_type: r.get(2)?,
            source_file: r.get(3)?,
            source_line: r.get(4)?,
            target_name: r.get(7)?,
            target_file: r.get(8)?,
            target_line: r.get(9)?,
            rel_type: r.get(6)?,
        })
    })?.collect::<Result<_, _>>()?;

    Ok(rows)
}

pub fn get_dependents(db: &rusqlite::Connection, file_path: &str) -> Result<Vec<DepInfo>, Box<dyn std::error::Error>> {
    let mut stmt = db.prepare(
        "SELECT c.id, c.name, c.chunk_type, c.file_path, c.line_start, c.line_end, r.rel_type, t.name as target_name, t.file_path as target_file, t.line_start as target_start
         FROM relationships r
         JOIN chunks c ON r.source_id = c.id
         JOIN chunks t ON r.target_id = t.id
         WHERE t.file_path = ?1
         ORDER BY t.file_path, c.line_start"
    )?;

    let rows = stmt.query_map(params![file_path], |r| {
        Ok(DepInfo {
            source_chunk_id: r.get(0)?,
            source_name: r.get(1)?,
            source_type: r.get(2)?,
            source_file: r.get(3)?,
            source_line: r.get(4)?,
            target_name: r.get(7)?,
            target_file: r.get(8)?,
            target_line: r.get(9)?,
            rel_type: r.get(6)?,
        })
    })?.collect::<Result<_, _>>()?;

    Ok(rows)
}

pub fn get_dependency_ids(db: &rusqlite::Connection, chunk_id: i64, depth: usize) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    if depth == 0 {
        return Ok(Vec::new());
    }

    let sql = format!(
        "WITH RECURSIVE dep_graph(target_id, d) AS (
            SELECT target_id, 1 FROM relationships WHERE source_id = ?1 AND rel_type IN ('imports', 'calls')
            UNION
            SELECT r.target_id, dg.d + 1 FROM relationships r
            JOIN dep_graph dg ON r.source_id = dg.target_id
            WHERE dg.d < {0} AND r.rel_type IN ('imports', 'calls')
        )
        SELECT DISTINCT target_id FROM dep_graph WHERE target_id != ?1",
        depth,
    );

    let mut stmt = db.prepare(&sql)?;
    let ids: Vec<i64> = stmt.query_map(params![chunk_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

pub fn get_dependent_ids(db: &rusqlite::Connection, chunk_id: i64, depth: usize) -> Result<Vec<i64>, Box<dyn std::error::Error>> {
    if depth == 0 {
        return Ok(Vec::new());
    }

    let sql = format!(
        "WITH RECURSIVE dep_graph(source_id, d) AS (
            SELECT source_id, 1 FROM relationships WHERE target_id = ?1 AND rel_type IN ('imports', 'calls')
            UNION
            SELECT r.source_id, dg.d + 1 FROM relationships r
            JOIN dep_graph dg ON r.target_id = dg.source_id
            WHERE dg.d < {0} AND r.rel_type IN ('imports', 'calls')
        )
        SELECT DISTINCT source_id FROM dep_graph WHERE source_id != ?1",
        depth,
    );

    let mut stmt = db.prepare(&sql)?;
    let ids: Vec<i64> = stmt.query_map(params![chunk_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    Ok(ids)
}

pub fn extract_calls(content: &str) -> Vec<String> {
    let mut calls = Vec::new();

    let patterns: &[&str] = &[
        r"(\w+)\s*\(",
    ];

    let mut seen = std::collections::HashSet::new();
    for pattern in patterns {
        if let Ok(re) = regex::Regex::new(pattern) {
            for cap in re.captures_iter(content) {
                if let Some(name) = cap.get(1) {
                    let s = name.as_str().to_string();
                    // Filter out keywords and common builtins
                    if seen.insert(s.clone()) && !is_keyword(&s) {
                        calls.push(s);
                    }
                }
            }
        }
    }

    calls
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "if" | "else" | "for" | "while" | "match" | "return" | "await"
            | "async" | "let" | "const" | "var" | "fn" | "function"
            | "new" | "delete" | "throw" | "try" | "catch" | "finally"
            | "import" | "export" | "from" | "class" | "extends" | "super"
            | "this" | "self" | "Self" | "print" | "println"
            | "assert" | "assert_eq" | "assert_ne" | "assert!"
            | "vec" | "Vec" | "String" | "HashMap" | "Option" | "Result"
            | "Some" | "None" | "Ok" | "Err" | "true" | "false"
            | "mut" | "pub" | "use" | "mod" | "struct" | "enum"
            | "impl" | "trait" | "type" | "where" | "in" | "as" | "ref"
            | "static" | "dyn" | "box" | "move" | "loop" | "break"
            | "continue" | "yield" | "def" | "pass" | "with"
            | "isinstance"
    )
}

#[derive(Debug, Clone)]
pub struct DepInfo {
    pub source_chunk_id: i64,
    pub source_name: Option<String>,
    pub source_type: String,
    pub source_file: String,
    pub source_line: i64,
    pub target_name: Option<String>,
    pub target_file: String,
    pub target_line: i64,
    pub rel_type: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_relative_path() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/auth/utils/helpers.ts', 'typescript', 100, 0.0, 'abc')", []).unwrap();

        let results = resolve_module_path(&db, "src/auth/login.ts", "./utils/helpers");
        assert!(results.contains(&"src/auth/utils/helpers.ts".to_string()), "got: {:?}", results);
    }

    #[test]
    fn test_resolve_parent_path() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/utils/common.ts', 'typescript', 100, 0.0, 'abc')", []).unwrap();

        let results = resolve_module_path(&db, "src/auth/login.ts", "../utils/common");
        assert!(results.contains(&"src/utils/common.ts".to_string()), "got: {:?}", results);
    }

    #[test]
    fn test_resolve_sibling() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('src/utils/helpers.ts', 'typescript', 100, 0.0, 'abc')", []).unwrap();

        let results = resolve_module_path(&db, "src/auth/login.ts", "../utils/helpers");
        assert!(results.contains(&"src/utils/helpers.ts".to_string()), "got: {:?}", results);
    }

    #[test]
    fn test_resolve_rust_crate_path() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('crates/sqmd-core/src/chunker.rs', 'rust', 100, 0.0, 'abc')", []).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('crates/sqmd-core/src/index.rs', 'rust', 100, 0.0, 'abc')", []).unwrap();

        let results = resolve_module_path(&db, "crates/sqmd-core/src/index.rs", "crate::chunker");
        assert!(!results.is_empty(), "should resolve crate::chunker, got: {:?}", results);
        assert!(results.contains(&"crates/sqmd-core/src/chunker.rs".to_string()), "got: {:?}", results);
    }

    #[test]
    fn test_resolve_nonexistent() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();

        let results = resolve_module_path(&db, "src/auth/login.ts", "./nonexistent");
        assert!(results.is_empty());
    }

    #[test]
    fn test_extract_calls() {
        let code = "async function login(user) { const result = await db.find(user); return session.create(result); }";
        let calls = extract_calls(code);
        assert!(calls.contains(&"find".to_string()), "got: {:?}", calls);
        assert!(calls.contains(&"create".to_string()), "got: {:?}", calls);
    }

    #[test]
    fn test_extract_calls_filters_keywords() {
        let code = "if (x) { return fn(); }";
        let calls = extract_calls(code);
        assert!(!calls.iter().any(|c| c == "if"));
        assert!(!calls.iter().any(|c| c == "return"));
    }

    #[test]
    fn test_get_dependency_ids_with_depth() {
        let mut db = rusqlite::Connection::open_in_memory().unwrap();
        crate::schema::init(&mut db).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('a.ts', 'typescript', 10, 0.0, 'a')", []).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('b.ts', 'typescript', 10, 0.0, 'b')", []).unwrap();
        db.execute("INSERT INTO files (path, language, size, mtime, hash) VALUES ('c.ts', 'typescript', 10, 0.0, 'c')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (1, 'a.ts', 'typescript', 'function', 'fa', 0, 1, 'fn fa()', 'x')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (2, 'b.ts', 'typescript', 'function', 'fb', 0, 1, 'fn fb()', 'y')", []).unwrap();
        db.execute("INSERT INTO chunks (id, file_path, language, chunk_type, name, line_start, line_end, content_raw, content_hash) VALUES (3, 'c.ts', 'typescript', 'function', 'fc', 0, 1, 'fn fc()', 'z')", []).unwrap();
        db.execute("INSERT INTO relationships (source_id, target_id, rel_type) VALUES (1, 2, 'imports')", []).unwrap();
        db.execute("INSERT INTO relationships (source_id, target_id, rel_type) VALUES (2, 3, 'imports')", []).unwrap();

        // depth=1: only direct deps of chunk 1
        let d1 = get_dependency_ids(&db, 1, 1).unwrap();
        assert_eq!(d1, vec![2]);

        // depth=2: includes transitive
        let d2 = get_dependency_ids(&db, 1, 2).unwrap();
        assert!(d2.contains(&2));
        assert!(d2.contains(&3));
    }
}
