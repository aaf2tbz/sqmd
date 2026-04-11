use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;

const SERVER_NAME: &str = "sqmd";
const SERVER_VERSION: &str = "3.0.0";
const PROTOCOL_VERSION: &str = "2025-03-26";

pub fn run(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let db = crate::schema::open_fast(db_path)?;
    let mut stdin = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();

    loop {
        let mut content_length: Option<usize> = None;
        loop {
            let mut header = String::new();
            let n = stdin.read_line(&mut header)?;
            if n == 0 {
                return Ok(());
            }
            let trimmed = header.trim();
            if trimmed.is_empty() {
                break;
            }
            if let Some(len_str) = trimmed.strip_prefix("Content-Length:") {
                content_length = Some(len_str.trim().parse()?);
            }
        }

        if let Some(len) = content_length {
            let mut buf = vec![0u8; len];
            stdin.read_exact(&mut buf)?;
            let msg: Value = serde_json::from_slice(&buf)?;
            let response = handle_message(&db, &msg);
            send_response(&mut stdout, &response)?;
        }
    }
}

fn send_response(
    stdout: &mut impl Write,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = serde_json::to_string(response)?;
    write!(stdout, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    stdout.flush()?;
    Ok(())
}

fn handle_message(db: &Connection, msg: &Value) -> Value {
    let method = msg["method"].as_str().unwrap_or("");
    let id = msg.get("id").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {
                    "tools": { "listChanged": false }
                },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": SERVER_VERSION
                }
            }
        }),
        "notifications/initialized" => json!({}),
        "tools/list" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "tools": tools()
            }
        }),
        "tools/call" => {
            let tool_name = msg["params"]["name"].as_str().unwrap_or("");
            let arguments = &msg["params"]["arguments"];
            let result = call_tool(db, tool_name, arguments);
            match result {
                Ok(content) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": content,
                        "isError": false
                    }
                }),
                Err(e) => json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "content": [{ "type": "text", "text": format!("Error: {e}") }],
                        "isError": true
                    }
                }),
            }
        }
        _ => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") }
        }),
    }
}

fn tools() -> Vec<Value> {
    vec![
        json!({
            "name": "search",
            "description": "Search the code index using layered search (FTS + entity graph + community + vector + hint vector). Returns ranked code chunks with file paths, names, signatures, line numbers, and scores.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query text" },
                    "top_k": { "type": "integer", "description": "Maximum results (default: 10)", "default": 10 },
                    "file_filter": { "type": "string", "description": "Filter by file path substring" },
                    "type_filter": { "type": "string", "description": "Filter by chunk type (function, class, method, struct, enum, etc.)" },
                    "source_filter": { "type": "string", "description": "Filter by source type (code, memory, transcript, document, entity)" }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "context",
            "description": "Assemble context for a query — returns ranked code chunks formatted as markdown, ready for inclusion in an LLM prompt.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Query to find relevant context for" },
                    "top_k": { "type": "integer", "description": "Maximum chunks (default: 10)", "default": 10 },
                    "max_tokens": { "type": "integer", "description": "Token budget for context window (default: 8000)", "default": 8000 }
                },
                "required": ["query"]
            }
        }),
        json!({
            "name": "deps",
            "description": "Get dependency graph for a file — returns files that the target file imports/depends on and files that depend on it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path to analyze" },
                    "depth": { "type": "integer", "description": "Traversal depth (1 = direct only, default: 2)", "default": 2 }
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "stats",
            "description": "Get index statistics — total chunks, files, embedding coverage, entity counts, and more.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "get",
            "description": "Get a specific code chunk at a file path and line number.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_path": { "type": "string", "description": "File path" },
                    "line": { "type": "integer", "description": "Line number (1-indexed)" }
                },
                "required": ["file_path", "line"]
            }
        }),
    ]
}

fn call_tool(
    db: &Connection,
    name: &str,
    args: &Value,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    match name {
        "search" => tool_search(db, args),
        "context" => tool_context(db, args),
        "deps" => tool_deps(db, args),
        "stats" => tool_stats(db),
        "get" => tool_get(db, args),
        _ => Err(format!("Unknown tool: {name}").into()),
    }
}

fn tool_search(db: &Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let query_text = args["query"].as_str().ok_or("missing 'query' parameter")?;
    let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;
    let file_filter = args["file_filter"].as_str().map(|s| s.to_string());
    let type_filter = args["type_filter"].as_str().map(|s| s.to_string());
    let source_filter = args["source_filter"].as_str().map(|s| vec![s.to_string()]);

    let search_query = crate::search::SearchQuery {
        text: query_text.to_string(),
        top_k,
        file_filter,
        type_filter,
        source_type_filter: source_filter,
        ..Default::default()
    };

    let results = {
        #[cfg(feature = "native")]
        {
            let mut provider = crate::embed::make_provider()?;
            crate::search::layered_search(db, &search_query, Some(&mut *provider))
                .map(|lr| lr.results)?
        }
        #[cfg(not(feature = "native"))]
        {
            crate::search::fts_search(db, &search_query)?
        }
    };

    let markdown = crate::search::render_search_markdown(db, &results)?;

    let mut text = String::new();
    for (i, r) in results.iter().enumerate() {
        let md = markdown.get(i).map(|s| s.as_str()).unwrap_or("");
        text.push_str(&format!(
            "## Result {} (score: {:.3})\n**{}** `{}` at {}:{}-{}\n",
            i + 1,
            r.score,
            r.chunk_type,
            r.name.as_deref().unwrap_or("(anonymous)"),
            r.file_path,
            r.line_start + 1,
            r.line_end + 1
        ));
        if !md.is_empty() {
            text.push_str(md);
            text.push('\n');
        }
        text.push('\n');
    }

    Ok(vec![json!({ "type": "text", "text": text })])
}

