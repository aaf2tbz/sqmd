use rusqlite::Connection;
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const SERVER_NAME: &str = "sqmd";
const SERVER_VERSION: &str = "3.4.1";
const PROTOCOL_VERSION: &str = "2024-11-05";

struct EmbedState {
    total: usize,
    embedded: usize,
    pending: usize,
    started_at: std::time::Instant,
    chunks_per_sec: f64,
    is_running: bool,
    error: Option<String>,
}

impl EmbedState {
    fn new(total: usize, pending: usize) -> Self {
        Self {
            total,
            embedded: 0,
            pending,
            started_at: std::time::Instant::now(),
            chunks_per_sec: 0.0,
            is_running: true,
            error: None,
        }
    }

    fn bar(&self) -> String {
        if self.total == 0 {
            return String::new();
        }
        let pct = self.embedded as f64 / self.total as f64;
        let filled = (pct * 20.0).round() as usize;
        let empty = 20 - filled;
        format!(
            "{}{} {:.0}%",
            "█".repeat(filled),
            "░".repeat(empty),
            pct * 100.0
        )
    }
}

pub fn run(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if !db_path.exists() {
        eprintln!(
            "[sqmd mcp] ERROR: index not found at {:?}. Run `sqmd init` in the project directory first.",
            db_path
        );
        std::process::exit(1);
    }

    let root = project_root_from_index_db(db_path);
    let config = crate::config::ProjectConfig::load(&root);

    let log_path = std::path::PathBuf::from("/tmp/sqmd-mcp-debug.log");
    let start = std::time::Instant::now();

    let _ = std::fs::remove_file(&log_path);
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);

    macro_rules! dbg {
        ($f:expr, $($arg:tt)*) => {
            if let Ok(ref mut f) = $f {
                let _ = writeln!(f, "[{}] {}", chrono::Local::now().format("%H:%M:%S%.3f"), format!($($arg)*));
                let _ = f.flush();
            }
        };
    }

    dbg!(
        log,
        "=== sqmd MCP starting === pid={} CWD={:?} db_path={:?}",
        std::process::id(),
        std::env::current_dir().unwrap_or_default().display(),
        db_path.display()
    );
    dbg!(log, "PATH={:?}", std::env::var("PATH").unwrap_or_default());
    dbg!(log, "args={:?}", std::env::args().collect::<Vec<_>>());

    let mut db = match crate::schema::open_with_config(db_path, &config) {
        Ok(db) => db,
        Err(e) => {
            dbg!(
                log,
                "ERROR: schema::open failed: {} ({:.1}ms)",
                e,
                start.elapsed().as_millis()
            );
            eprintln!(
                "[sqmd mcp] ERROR: failed to open index: {}. Run `sqmd init` first.",
                e
            );
            return Err(Box::new(e));
        }
    };

    let total_chunks: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if total_chunks == 0 {
        eprintln!("[sqmd mcp] WARNING: index has 0 chunks. Did you run `sqmd index`?");
    }

    dbg!(
        log,
        "DB opened ({:.1}ms), {} chunks",
        start.elapsed().as_millis(),
        total_chunks
    );

    let embed_state: Arc<Mutex<EmbedState>> = Arc::new(Mutex::new(EmbedState {
        total: 0,
        embedded: 0,
        pending: 0,
        started_at: std::time::Instant::now(),
        chunks_per_sec: 0.0,
        is_running: false,
        error: None,
    }));
    let mut stdin = BufReader::new(std::io::stdin());
    let mut stdout = std::io::stdout();
    let mut msg_count: usize = 0;

    dbg!(log, "Entering message loop...");

    let mut framed_mode: Option<bool> = None;

    loop {
        let mut line = String::new();
        let n = match stdin.read_line(&mut line) {
            Ok(n) => n,
            Err(e) => {
                dbg!(
                    log,
                    "stdin read_line error: {} (kind={:?}, os={:?})",
                    e,
                    e.kind(),
                    e.raw_os_error()
                );
                if e.kind() == std::io::ErrorKind::BrokenPipe || e.raw_os_error() == Some(32) {
                    dbg!(log, "Broken pipe — exiting");
                    return Ok(());
                }
                return Err(Box::new(e));
            }
        };
        if n == 0 {
            dbg!(log, "stdin EOF — exiting");
            return Ok(());
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let is_framed = line.starts_with("Content-Length:");
        if framed_mode.is_none() {
            framed_mode = Some(is_framed);
            dbg!(
                log,
                "Detected transport mode: {}",
                if is_framed {
                    "Content-Length framed (stdio)"
                } else {
                    "raw JSON line-delimited"
                }
            );
        }

        let msg: Value = if is_framed {
            let len_str = line.strip_prefix("Content-Length:").unwrap();
            let len: usize = match len_str.trim().parse() {
                Ok(l) => l,
                Err(e) => {
                    dbg!(log, "Failed to parse Content-Length: {:?} ({})", len_str, e);
                    continue;
                }
            };
            let mut sep = String::new();
            let _ = stdin.read_line(&mut sep);
            let mut buf = vec![0u8; len];
            if stdin.read_exact(&mut buf).is_err() {
                dbg!(log, "Failed to read {} bytes of body", len);
                continue;
            }
            let body_str = String::from_utf8_lossy(&buf);
            dbg!(
                log,
                "MSG [{}]: {}",
                msg_count,
                &body_str[..body_str.len().min(500)]
            );
            match serde_json::from_slice(&buf) {
                Ok(m) => m,
                Err(e) => {
                    dbg!(log, "JSON parse error: {}", e);
                    continue;
                }
            }
        } else if line.starts_with('{') {
            dbg!(
                log,
                "MSG [{}] (raw JSON): {}",
                msg_count,
                &line[..line.len().min(500)]
            );
            match serde_json::from_str(line) {
                Ok(m) => m,
                Err(e) => {
                    dbg!(log, "JSON parse error: {}", e);
                    continue;
                }
            }
        } else {
            dbg!(log, "Skipping line: {}", &line[..line.len().min(200)]);
            continue;
        };

        msg_count += 1;

        let has_id = msg.get("id").is_some();
        let method = msg["method"].as_str().unwrap_or("");
        dbg!(
            log,
            "Processing msg #{}: method={} has_id={}",
            msg_count,
            method,
            has_id
        );

        if !has_id
            && msg.get("method").is_some()
            && (method == "notifications/initialized" || method == "initialized")
        {
            dbg!(log, "Skipping notification: {}", method);
            continue;
        }

        let response = handle_message(&mut db, &root, db_path, &msg, &embed_state);
        dbg!(
            log,
            "Response for msg #{}: {}",
            msg_count,
            serde_json::to_string(&response).unwrap_or_else(|_| "SER_ERROR".into())
        );

        if has_id {
            if framed_mode.unwrap_or(false) {
                if let Err(e) = send_response_framed(&mut stdout, &response) {
                    dbg!(log, "send_response_framed ERROR: {}", e);
                    return Err(e);
                }
            } else {
                if let Err(e) = send_response_raw(&mut stdout, &response) {
                    dbg!(log, "send_response_raw ERROR: {}", e);
                    return Err(e);
                }
            }
            dbg!(
                log,
                "Response sent for msg #{} (mode={})",
                msg_count,
                if framed_mode.unwrap_or(false) {
                    "framed"
                } else {
                    "raw"
                }
            );
        } else {
            dbg!(log, "No id — notification, not sending response");
        }
    }
}

