use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::io::Write;

#[derive(Parser)]
#[command(name = "sqmd", version, about = "SQLite + Markdown code index for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
    /// Output results as JSON
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new index in the current project
    Init,
    /// Index the project (default: current directory)
    Index {
        /// Project root directory
        #[arg(default_value = ".")]
        path: PathBuf,
        #[cfg(feature = "embed")]
        /// Also generate embeddings
        #[arg(long)]
        embed: bool,
    },
    /// Search the index (hybrid: FTS5 + vector by default)
    Search {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        top_k: usize,
        #[cfg(feature = "embed")]
        /// Vector search weight (0.0 = keyword only, 1.0 = vector only)
        #[arg(long, default_value = "0.7")]
        alpha: f64,
        /// Filter by file path
        #[arg(long)]
        file: Option<String>,
        /// Filter by chunk type (function, class, method, etc.)
        #[arg(long)]
        r#type: Option<String>,
        /// Keyword-only search (skip vector)
        #[cfg(feature = "embed")]
        #[arg(long)]
        keyword: bool,
    },
    #[cfg(feature = "embed")]
    /// Generate embeddings for unembedded chunks
    Embed,
    /// Show index statistics
    Stats,
    /// Get chunk at file:line
    Get {
        /// File path and line (e.g., src/main.rs:42)
        location: String,
    },
    /// Reset the index
    Reset,
    /// Show dependencies for a file
    Deps {
        /// File path
        path: String,
        /// Traversal depth (1 = direct only)
        #[arg(short, long, default_value = "1")]
        depth: usize,
    },
    /// Assemble context for an AI agent
    Context {
        /// Search query
        #[arg(long)]
        query: Option<String>,
        /// Files to include
        #[arg(long, num_args = 0..=100)]
        files: Vec<String>,
        /// Maximum tokens in output
        #[arg(short = 't', long, default_value = "8000")]
        max_tokens: usize,
        /// Include dependency graph
        #[arg(long, default_value = "true")]
        deps: bool,
        /// Dependency depth
        #[arg(long, default_value = "1")]
        dep_depth: usize,
    },
    /// Start the daemon server (Unix socket)
    Serve {
        /// Project root directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Watch for file changes and re-index incrementally
    Watch {
        /// Project root directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// List chunks (file-system view)
    Ls {
        /// Filter by file path
        #[arg(long)]
        file: Option<String>,
        /// Filter by chunk type
        #[arg(long)]
        r#type: Option<String>,
        /// Max depth for contains tree
        #[arg(short = 'd', long, default_value = "1")]
        depth: usize,
    },
    /// Get chunk by ID
    Cat {
        /// Chunk ID
        id: i64,
    },
    /// Show chunks modified since timestamp
    Diff {
        /// ISO timestamp (e.g., "2025-01-01T00:00:00")
        since: String,
    },
}

fn main() {
    let cli = Cli::parse();
    let is_json = cli.json;
    let result = run(cli);
    if let Err(e) = result {
        if is_json {
            eprintln!("{}", serde_json::json!({"ok": false, "error": e.to_string()}));
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Init => cmd_init(),
        #[cfg(feature = "embed")]
        Commands::Index { path, embed } => {
            if embed {
                cmd_index_embed(&path)
            } else {
                cmd_index(&path)
            }
        }
        #[cfg(not(feature = "embed"))]
        Commands::Index { path } => cmd_index(&path),
        #[cfg(feature = "embed")]
        Commands::Search { query, top_k, alpha, file, r#type, keyword } => {
            cmd_search(&query, top_k, Some(alpha), file, r#type, Some(keyword), cli.json)
        }
        #[cfg(not(feature = "embed"))]
        Commands::Search { query, top_k, file, r#type } => {
            cmd_search(&query, top_k, None, file, r#type, None, cli.json)
        }
        #[cfg(feature = "embed")]
        Commands::Embed => cmd_embed(),
        Commands::Stats => cmd_stats(cli.json),
        Commands::Get { location } => cmd_get(&location, cli.json),
        Commands::Reset => cmd_reset(),
        Commands::Deps { path, depth } => cmd_deps(&path, depth),
        Commands::Context { query, files, max_tokens, deps, dep_depth } => {
            cmd_context(query, files, max_tokens, deps, dep_depth)
        }
        Commands::Serve { path } => cmd_serve(&path),
        Commands::Watch { path } => cmd_watch(&path),
        Commands::Ls { file, r#type, depth } => cmd_ls(file.as_deref(), r#type.as_deref(), depth, cli.json),
        Commands::Cat { id } => cmd_cat(id, cli.json),
        Commands::Diff { since } => cmd_diff(&since, cli.json),
    }
}

fn find_project_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        if dir.join(".sqmd").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn db_path() -> PathBuf {
    find_project_root()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
        .join(".sqmd/index.db")
}

fn ensure_db() -> Result<rusqlite::Connection, Box<dyn std::error::Error>> {
    let path = db_path();
    if !path.exists() {
        eprintln!("No index found. Run `sqmd init` first.");
        std::process::exit(1);
    }
    let db = sqmd_core::schema::open(&path)?;
    Ok(db)
}

fn cmd_init() -> Result<(), Box<dyn std::error::Error>> {
    let path = db_path();
    if path.exists() {
        eprintln!("Index already exists at {}", path.display());
        return Ok(());
    }
    std::fs::create_dir_all(path.parent().unwrap())?;
    let db = sqmd_core::schema::open(&path)?;
    println!("Initialized index at {}", path.display());

    let gitignore = PathBuf::from(".gitignore");
    if gitignore.exists() {
        let content = std::fs::read_to_string(&gitignore)?;
        if !content.contains(".sqmd/") {
            std::fs::OpenOptions::new()
                .append(true)
                .open(&gitignore)?
                .write_all(b"\n.sqmd/\n")?;
            println!("Added .sqmd/ to .gitignore");
        }
    } else {
        std::fs::write(&gitignore, ".sqmd/\n")?;
        println!("Created .gitignore with .sqmd/");
    }

    drop(db);
    Ok(())
}

fn cmd_index(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let root = root.canonicalize()?;
    let path = db_path();
    let mut db = if path.exists() {
        sqmd_core::schema::open(&path)?
    } else {
        std::fs::create_dir_all(path.parent().unwrap())?;
        sqmd_core::schema::open(&path)?
    };

    let start = std::time::Instant::now();
    let mut indexer = sqmd_core::index::Indexer::new(&mut db, &root);
    let stats = indexer.index()?;
    let elapsed = start.elapsed();

    println!("Indexed in {:?}", elapsed);
    println!(
        "  {} files scanned, {} indexed, {} skipped, {} deleted",
        stats.files_scanned, stats.files_indexed, stats.files_skipped, stats.files_deleted
    );
    println!("  {} total chunks, {} relationships", stats.chunks_total, stats.relationships_total);

    Ok(())
}

#[cfg(feature = "embed")]
fn cmd_index_embed(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    cmd_index(root)?;
    let mut db = ensure_db()?;
    cmd_embed_with_db(&mut db)
}

#[cfg(feature = "embed")]
fn cmd_embed() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = ensure_db()?;
    cmd_embed_with_db(&mut db)
}

#[cfg(feature = "embed")]
fn cmd_embed_with_db(db: &mut rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut embedder = sqmd_core::embed::Embedder::new()?;

    let unembedded: i64 = db.query_row(
        "SELECT COUNT(*) FROM chunks LEFT JOIN embeddings ON chunks.id = embeddings.chunk_id WHERE embeddings.chunk_id IS NULL",
        [],
        |r| r.get(0),
    )?;

    if unembedded == 0 {
        println!("All chunks already embedded.");
        return Ok(());
    }

    println!("Embedding {} chunks...", unembedded);
    let start = std::time::Instant::now();
    let mut total = 0;

    loop {
        let count = sqmd_core::search::embed_unembedded(db, &mut embedder)?;
        if count == 0 {
            break;
        }
        total += count;
        print!("\r  {} / {}", total, unembedded);
        std::io::stderr().flush().ok();
    }

    let elapsed = start.elapsed();
    println!();
    println!("Embedded {} chunks in {:?}", total, elapsed);
    if total > 0 {
        println!("  {:.0} chunks/sec", total as f64 / elapsed.as_secs_f64());
    }

    Ok(())
}

fn cmd_search(
    query: &str,
    top_k: usize,
    alpha: Option<f64>,
    file_filter: Option<String>,
    type_filter: Option<String>,
    #[cfg_attr(not(feature = "embed"), allow(unused_variables))]
    keyword_only: Option<bool>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let search_query = sqmd_core::search::SearchQuery {
        text: query.to_string(),
        top_k,
        alpha: alpha.unwrap_or(0.7),
        file_filter,
        type_filter,
        ..Default::default()
    };

    #[cfg(feature = "embed")]
    let results = if keyword_only.unwrap_or(false) {
        sqmd_core::search::fts_search(&db, &search_query)?
    } else {
        let mut embedder = sqmd_core::embed::Embedder::new()?;
        sqmd_core::search::hybrid_search(&db, &search_query, &mut embedder)?
    };

    #[cfg(not(feature = "embed"))]
    let results = sqmd_core::search::fts_search(&db, &search_query)?;

    if results.is_empty() {
        if json {
            println!("{}", serde_json::json!({"query": query, "results": []}));
        } else {
            println!("No results for: {}", query);
        }
        return Ok(());
    }

    if json {
        let arr: Vec<serde_json::Value> = results.iter().map(|r| {
            serde_json::json!({
                "file_path": r.file_path,
                "chunk_type": r.chunk_type,
                "name": r.name,
                "line_start": r.line_start + 1,
                "line_end": r.line_end + 1,
                "score": r.score,
                "snippet": r.snippet,
            })
        }).collect();
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "query": query,
            "result_count": results.len(),
            "results": arr,
        }))?);
        drop(db);
        return Ok(());
    }

    println!("Found {} results for \"{}\":\n", results.len(), query);
    for (i, r) in results.iter().enumerate() {
        let score_tag = if r.vec_distance.is_some() && r.fts_rank.is_some() {
            "hybrid"
        } else if r.vec_distance.is_some() {
            "vector"
        } else {
            "keyword"
        };

        println!(
            "{}. [{}] {}:{}-{} {}",
            i + 1,
            r.chunk_type,
            r.file_path,
            r.line_start + 1,
            r.line_end + 1,
            r.name.as_deref().unwrap_or(""),
        );
        println!(
            "   score: {:.3} ({})",
            r.score, score_tag
        );
        if let Some(snippet) = &r.snippet {
            println!("   {}", snippet);
        }
        println!();
    }

    drop(db);
    Ok(())
}