fn tool_context(db: &Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let query_text = args["query"].as_str().ok_or("missing 'query' parameter")?;
    let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;
    let max_tokens = args["max_tokens"].as_u64().unwrap_or(8000) as usize;

    let request = crate::context::ContextRequest {
        query: query_text.to_string(),
        top_k,
        max_tokens,
        files: Vec::new(),
        include_deps: false,
        dep_depth: 1,
        source_types: None,
    };

    let response = crate::context::ContextAssembler::build(db, &request)?;

    let mut text = format!(
        "# Context for: \"{}\"\nChunks: {} | Tokens: {}\n\n",
        query_text, response.chunk_count, response.token_count
    );
    text.push_str(&response.markdown);

    Ok(vec![json!({ "type": "text", "text": text })])
}

fn tool_deps(db: &Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let path = args["path"].as_str().ok_or("missing 'path' parameter")?;

    let imports = crate::relationships::get_dependencies(db, path)?;
    let dependents = crate::relationships::get_dependents(db, path)?;

    let mut text = format!("# Dependencies for: {}\n\n", path);

    if !imports.is_empty() {
        text.push_str("## Imports\n");
        for dep in &imports {
            text.push_str(&format!(
                "- {}:{} {} ({})\n",
                dep.target_file,
                dep.target_line + 1,
                dep.target_name.as_deref().unwrap_or("(unnamed)"),
                dep.rel_type,
            ));
        }
        text.push('\n');
    }

    if !dependents.is_empty() {
        text.push_str("## Imported by\n");
        for dep in &dependents {
            text.push_str(&format!(
                "- {}:{} {} ({})\n",
                dep.source_file,
                dep.source_line + 1,
                dep.source_name.as_deref().unwrap_or("(unnamed)"),
                dep.rel_type,
            ));
        }
        text.push('\n');
    }

    if imports.is_empty() && dependents.is_empty() {
        text.push_str("No dependencies found.\n");
    }

    Ok(vec![json!({ "type": "text", "text": text })])
}

fn tool_stats(db: &Connection) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let total_chunks: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let total_files: i64 =
        db.query_row("SELECT COUNT(DISTINCT file_path) FROM chunks", [], |r| {
            r.get(0)
        })?;
    let embedded: i64 = db.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
    let entities: i64 = db.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
    let communities: i64 = db.query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))?;

    let type_breakdown: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT chunk_type, COUNT(*) as cnt FROM chunks GROUP BY chunk_type ORDER BY cnt DESC",
        )?;
        let rows: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<Result<_, _>>()?;
        rows
    };

    let mut text = String::from("# Index Statistics\n\n");
    text.push_str(&format!("- **Chunks**: {}\n", total_chunks));
    text.push_str(&format!("- **Files**: {}\n", total_files));
    text.push_str(&format!(
        "- **Embedded**: {}/{} ({:.1}%)\n",
        embedded,
        total_chunks,
        if total_chunks > 0 {
            embedded as f64 / total_chunks as f64 * 100.0
        } else {
            0.0
        }
    ));
    text.push_str(&format!("- **Entities**: {}\n", entities));
    text.push_str(&format!("- **Communities**: {}\n", communities));

    text.push_str("\n## Chunk types\n");
    for (ct, count) in &type_breakdown {
        text.push_str(&format!("- {}: {}\n", ct, count));
    }

    Ok(vec![json!({ "type": "text", "text": text })])
}

fn tool_get(db: &Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let file_path = args["file_path"]
        .as_str()
        .ok_or("missing 'file_path' parameter")?;
    let line = args["line"].as_u64().ok_or("missing 'line' parameter")? as i64;

    let result = db.query_row(
        "SELECT id, name, chunk_type, line_start, line_end, content_raw, signature FROM chunks WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2 ORDER BY importance DESC LIMIT 1",
        params![file_path, line],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        },
    );

    match result {
        Ok((id, name, chunk_type, line_start, line_end, content, signature)) => {
            let mut text = format!(
                "# {} `{}` at {}:{}-{}\n",
                chunk_type,
                name.as_deref().unwrap_or("(anonymous)"),
                file_path,
                line_start + 1,
                line_end + 1
            );
            if let Some(sig) = &signature {
                text.push_str(&format!("Signature: `{}`\n\n", sig));
            }
            if let Some(c) = &content {
                text.push_str(&format!("```\n{}\n```\n", c));
            }
            text.push_str(&format!("\nChunk ID: {}", id));
            Ok(vec![json!({ "type": "text", "text": text })])
        }
        Err(_) => Ok(vec![json!({
            "type": "text",
            "text": format!("No chunk found at {}:{} ", file_path, line)
        })]),
    }
}

use rusqlite::params;