fn project_root_from_index_db(db_path: &Path) -> PathBuf {
    let inferred = db_path
        .parent()
        .and_then(|dir| {
            if dir.file_name().is_some_and(|name| name == ".sqmd") {
                dir.parent()
            } else {
                Some(dir)
            }
        })
        .unwrap_or(db_path)
        .to_path_buf();

    let home = dirs::home_dir();
    let is_home_dir = home
        .as_ref()
        .is_some_and(|h| inferred == *h || inferred.as_os_str().is_empty());

    if is_home_dir {
        if let Ok(cwd) = std::env::current_dir() {
            if cwd != inferred {
                eprintln!(
                    "[sqmd mcp] WARNING: index at {:?} resolves to home directory as project root. \
                     Using CWD {:?} instead. Relative paths in chunks may not match.",
                    db_path, cwd
                );
                return cwd;
            }
        }
    }

    inferred
}

#[cfg(test)]
mod tests {
    use super::project_root_from_index_db;
    use std::path::Path;

    #[test]
    fn mcp_project_root_is_parent_of_sqmd_dir() {
        let root = project_root_from_index_db(Path::new("/repo/.sqmd/index.db"));
        assert_eq!(root, Path::new("/repo"));
    }

    #[test]
    fn mcp_project_root_home_dir_falls_back_to_cwd() {
        let home = dirs::home_dir().unwrap();
        let home_db = home.join(".sqmd/index.db");
        let root = project_root_from_index_db(&home_db);
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            root, cwd,
            "should fall back to CWD when index resolves to home"
        );
    }

    #[test]
    fn mcp_project_root_non_home_dir_unchanged() {
        let root = project_root_from_index_db(Path::new("/opt/project/.sqmd/index.db"));
        assert_eq!(root, Path::new("/opt/project"));
    }
}