fn cmd_stats(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let files: i64 = db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let chunks: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let rels: i64 = db.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))?;
    let embedded: i64 = db.query_row(
        "SELECT COUNT(*) FROM embeddings",
        [],
        |r| r.get(0),
    )?;
    let db_size = std::fs::metadata(db_path())?.len();

    if json {
        println!("{}", serde_json::json!({
            "files": files,
            "chunks": chunks,
            "relationships": rels,
            "embedded": embedded,
            "db_size_bytes": db_size,
        }));
        drop(db);
        return Ok(());
    }

    let langs: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT language, COUNT(*) FROM chunks GROUP BY language ORDER BY COUNT(*) DESC LIMIT 10"
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let types: Vec<(String, i64)> = {
        let mut stmt = db.prepare(
            "SELECT chunk_type, COUNT(*) FROM chunks GROUP BY chunk_type ORDER BY COUNT(*) DESC"
        )?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?
    };

    println!("sqmd index statistics");
    println!("=====================");
    println!("Files indexed:  {}", files);
    println!("Total chunks:   {}", chunks);
    println!("Embedded:       {} / {}", embedded, chunks);
    println!("Relationships:  {}", rels);
    println!("DB size:        {} KB", db_size / 1024);
    println!();
    println!("By language:");
    for (lang, count) in &langs {
        println!("  {:<15} {}", lang, count);
    }
    println!();
    println!("By chunk type:");
    for (ct, count) in &types {
        println!("  {:<15} {}", ct, count);
    }

    drop(db);
    Ok(())
}