fn send_response_framed(
    stdout: &mut impl Write,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = serde_json::to_string(response)?;
    write!(stdout, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    stdout.flush()?;
    Ok(())
}

fn send_response_raw(
    stdout: &mut impl Write,
    response: &Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let body = serde_json::to_string(response)?;
    writeln!(stdout, "{}", body)?;
    stdout.flush()?;
    Ok(())
}

fn handle_message(
    db: &mut Connection,
    root: &Path,
    db_path: &Path,
    msg: &Value,
    embed_state: &Arc<Mutex<EmbedState>>,
) -> Value {
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
        "notifications/initialized" | "initialized" => Value::Null,
        "ping" => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {}
        }),
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
            let result = call_tool(db, root, db_path, tool_name, arguments, embed_state);
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
        json!({
            "name": "index_file",
            "description": "Index a single file (or all files if no path given). Re-indexes changed files incrementally. Use this after editing files to keep the index up-to-date.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "File path relative to project root. Omit to index all changed files." }
                }
            }
        }),
        json!({
            "name": "embed",
            "description": "Embed unembedded chunks using local llama.cpp. Processes up to batch_size chunks (default: 64). Call repeatedly until all chunks are embedded.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "batch_size": { "type": "integer", "description": "Max chunks to embed in this call (default: 64)", "default": 64 }
                }
            }
        }),
        json!({
            "name": "ls",
            "description": "List chunks in the index, optionally filtered by file, type, or language. Returns chunk IDs, names, types, and locations.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "file_filter": { "type": "string", "description": "Filter by file path substring" },
                    "type_filter": { "type": "string", "description": "Filter by chunk type (function, class, method, struct, enum, etc.)" },
                    "language": { "type": "string", "description": "Filter by language (typescript, rust, python, etc.)" },
                    "limit": { "type": "integer", "description": "Max results (default: 50)", "default": 50 }
                }
            }
        }),
        json!({
            "name": "cat",
            "description": "Get a chunk by its ID (from ls or search results). Returns full source code.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Chunk ID" }
                },
                "required": ["id"]
            }
        }),
        json!({
            "name": "embed",
            "description": "Embed unembedded chunks (blocking). Processes batches until done or batch_size reached. For progress tracking, prefer embed_start + embed_progress.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "batch_size": { "type": "integer", "description": "Max chunks to embed (default: 64)", "default": 64 }
                }
            }
        }),
        json!({
            "name": "embed_start",
            "description": "Start embedding in the background. Returns immediately. Poll embed_progress for status.",
            "inputSchema": {
                "type": "object",
                "properties": {}
            }
        }),
        json!({
            "name": "embed_progress",
            "description": "Get embedding progress. Returns current status, percentage, progress bar, ETA.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "embed_stop",
            "description": "Stop a running embedding job. Will stop after the current batch completes.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "health",
            "description": "Check index health — integrity, orphan counts, index/WAL size, FTS/vector consistency.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": "projects",
            "description": "List registered projects, add/remove projects, or search across multiple projects.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["list", "add", "remove", "search"],
                        "description": "Action to perform"
                    },
                    "name": { "type": "string", "description": "Project name (for add/remove)" },
                    "path": { "type": "string", "description": "Project root path (for add)" },
                    "query": { "type": "string", "description": "Search query (for search action)" },
                    "top_k": { "type": "integer", "description": "Max results (for search action)", "default": 10 }
                },
                "required": ["action"]
            }
        }),
    ]
}