fn cmd_get(location: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let (file, line) = location
        .rsplit_once(':')
        .ok_or("Invalid format. Use file:line")?;
    let line_num: i64 = line.parse()?;

    let db = ensure_db()?;
    let result: Option<(i64, i64, String, String, String)> = db.query_row(
        "SELECT line_start, line_end, name, language, content_raw FROM chunks
         WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2
         LIMIT 1",
        rusqlite::params![file, line_num],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
    ).ok();

    match result {
        Some((start, end, name, language, content)) => {
            if json {
                println!("{}", serde_json::json!({
                    "name": name,
                    "file_path": file,
                    "language": language,
                    "line_start": start + 1,
                    "line_end": end + 1,
                    "content": content,
                }));
            } else {
                println!("Chunk: {} (lines {}-{})", name, start + 1, end + 1);
                println!("```{}", language);
                println!("{}", content);
                println!("```");
            }
        }
        None => {
            println!("No chunk found at {}:{}", file, line);
        }
    }

    drop(db);
    Ok(())
}

fn cmd_deps(file_path: &str, depth: usize) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let imports = sqmd_core::relationships::get_dependencies(&db, file_path)?;
    let dependents = sqmd_core::relationships::get_dependents(&db, file_path)?;

    if imports.is_empty() && dependents.is_empty() {
        println!("No relationships found for {}", file_path);
        return Ok(());
    }

    if !imports.is_empty() {
        println!("Dependencies of {} (depth {}):\n", file_path, depth);
        let mut seen = std::collections::HashSet::new();
        for dep in &imports {
            let key = format!("{}:{}", dep.target_file, dep.target_line);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            println!("  -> {}:{} {} ({})",
                dep.target_file,
                dep.target_line + 1,
                dep.target_name.as_deref().unwrap_or("(unnamed)"),
                dep.rel_type,
            );
        }

        if depth > 1 {
            let chunk_ids: Vec<i64> = imports.iter().map(|d| d.source_chunk_id).collect();
            let mut all_dep_ids = Vec::new();
            for &cid in &chunk_ids {
                if let Ok(ids) = sqmd_core::relationships::get_dependency_ids(&db, cid, depth) {
                    all_dep_ids.extend(ids);
                }
            }
            if !all_dep_ids.is_empty() {
                let placeholders: Vec<String> = (0..all_dep_ids.len()).map(|i| format!("?{}", i + 1)).collect();
                let sql = format!(
                    "SELECT DISTINCT file_path, name, line_start FROM chunks WHERE id IN ({})",
                    placeholders.join(", ")
                );
                let mut stmt = db.prepare(&sql)?;
                let params: Vec<&dyn rusqlite::ToSql> = all_dep_ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
                let transitive: Vec<(String, Option<String>, i64)> = stmt
                    .query_map(params.as_slice(), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<Result<_, _>>()?;
                if !transitive.is_empty() {
                    println!("\n  Transitive (depth {}):", depth);
                    for (fp, name, line) in &transitive {
                        println!("    -> {}:{} {}", fp, line + 1, name.as_deref().unwrap_or("(unnamed)"));
                    }
                }
            }
        }
        println!();
    }

    if !dependents.is_empty() {
        println!("Dependents of {} (who imports this):\n", file_path);
        let mut seen = std::collections::HashSet::new();
        for dep in &dependents {
            let key = format!("{}:{}", dep.source_file, dep.source_line);
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            println!("  <- {}:{} {}",
                dep.source_file,
                dep.source_line + 1,
                dep.source_name.as_deref().unwrap_or("(unnamed)"),
            );
        }
    }

    drop(db);
    Ok(())
}

fn cmd_context(
    query: Option<String>,
    files: Vec<String>,
    max_tokens: usize,
    include_deps: bool,
    dep_depth: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let request = sqmd_core::context::ContextRequest {
        query: query.unwrap_or_default(),
        files,
        max_tokens,
        include_deps,
        dep_depth,
        top_k: 10,
    };

    let resp = sqmd_core::context::ContextAssembler::build(&db, &request)?;
    println!("{}", resp.markdown);
    eprintln!("\n--- {} chunks, ~{} tokens ---", resp.chunk_count, resp.token_count);

    drop(db);
    Ok(())
}

fn cmd_serve(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let root = root.canonicalize()?;
    sqmd_core::daemon::serve(&root)
}

fn cmd_reset() -> Result<(), Box<dyn std::error::Error>> {
    let path = db_path();
    if path.exists() {
        std::fs::remove_file(&path)?;
        println!("Removed index at {}", path.display());
    }
    let wal = path.with_extension("db-wal");
    let shm = path.with_extension("db-shm");
    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(&shm);
    println!("Index reset. Run `sqmd index` to rebuild.");
    Ok(())
}

fn cmd_watch(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let root = root.canonicalize()?;
    sqmd_core::watcher::watch(&root)
}

fn cmd_ls(file: Option<&str>, type_filter: Option<&str>, depth: usize, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;
    let entries = sqmd_core::vfs::list_chunks(&db, file, type_filter, depth)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        if entries.is_empty() {
            println!("No chunks found.");
            return Ok(());
        }
        let tree = sqmd_core::vfs::render_tree(&entries, 0);
        print!("{}", tree);
        println!("\n{} chunks", entries.len());
    }

    drop(db);
    Ok(())
}