fn call_tool(
    db: &mut Connection,
    root: &Path,
    db_path: &Path,
    name: &str,
    args: &Value,
    embed_state: &Arc<Mutex<EmbedState>>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    match name {
        "search" => tool_search(db, args),
        "context" => tool_context(db, args),
        "deps" => tool_deps(db, args),
        "stats" => tool_stats(db),
        "get" => tool_get(db, args),
        "index_file" => tool_index_file(db, root, args),
        "embed" => tool_embed(db, args),
        "embed_start" => tool_embed_start(db, db_path, embed_state),
        "embed_progress" => tool_embed_progress(embed_state),
        "embed_stop" => tool_embed_stop(embed_state),
        "ls" => tool_ls(db, args),
        "cat" => tool_cat(db, args),
        "health" => tool_health(db),
        "projects" => tool_projects(args),
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
        max_dep_chunks: 50,
        community_boost: 0.1,
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
    let total_chunks: i64 = db.query_row(
        "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )?;
    let total_files: i64 = db.query_row(
        "SELECT COUNT(DISTINCT file_path) FROM chunks WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )?;
    let embedded: i64 = db.query_row(
        "SELECT COUNT(*) FROM chunks c INNER JOIN embeddings e ON e.chunk_id = c.id WHERE c.is_deleted = 0",
        [],
        |r| r.get(0),
    )?;
    let entities: i64 = db.query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))?;
    let communities: i64 = db.query_row("SELECT COUNT(*) FROM communities", [], |r| r.get(0))?;

    let type_breakdown: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT chunk_type, COUNT(*) as cnt FROM chunks WHERE is_deleted = 0 GROUP BY chunk_type ORDER BY cnt DESC",
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
        "SELECT id, name, chunk_type, line_start, line_end, content_raw, signature FROM chunks WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2 AND is_deleted = 0 ORDER BY importance DESC LIMIT 1",
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

fn tool_index_file(
    db: &mut Connection,
    root: &Path,
    args: &Value,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut indexer = crate::index::Indexer::new(db, root);

    if let Some(path) = args["path"].as_str() {
        let abs = if std::path::Path::new(path).is_absolute() {
            std::path::PathBuf::from(path)
        } else {
            root.join(path)
        };
        if !abs.exists() {
            return Ok(vec![json!({
                "type": "text",
                "text": format!("Error: file not found: {:?} (resolved from path={:?}, root={:?})", abs, path, root)
            })]);
        }
        match indexer.index_file(&abs) {
            Ok(Some(result)) => {
                let text = format!(
                    "Indexed: {}\n  Chunks: {}\n  Relationships: {}{}",
                    result.file_path,
                    result.chunks,
                    result.relationships,
                    if result.was_deleted {
                        " (file deleted, tombstoned)"
                    } else {
                        ""
                    },
                );
                Ok(vec![json!({ "type": "text", "text": text })])
            }
            Ok(None) => Ok(vec![json!({
                "type": "text",
                "text": format!("Skipped: {} (unsupported language)", path)
            })]),
            Err(e) => Ok(vec![json!({
                "type": "text",
                "text": format!("Error indexing {}: {}", path, e)
            })]),
        }
    } else {
        match indexer.index() {
            Ok(stats) => {
                let text = format!(
                    "Full index complete.\n  Files scanned: {}\n  Files indexed: {}\n  Files skipped: {}\n  Chunks: {}\n  Relationships: {}\n",
                    stats.files_scanned,
                    stats.files_indexed,
                    stats.files_skipped,
                    stats.chunks_total,
                    stats.relationships_total,
                );
                Ok(vec![json!({ "type": "text", "text": text })])
            }
            Err(e) => Ok(vec![json!({
                "type": "text",
                "text": format!("Error: {}", e)
            })]),
        }
    }
}

fn tool_embed(db: &mut Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    #[cfg(feature = "native")]
    {
        let batch_size = args["batch_size"].as_u64().unwrap_or(64) as usize;
        let mut provider = crate::embed::make_provider()?;

        let unembedded: i64 = db.query_row(
            "SELECT COUNT(*) FROM chunks LEFT JOIN embeddings ON chunks.id = embeddings.chunk_id WHERE embeddings.chunk_id IS NULL AND chunks.is_deleted = 0",
            [],
            |r| r.get(0),
        )?;

        if unembedded == 0 {
            return Ok(vec![json!({
                "type": "text",
                "text": "All chunks already embedded."
            })]);
        }

        let mut total = 0usize;
        let start = std::time::Instant::now();
        loop {
            let count = crate::search::embed_unembedded(db, &mut *provider)?;
            if count == 0 || total + count > batch_size {
                total += count.min(batch_size.saturating_sub(total));
                break;
            }
            total += count;
            if total >= batch_size {
                break;
            }
        }

        let elapsed = start.elapsed();
        let remaining = unembedded as usize - total;
        let text = format!(
            "Embedded {}/{} chunks in {:?} ({:.0} chunks/sec)\n{} remaining.",
            total,
            unembedded,
            elapsed,
            if elapsed.as_secs_f64() > 0.0 {
                total as f64 / elapsed.as_secs_f64()
            } else {
                0.0
            },
            remaining,
        );
        Ok(vec![json!({ "type": "text", "text": text })])
    }

    #[cfg(not(feature = "native"))]
    {
        let _ = (db, args);
        Ok(vec![json!({
            "type": "text",
            "text": "Embedding requires the 'native' feature. Rebuild with --features native."
        })])
    }
}

fn tool_embed_start(
    db: &Connection,
    db_path: &Path,
    embed_state: &Arc<Mutex<EmbedState>>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    #[cfg(feature = "native")]
    {
        let mut state = embed_state.lock().map_err(|e| e.to_string())?;
        if state.is_running {
            return Ok(vec![json!({
                "type": "text",
                "text": json!({
                    "status": "already_running",
                    "embedded": state.embedded,
                    "total": state.total
                })
            })]);
        }

        let total_chunks: i64 = db.query_row(
            "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
            [],
            |r| r.get(0),
        )?;
        let embedded_chunks: i64 =
            db.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
        let pending = (total_chunks - embedded_chunks) as usize;

        if pending == 0 {
            return Ok(vec![json!({
                "type": "text",
                "text": "All chunks already embedded. Nothing to do."
            })]);
        }

        *state = EmbedState::new(total_chunks as usize, pending);

        let state_clone = embed_state.clone();
        let db_path_owned = db_path.to_path_buf();

        std::thread::spawn(move || {
            let db_result = crate::schema::open(&db_path_owned);
            let mut db = match db_result {
                Ok(db) => db,
                Err(e) => {
                    if let Ok(mut s) = state_clone.lock() {
                        s.is_running = false;
                        s.error = Some(format!("Failed to open DB: {e}"));
                    }
                    return;
                }
            };

            let provider_result = crate::embed::make_provider();
            let mut provider = match provider_result {
                Ok(p) => p,
                Err(e) => {
                    if let Ok(mut s) = state_clone.lock() {
                        s.is_running = false;
                        s.error = Some(format!("Failed to create embedder: {e}"));
                    }
                    return;
                }
            };

            loop {
                if let Ok(s) = state_clone.lock() {
                    if !s.is_running {
                        break;
                    }
                }

                match crate::search::embed_unembedded(&mut db, &mut *provider) {
                    Ok(0) => {
                        if let Ok(mut s) = state_clone.lock() {
                            s.is_running = false;
                            s.embedded = s.total;
                            s.pending = 0;
                        }
                        break;
                    }
                    Ok(count) => {
                        if let Ok(mut s) = state_clone.lock() {
                            s.embedded += count;
                            s.pending = s.total.saturating_sub(s.embedded);
                            let elapsed = s.started_at.elapsed().as_secs_f64();
                            if elapsed > 0.0 {
                                s.chunks_per_sec = s.embedded as f64 / elapsed;
                            }
                        }
                    }
                    Err(e) => {
                        if let Ok(mut s) = state_clone.lock() {
                            s.is_running = false;
                            s.error = Some(format!("Batch failed: {e}"));
                        }
                        break;
                    }
                }
            }
        });

        Ok(vec![json!({
            "type": "text",
            "text": json!({
                "status": "started",
                "total": state.total,
                "pending": state.pending
            })
        })])
    }

    #[cfg(not(feature = "native"))]
    {
        let _ = (db, embed_state);
        Ok(vec![json!({
            "type": "text",
            "text": "Embedding requires the 'native' feature. Rebuild with --features native."
        })])
    }
}

fn tool_embed_progress(
    embed_state: &Arc<Mutex<EmbedState>>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let state = embed_state.lock().map_err(|e| e.to_string())?;

    let mut response = json!({
        "status": if state.is_running { "running" } else if state.error.is_some() { "error" } else if state.total > 0 { "complete" } else { "idle" },
        "total": state.total,
        "embedded": state.embedded,
        "pending": state.pending,
    });

    if state.total > 0 {
        let pct = state.embedded as f64 / state.total as f64 * 100.0;
        let elapsed = state.started_at.elapsed().as_secs();
        let eta = if state.chunks_per_sec > 0.0 {
            Some((state.pending as f64 / state.chunks_per_sec).round() as u64)
        } else {
            None
        };

        response["percent"] = json!(pct);
        response["chunks_per_sec"] = json!(state.chunks_per_sec);
        response["elapsed_secs"] = json!(elapsed);
        response["bar"] = json!(state.bar());

        if let Some(eta_secs) = eta {
            response["eta_secs"] = json!(eta_secs);
        }

        if !state.is_running && state.error.is_none() && state.embedded > 0 {
            let dur = state.started_at.elapsed();
            response["summary"] = json!(format!(
                "Embedded {}/{} chunks in {} ({:.0} chunks/sec)",
                state.embedded,
                state.total,
                humantime(dur),
                state.chunks_per_sec
            ));
        }
    }

    if let Some(ref err) = state.error {
        response["error"] = json!(err);
        response["embedded_before_error"] = json!(state.embedded);
    }

    if state.total == 0 {
        response["message"] = json!("No embedding in progress. Call embed_start to begin.");
    }

    Ok(vec![
        json!({ "type": "text", "text": serde_json::to_string_pretty(&response).unwrap_or_default() }),
    ])
}

fn tool_embed_stop(
    embed_state: &Arc<Mutex<EmbedState>>,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let mut state = embed_state.lock().map_err(|e| e.to_string())?;
    if state.is_running {
        state.is_running = false;
        Ok(vec![json!({
            "type": "text",
            "text": json!({
                "status": "stopping",
                "embedded": state.embedded,
                "message": "Will stop after current batch completes."
            })
        })])
    } else {
        Ok(vec![json!({
            "type": "text",
            "text": "No embedding in progress."
        })])
    }
}

fn humantime(dur: std::time::Duration) -> String {
    let secs = dur.as_secs();
    if secs < 60 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    let rem_secs = secs % 60;
    if mins < 60 {
        return format!("{mins}m {rem_secs}s");
    }
    let hours = mins / 60;
    let rem_mins = mins % 60;
    format!("{hours}h {rem_mins}m")
}

fn tool_ls(db: &Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    #[allow(clippy::type_complexity)]
    type ChunkRow = (i64, Option<String>, String, String, String, i64, i64, f64);

    let file_filter = args["file_filter"].as_str().map(|s| s.to_string());
    let type_filter = args["type_filter"].as_str().map(|s| s.to_string());
    let language = args["language"].as_str().map(|s| s.to_string());
    let limit = args["limit"].as_u64().unwrap_or(50) as usize;

    let mut sql = String::from(
        "SELECT id, name, chunk_type, file_path, language, line_start, line_end, importance \
         FROM chunks WHERE is_deleted = 0",
    );
    let mut param_idx = 0u32;
    let mut param_vals: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ff) = &file_filter {
        param_idx += 1;
        sql.push_str(&format!(" AND file_path LIKE ?{param_idx}"));
        param_vals.push(Box::new(format!("%{ff}%")));
    }
    if let Some(tf) = &type_filter {
        param_idx += 1;
        sql.push_str(&format!(" AND chunk_type = ?{param_idx}"));
        param_vals.push(Box::new(tf.clone()));
    }
    if let Some(lang) = &language {
        param_idx += 1;
        sql.push_str(&format!(" AND language = ?{param_idx}"));
        param_vals.push(Box::new(lang.clone()));
    }

    sql.push_str(&format!(" ORDER BY importance DESC LIMIT {}", limit));

    let mut stmt = db.prepare(&sql)?;
    let rows: Vec<ChunkRow> = stmt
        .query_map(
            rusqlite::params_from_iter(param_vals.iter().map(|v| v.as_ref())),
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )?
        .collect::<Result<_, _>>()?;

    let mut text = String::new();
    for (id, name, chunk_type, file_path, _language, line_start, line_end, importance) in &rows {
        let name = name.as_deref().unwrap_or("(anonymous)");
        text.push_str(&format!(
            "[{}] {} `{}` {}:{}-{} (imp: {:.2})\n",
            id,
            chunk_type,
            name,
            file_path,
            line_start + 1,
            line_end + 1,
            importance,
        ));
    }

    if rows.is_empty() {
        text.push_str("No matching chunks found.\n");
    }

    Ok(vec![json!({ "type": "text", "text": text })])
}

fn tool_cat(db: &Connection, args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let id = args["id"].as_u64().ok_or("missing 'id' parameter")? as i64;

    let result = db.query_row(
        "SELECT name, chunk_type, file_path, language, line_start, line_end, content_raw, signature FROM chunks WHERE id = ?1 AND is_deleted = 0",
        params![id],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
            ))
        },
    );

    match result {
        Ok((name, chunk_type, file_path, language, line_start, line_end, content, signature)) => {
            let mut text = format!(
                "[{}] {} `{}` ({}) at {}:{}-{}\n",
                id,
                chunk_type,
                name.as_deref().unwrap_or("(anonymous)"),
                language,
                file_path,
                line_start + 1,
                line_end + 1,
            );
            if let Some(sig) = &signature {
                text.push_str(&format!("Signature: `{}`\n", sig));
            }
            if let Some(c) = &content {
                text.push_str(&format!("\n```\n{}\n```\n", c));
            }
            Ok(vec![json!({ "type": "text", "text": text })])
        }
        Err(_) => Ok(vec![json!({
            "type": "text",
            "text": format!("No chunk found with ID {}", id)
        })]),
    }
}