fn cmd_cat(id: i64, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;
    let entry = sqmd_core::vfs::get_chunk_by_id(&db, id)?;

    match entry {
        Some(e) => {
            let content: String = db
                .query_row("SELECT content_raw FROM chunks WHERE id = ?1", rusqlite::params![id], |r| r.get(0))
                .unwrap_or_default();

            if json {
                println!("{}", serde_json::json!({
                    "id": e.id,
                    "file_path": e.file_path,
                    "language": e.language,
                    "chunk_type": e.chunk_type,
                    "name": e.name,
                    "signature": e.signature,
                    "line_start": e.line_start + 1,
                    "line_end": e.line_end + 1,
                    "content": content,
                }));
            } else {
                println!("Chunk #{}: {} ({}:{})", e.id, e.name.as_deref().unwrap_or("(unnamed)"), e.file_path, e.line_start + 1);
                if let Some(sig) = &e.signature {
                    println!("Signature: {}", sig);
                }
                println!("Type: {} | Language: {} | Lines: {}-{}", e.chunk_type, e.language, e.line_start + 1, e.line_end + 1);
                println!();
                println!("```{}", e.language);
                println!("{}", content);
                println!("```");
            }
        }
        None => {
            println!("No chunk found with id {}", id);
        }
    }

    drop(db);
    Ok(())
}

fn cmd_diff(since: &str, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;
    let diffs = sqmd_core::vfs::diff_snapshots(&db, since)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&diffs)?);
    } else {
        if diffs.is_empty() {
            println!("No changes since {}", since);
            return Ok(());
        }
        println!("Changes since {} ({} chunks):\n", since, diffs.len());
        for d in &diffs {
            let name = d.name.as_deref().unwrap_or("(unnamed)");
            println!("{} {} [{}] {}", d.change, name, d.chunk_type, d.file_path);
            if let Some(content) = &d.new_content {
                for line in content.lines().take(5) {
                    println!("  {}", line);
                }
            }
            println!();
        }
    }

    drop(db);
    Ok(())
}