fn tool_health(db: &Connection) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let report = crate::maintain::run_health_check(db)?;
    let text = format!(
        "# Index Health\n\n\
         - **Integrity**: {}\n\
         - **Chunks**: {} live, {} tombstoned\n\
         - **Relationships**: {}\n\
         - **Embeddings**: {}\n\
         - **Entities**: {}\n\
         - **FTS entries**: {}\n\
         - **Vector entries**: {} (chunks), {} (hints)\n\
         - **Index size**: {} bytes\n\
         - **WAL size**: {} bytes\n",
        if report.integrity_ok {
            "OK"
        } else {
            report.integrity_error.as_deref().unwrap_or("FAILED")
        },
        report.live_chunks,
        report.tombstoned_chunks,
        report.total_relationships,
        report.total_embeddings,
        report.total_entities,
        report.fts_entries,
        report.vec_entries,
        report.hints_vec_entries,
        report.index_size_bytes,
        report.wal_size_bytes,
    );
    let total_orphans = report.orphan_hints
        + report.orphan_relationships
        + report.orphan_entity_attributes
        + report.orphan_embeddings
        + report.orphan_entity_deps;
    let mut text = text;
    if total_orphans > 0 {
        text.push_str(&format!(
            "\n**Orphans**: {} total (run `sqmd maintain clean-orphans` to fix)\n\
             - Hints: {}\n\
             - Relationships: {}\n\
             - Entity attributes: {}\n\
             - Embeddings: {}\n\
             - Entity deps: {}\n",
            total_orphans,
            report.orphan_hints,
            report.orphan_relationships,
            report.orphan_entity_attributes,
            report.orphan_embeddings,
            report.orphan_entity_deps,
        ));
    } else {
        text.push_str("\n**Orphans**: none\n");
    }
    Ok(vec![json!({ "type": "text", "text": text })])
}

fn tool_projects(args: &Value) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    let action = args["action"]
        .as_str()
        .ok_or("missing 'action' parameter")?;
    let reg = crate::multi_project::ProjectsRegistry::load();

    match action {
        "list" => {
            let list = reg.list();
            if list.is_empty() {
                Ok(vec![
                    json!({ "type": "text", "text": "No projects registered." }),
                ])
            } else {
                let mut text = String::from("Registered projects:\n\n");
                for (name, entry) in &list {
                    text.push_str(&format!("  {} -> {}\n", name, entry.path));
                }
                Ok(vec![json!({ "type": "text", "text": text })])
            }
        }
        "add" => {
            let name = args["name"].as_str().ok_or("missing 'name'")?;
            let path = args["path"].as_str().ok_or("missing 'path'")?;
            let mut reg = reg;
            reg.add(name.to_string(), path.to_string());
            reg.save()?;
            Ok(vec![
                json!({ "type": "text", "text": format!("Added project '{}'", name) }),
            ])
        }
        "remove" => {
            let name = args["name"].as_str().ok_or("missing 'name'")?;
            let mut reg = reg;
            reg.remove(name);
            reg.save()?;
            Ok(vec![
                json!({ "type": "text", "text": format!("Removed project '{}'", name) }),
            ])
        }
        "search" => {
            let query = args["query"].as_str().ok_or("missing 'query'")?;
            let top_k = args["top_k"].as_u64().unwrap_or(10) as usize;
            let project_spec = args["project"].as_str();
            let paths: Vec<std::path::PathBuf> = if let Some(spec) = project_spec {
                spec.split(',')
                    .filter_map(|p| reg.resolve_path(p.trim()))
                    .collect()
            } else {
                reg.list()
                    .iter()
                    .filter_map(|(_, entry)| {
                        let p = std::path::PathBuf::from(&entry.path);
                        if p.join(".sqmd").join("index.db").exists() {
                            Some(p)
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            if paths.is_empty() {
                return Ok(vec![
                    json!({ "type": "text", "text": "No projects to search." }),
                ]);
            }
            let results = crate::multi_project::multi_project_search(&paths, query, top_k)?;
            let mut text = String::new();
            for r in &results {
                text.push_str(&format!(
                    "[{}] {} ({}) {}-{} score={:.2}\n\n",
                    r.project,
                    r.file_path,
                    r.name.as_deref().unwrap_or("?"),
                    r.line_start + 1,
                    r.line_end + 1,
                    r.score,
                ));
                text.push_str(&r.markdown);
            }
            Ok(vec![json!({ "type": "text", "text": text })])
        }
        _ => Err(format!("Unknown projects action: {}", action).into()),
    }
}

use rusqlite::params;
