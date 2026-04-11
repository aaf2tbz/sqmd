use clap::{Parser, Subcommand};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Parser)]
#[command(
    name = "sqmd",
    version,
    about = "SQLite + Markdown code index for AI agents"
)]
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
        #[cfg(feature = "native")]
        /// Also generate embeddings
        #[arg(long)]
        embed: bool,
    },
    /// Search the index
    Search {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        top_k: usize,
        /// Filter by file path
        #[arg(long)]
        file: Option<String>,
        /// Filter by chunk type (function, class, method, etc.)
        #[arg(long)]
        r#type: Option<String>,
        /// Filter by source type (code, memory, transcript, document, entity)
        #[arg(long)]
        source: Option<String>,
        /// Keyword-only search (skip vector layers)
        #[arg(long)]
        keyword: bool,
    },
    #[cfg(feature = "native")]
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
    /// List entities in the knowledge graph
    Entities {
        /// Filter by entity type (file, struct, function, etc.)
        #[arg(long)]
        r#type: Option<String>,
        /// Max results
        #[arg(short = 'n', long, default_value = "50")]
        limit: usize,
    },
    /// Show entity dependencies
    EntityDeps {
        /// Entity name or file path
        name: String,
        /// Traversal depth
        #[arg(short = 'd', long, default_value = "1")]
        depth: usize,
    },
    /// Purge soft-deleted chunks older than N days
    Prune {
        /// Max age in days
        #[arg(default_value = "30")]
        days: i64,
    },
    /// Generate prospective search hints using native llama.cpp
    #[cfg(feature = "native")]
    Hints {
        /// Minimum chunk importance to generate hints for
        #[arg(long, default_value = "0.5")]
        min_importance: f64,
        /// Max chunks to process (0 = all eligible)
        #[arg(long, default_value = "0")]
        limit: usize,
    },
    /// Start MCP server (JSON-RPC over stdio)
    Mcp {
        /// Project root directory (defaults to ~/.sqmd if no local .sqmd found)
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Start sqmd daemon in background
    Start {
        /// Project root directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Stop running sqmd daemon
    Stop,
    /// Setup sqmd connections to AI harnesses (codex, claude-code, opencode)
    Setup {
        /// Harness to configure
        #[arg(value_enum)]
        harness: Option<String>,
    },
    /// Run diagnostic checks on sqmd installation
    Doctor {
        /// Check specific component (index, embed, model, mcp)
        #[arg(long)]
        check: Option<String>,
    },
    /// Update sqmd to the latest version
    Update {
        /// Force update even if already on latest
        #[arg(long)]
        force: bool,
    },
    /// Install or reinstall sqmd
    Install {
        /// Version to install (default: latest)
        version: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();
    let is_json = cli.json;
    let result = run(cli);
    if let Err(e) = result {
        if is_json {
            eprintln!(
                "{}",
                serde_json::json!({"ok": false, "error": e.to_string()})
            );
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Init => cmd_init(),
        #[cfg(feature = "native")]
        Commands::Index { path, embed } => {
            if embed {
                cmd_index_embed(&path)
            } else {
                cmd_index(&path)
            }
        }
        #[cfg(not(feature = "native"))]
        Commands::Index { path } => cmd_index(&path),
        Commands::Search {
            query,
            top_k,
            file,
            r#type,
            source,
            keyword,
        } => cmd_search(&query, top_k, file, r#type, source, keyword, cli.json),
        #[cfg(feature = "native")]
        Commands::Embed => cmd_embed(),
        Commands::Stats => cmd_stats(cli.json),
        Commands::Get { location } => cmd_get(&location, cli.json),
        Commands::Reset => cmd_reset(),
        Commands::Deps { path, depth } => cmd_deps(&path, depth),
        Commands::Context {
            query,
            files,
            max_tokens,
            deps,
            dep_depth,
        } => cmd_context(query, files, max_tokens, deps, dep_depth),
        Commands::Serve { path } => cmd_serve(&path),
        Commands::Watch { path } => cmd_watch(&path),
        Commands::Ls {
            file,
            r#type,
            depth,
        } => cmd_ls(file.as_deref(), r#type.as_deref(), depth, cli.json),
        Commands::Cat { id } => cmd_cat(id, cli.json),
        Commands::Diff { since } => cmd_diff(&since, cli.json),
        Commands::Entities { r#type, limit } => cmd_entities(r#type.as_deref(), limit, cli.json),
        Commands::EntityDeps { name, depth } => cmd_entity_deps(&name, depth),
        Commands::Prune { days } => cmd_prune(days),
        #[cfg(feature = "native")]
        Commands::Hints {
            min_importance,
            limit,
        } => cmd_hints(min_importance, limit),
        Commands::Mcp { path } => cmd_mcp(&path),
        Commands::Start { path } => cmd_start(&path),
        Commands::Stop => cmd_stop(),
        Commands::Setup { harness } => cmd_setup(harness.as_deref()),
        Commands::Doctor { check } => cmd_doctor(check.as_deref()),
        Commands::Update { force } => cmd_update(force),
        Commands::Install { version } => cmd_install(version.as_deref()),
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

fn cmd_mcp(given_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let local_index = given_path.join(".sqmd/index.db");
    let home_index = dirs::home_dir()
        .map(|h| h.join(".sqmd/index.db"))
        .filter(|p| p.exists());

    let path = if local_index.exists() {
        local_index
    } else if let Some(home) = home_index {
        home
    } else {
        eprintln!(
            "No index found at {}. Run `sqmd init` first.",
            local_index.display()
        );
        std::process::exit(1);
    };
    sqmd_core::mcp_server::run(&path)
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
    println!(
        "  {} total chunks, {} relationships",
        stats.chunks_total, stats.relationships_total
    );
    let d = &stats.decisions;
    println!(
        "  decisions: {} added, {} updated, {} skipped, {} tombstoned",
        d.added, d.updated, d.skipped, d.tombstoned
    );

    Ok(())
}

#[cfg(feature = "native")]
fn cmd_index_embed(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    cmd_index(root)?;
    let mut db = ensure_db()?;
    cmd_embed_with_db(&mut db)
}

#[cfg(feature = "native")]
fn cmd_embed() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = ensure_db()?;
    cmd_embed_with_db(&mut db)
}

#[cfg(feature = "native")]
fn cmd_embed_with_db(db: &mut rusqlite::Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut embedder = sqmd_core::embed::make_provider()?;

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
        let count = sqmd_core::search::embed_unembedded(db, &mut *embedder)?;
        if count == 0 {
            break;
        }
        total += count;
        let pct = (total as f64 / unembedded as f64) * 100.0;
        eprint!("\r  {}/{} ({:.0}%)", total, unembedded, pct);
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

#[allow(clippy::too_many_arguments)]
fn cmd_search(
    query: &str,
    top_k: usize,
    file_filter: Option<String>,
    type_filter: Option<String>,
    source_filter: Option<String>,
    keyword_only: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let search_query = sqmd_core::search::SearchQuery {
        text: query.to_string(),
        top_k,
        file_filter,
        type_filter,
        source_type_filter: source_filter.map(|s| vec![s]),
        ..Default::default()
    };

    let results = if keyword_only {
        sqmd_core::search::fts_search(&db, &search_query)?
    } else {
        #[cfg(feature = "native")]
        {
            let mut embedder = sqmd_core::embed::make_provider()?;
            sqmd_core::search::layered_search(&db, &search_query, Some(&mut *embedder))
                .map(|lr| lr.results)?
        }
        #[cfg(not(feature = "native"))]
        {
            sqmd_core::search::fts_search(&db, &search_query)?
        }
    };

    if results.is_empty() {
        if json {
            println!("{}", serde_json::json!({"query": query, "results": []}));
        } else {
            println!("No results for: {}", query);
        }
        return Ok(());
    }

    if json {
        let arr: Vec<serde_json::Value> = results
            .iter()
            .map(|r| {
                serde_json::json!({
                    "file_path": r.file_path,
                    "chunk_type": r.chunk_type,
                    "name": r.name,
                    "line_start": r.line_start + 1,
                    "line_end": r.line_end + 1,
                    "score": r.score,
                    "snippet": r.snippet,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "query": query,
                "result_count": results.len(),
                "results": arr,
            }))?
        );
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
        println!("   score: {:.3} ({})", r.score, score_tag);
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
    let embedded: i64 = db.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
    let db_size = std::fs::metadata(db_path())?.len();

    if json {
        println!(
            "{}",
            serde_json::json!({
                "files": files,
                "chunks": chunks,
                "relationships": rels,
                "embedded": embedded,
                "db_size_bytes": db_size,
            })
        );
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
            "SELECT chunk_type, COUNT(*) FROM chunks GROUP BY chunk_type ORDER BY COUNT(*) DESC",
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
    let result: Option<(i64, i64, String, String, String)> = db
        .query_row(
            "SELECT line_start, line_end, name, language, content_raw FROM chunks
         WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2
         LIMIT 1",
            rusqlite::params![file, line_num],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok();

    match result {
        Some((start, end, name, language, content)) => {
            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "name": name,
                        "file_path": file,
                        "language": language,
                        "line_start": start + 1,
                        "line_end": end + 1,
                        "content": content,
                    })
                );
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
            println!(
                "  -> {}:{} {} ({})",
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
                let placeholders: Vec<String> = (0..all_dep_ids.len())
                    .map(|i| format!("?{}", i + 1))
                    .collect();
                let sql = format!(
                    "SELECT DISTINCT file_path, name, line_start FROM chunks WHERE id IN ({})",
                    placeholders.join(", ")
                );
                let mut stmt = db.prepare(&sql)?;
                let params: Vec<&dyn rusqlite::ToSql> = all_dep_ids
                    .iter()
                    .map(|id| id as &dyn rusqlite::ToSql)
                    .collect();
                let transitive: Vec<(String, Option<String>, i64)> = stmt
                    .query_map(params.as_slice(), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                    .collect::<Result<_, _>>()?;
                if !transitive.is_empty() {
                    println!("\n  Transitive (depth {}):", depth);
                    for (fp, name, line) in &transitive {
                        println!(
                            "    -> {}:{} {}",
                            fp,
                            line + 1,
                            name.as_deref().unwrap_or("(unnamed)")
                        );
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
            println!(
                "  <- {}:{} {}",
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
        source_types: None,
    };

    let resp = sqmd_core::context::ContextAssembler::build(&db, &request)?;
    println!("{}", resp.markdown);
    eprintln!(
        "\n--- {} chunks, ~{} tokens ---",
        resp.chunk_count, resp.token_count
    );

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

fn cmd_ls(
    file: Option<&str>,
    type_filter: Option<&str>,
    depth: usize,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
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
                .query_row(
                    "SELECT content_raw FROM chunks WHERE id = ?1",
                    rusqlite::params![id],
                    |r| r.get(0),
                )
                .unwrap_or_default();

            if json {
                println!(
                    "{}",
                    serde_json::json!({
                        "id": e.id,
                        "file_path": e.file_path,
                        "language": e.language,
                        "chunk_type": e.chunk_type,
                        "name": e.name,
                        "signature": e.signature,
                        "line_start": e.line_start + 1,
                        "line_end": e.line_end + 1,
                        "content": content,
                    })
                );
            } else {
                println!(
                    "Chunk #{}: {} ({}:{})",
                    e.id,
                    e.name.as_deref().unwrap_or("(unnamed)"),
                    e.file_path,
                    e.line_start + 1
                );
                if let Some(sig) = &e.signature {
                    println!("Signature: {}", sig);
                }
                println!(
                    "Type: {} | Language: {} | Lines: {}-{}",
                    e.chunk_type,
                    e.language,
                    e.line_start + 1,
                    e.line_end + 1
                );
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

fn cmd_entities(
    entity_type: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;
    let ents = sqmd_core::entities::list_entities(&db, entity_type, limit)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&ents)?);
    } else {
        if ents.is_empty() {
            println!("No entities found.");
            return Ok(());
        }
        println!("Entities ({}):\n", ents.len());
        for e in &ents {
            let loc = match (e.file_path.as_deref(), e.line_start) {
                (Some(fp), Some(ls)) => format!(" ({fp}:{})", ls + 1),
                (Some(fp), _) => format!(" ({fp})"),
                _ => String::new(),
            };
            println!(
                "  {} [{}] mentions: {}{}",
                e.name, e.entity_type, e.mentions, loc
            );
        }
    }

    drop(db);
    Ok(())
}

fn cmd_entity_deps(name: &str, depth: usize) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let entity = sqmd_core::entities::get_entity(&db, name)?;
    match entity {
        Some(e) => {
            println!(
                "Entity: {} [{}] (mentions: {})\n",
                e.name, e.entity_type, e.mentions
            );

            let aspects = sqmd_core::entities::get_aspects(&db, e.id)?;
            if !aspects.is_empty() {
                println!("Aspects:");
                for a in &aspects {
                    println!("  {} (weight: {:.2})", a.name, a.weight);
                }
                println!();
            }

            if depth > 0 {
                let dep_ids = sqmd_core::entities::get_dependency_ids(&db, e.id, depth)?;
                if !dep_ids.is_empty() {
                    println!("Dependencies (depth {}):", depth);
                    let placeholders: Vec<String> = dep_ids
                        .iter()
                        .enumerate()
                        .map(|(i, _)| format!("?{}", i + 1))
                        .collect();
                    let sql = format!(
                        "SELECT id, name, entity_type FROM entities WHERE id IN ({})",
                        placeholders.join(", ")
                    );
                    let mut stmt = db.prepare(&sql)?;
                    let params: Vec<&dyn rusqlite::ToSql> = dep_ids
                        .iter()
                        .map(|id| id as &dyn rusqlite::ToSql)
                        .collect();
                    let deps: Vec<(i64, String, String)> = stmt
                        .query_map(params.as_slice(), |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                        .collect::<Result<_, _>>()?;
                    for (_id, dep_name, dep_type) in &deps {
                        println!("  -> {} [{}]", dep_name, dep_type);
                    }
                }
            }
        }
        None => println!("Entity '{}' not found.", name),
    }

    drop(db);
    Ok(())
}

fn cmd_prune(days: i64) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;
    let purged = sqmd_core::entities::purge_tombstones(&db, days)?;
    println!(
        "Purged {} tombstoned chunks older than {} days.",
        purged, days
    );
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

#[cfg(feature = "native")]
fn cmd_hints(min_importance: f64, limit: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut db = ensure_db()?;
    let effective_limit = if limit == 0 { 10000 } else { limit };
    eprintln!(
        "Generating prospective hints (min_importance={}, limit={})...",
        min_importance, effective_limit
    );
    let start = std::time::Instant::now();
    let count = sqmd_core::search::generate_hints_batch(&mut db, effective_limit, min_importance)?;
    let elapsed = start.elapsed();
    println!("Generated hints for {} chunks in {:?}", count, elapsed);
    drop(db);
    Ok(())
}

fn sqmd_home() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join(".sqmd")
}

fn cmd_start(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let root = path.canonicalize()?;
    let pid_path = sqmd_home().join("daemon.pid");

    if pid_path.exists() {
        let pid_str = std::fs::read_to_string(&pid_path)?;
        if let Ok(existing_pid) = pid_str.trim().parse::<u32>() {
            let alive = unsafe { libc::kill(existing_pid as i32, 0) == 0 };
            if alive {
                println!("Daemon already running (PID {})", existing_pid);
                return Ok(());
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    let db_path = root.join(".sqmd/index.db");
    if !db_path.exists() {
        eprintln!(
            "No index found at {}. Run `sqmd init` first.",
            root.display()
        );
        std::process::exit(1);
    }

    let exe = std::env::current_exe()?;
    let child = std::process::Command::new(&exe)
        .arg("serve")
        .arg(&root)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let pid = child.id();
    std::fs::create_dir_all(sqmd_home())?;
    std::fs::write(&pid_path, pid.to_string())?;
    println!("Daemon started (PID {}) for {}", pid, root.display());
    Ok(())
}

fn cmd_stop() -> Result<(), Box<dyn std::error::Error>> {
    let pid_path = sqmd_home().join("daemon.pid");
    if !pid_path.exists() {
        println!("No daemon running.");
        return Ok(());
    }
    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: u32 = pid_str.trim().parse()?;
    unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
    }
    std::fs::remove_file(&pid_path)?;
    println!("Daemon stopped (PID {})", pid);
    Ok(())
}

fn cmd_setup(harness: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let harness = harness.unwrap_or("all");
    let harnesses = if harness == "all" {
        vec!["opencode", "codex", "claude", "cursor"]
    } else {
        vec![harness]
    };

    for h in &harnesses {
        match *h {
            "opencode" => setup_opencode()?,
            "codex" => setup_codex()?,
            "claude" => setup_claude()?,
            "cursor" => setup_cursor()?,
            _ => {
                eprintln!(
                    "Unknown harness: {}. Available: opencode, codex, claude, cursor",
                    h
                );
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn setup_opencode() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = dirs::config_dir()
        .ok_or("Cannot find config directory")?
        .join("opencode")
        .join("opencode.json");

    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        std::fs::create_dir_all(config_path.parent().unwrap())?;
        serde_json::json!({})
    };

    let obj = config.as_object_mut().ok_or("config is not an object")?;
    if !obj.contains_key("mcp") {
        obj.insert("mcp".to_string(), serde_json::json!({}));
    }
    obj.get_mut("mcp")
        .unwrap()
        .as_object_mut()
        .ok_or("mcp is not an object")?
        .insert(
            "sqmd".to_string(),
            serde_json::json!({
                "type": "local",
                "command": ["sqmd", "mcp"],
                "enabled": true
            }),
        );

    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    println!("{}: registered sqmd MCP server", config_path.display());
    Ok(())
}

fn setup_codex() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_path = PathBuf::from(home).join(".codex").join("config.toml");

    let mut content = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "sqmd".to_string());

    let section = format!(
        "[mcp_servers.sqmd]\ncommand = \"{}\"\nargs = [\"mcp\"]\n",
        exe.replace('"', "\\\"")
    );

    if content.contains("[mcp_servers.sqmd]") {
        let lines: Vec<&str> = content.lines().collect();
        let mut skip_start = None;
        let mut skip_end = lines.len();
        for (i, line) in lines.iter().enumerate() {
            if *line == "[mcp_servers.sqmd]" {
                skip_start = Some(i);
                continue;
            }
            if skip_start.is_some() && (line.starts_with('[') || i == lines.len() - 1) {
                skip_end = if line.starts_with('[') { i } else { i + 1 };
                break;
            }
        }
        if let Some(start) = skip_start {
            let before: String = lines[..start].join("\n");
            let after: String = if skip_end < lines.len() {
                lines[skip_end..].join("\n")
            } else {
                String::new()
            };
            content = format!(
                "{}\n{}\n{}",
                before.trim_end(),
                section.trim_end(),
                after.trim_start()
            );
        }
    } else {
        if !content.is_empty() && !content.ends_with('\n') {
            content.push('\n');
        }
        content.push_str(&section);
    }

    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&config_path, content)?;
    println!("{}: registered sqmd MCP server", config_path.display());
    Ok(())
}

fn setup_claude() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").unwrap_or_default();
    let config_path = PathBuf::from(home).join(".claude").join("settings.json");

    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        std::fs::create_dir_all(config_path.parent().unwrap())?;
        serde_json::json!({})
    };

    let obj = config.as_object_mut().ok_or("config is not an object")?;
    if !obj.contains_key("mcpServers") {
        obj.insert("mcpServers".to_string(), serde_json::json!({}));
    }
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "sqmd".to_string());

    obj.get_mut("mcpServers")
        .unwrap()
        .as_object_mut()
        .ok_or("mcpServers is not an object")?
        .insert(
            "sqmd".to_string(),
            serde_json::json!({
                "command": [exe, "mcp"]
            }),
        );

    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    println!("{}: registered sqmd MCP server", config_path.display());
    Ok(())
}

fn setup_cursor() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let config_dir = cwd.join(".cursor");
    let config_path = config_dir.join("mcp.json");

    let mut config: serde_json::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        std::fs::create_dir_all(&config_dir)?;
        serde_json::json!({})
    };

    let obj = config.as_object_mut().ok_or("config is not an object")?;
    if !obj.contains_key("mcpServers") {
        obj.insert("mcpServers".to_string(), serde_json::json!({}));
    }
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "sqmd".to_string());

    obj.get_mut("mcpServers")
        .unwrap()
        .as_object_mut()
        .ok_or("mcpServers is not an object")?
        .insert(
            "sqmd".to_string(),
            serde_json::json!({
                "command": [exe, "mcp"]
            }),
        );

    std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;
    println!("{}: registered sqmd MCP server", config_path.display());
    Ok(())
}

fn cmd_doctor(check: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    println!("sqmd diagnostics\n");
    let check = check.unwrap_or("all");
    let checks = if check == "all" {
        vec!["index", "embed", "model", "mcp", "daemon"]
    } else {
        vec![check]
    };

    let mut all_ok = true;

    for c in &checks {
        match *c {
            "index" => {
                print!("  index:      ");
                let db_location = db_path();
                if !db_location.exists() {
                    println!("MISSING (run `sqmd init`)");
                    all_ok = false;
                } else {
                    let db = sqmd_core::schema::open_fast(&db_location)?;
                    let chunks: i64 = db.query_row(
                        "SELECT COUNT(*) FROM chunks WHERE is_deleted = 0",
                        [],
                        |r| r.get(0),
                    )?;
                    let rels: i64 =
                        db.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))?;
                    let embedded: i64 =
                        db.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))?;
                    drop(db);
                    println!(
                        "OK ({} chunks, {} embedded, {} rels)",
                        chunks, embedded, rels
                    );
                }
            }
            "embed" => {
                #[cfg(feature = "native")]
                {
                    print!("  embedder:   ");
                    match sqmd_core::native::NativeRuntime::new() {
                        Ok(_) => println!("OK (mxbai-embed-large, Metal GPU)"),
                        Err(e) => {
                            println!("FAIL ({})", e);
                            all_ok = false;
                        }
                    }
                }
                #[cfg(not(feature = "native"))]
                {
                    println!("  embedder:   SKIP (native feature not enabled)");
                }
            }
            "model" => {
                #[cfg(feature = "native")]
                {
                    print!("  model:      ");
                    let model_env = std::env::var("SQMD_NATIVE_MODEL")
                        .unwrap_or_else(|_| "mxbai-embed-large".to_string());
                    let ollama_home = std::env::var("OLLAMA_MODELS").unwrap_or_else(|_| {
                        format!(
                            "{}/.ollama/models",
                            std::env::var("HOME").unwrap_or_default()
                        )
                    });
                    let manifest_path = format!(
                        "{}/manifests/registry.ollama.ai/library/{}/latest",
                        ollama_home,
                        model_env.split(':').next().unwrap_or(&model_env)
                    );
                    if std::path::Path::new(&manifest_path).exists() {
                        println!("OK ({})", model_env);
                    } else {
                        println!(
                            "WARNING (manifest not found for {}, run `ollama pull {}`)",
                            model_env, model_env
                        );
                        all_ok = false;
                    }
                }
            }
            "mcp" => {
                print!("  mcp server: ");
                let exe = std::env::current_exe()?;
                match std::process::Command::new(&exe).arg("--version").output() {
                    Ok(o) if o.status.success() => println!("OK"),
                    _ => {
                        println!("FAIL");
                        all_ok = false;
                    }
                }
            }
            "daemon" => {
                print!("  daemon:     ");
                let pid_path = sqmd_home().join("daemon.pid");
                if pid_path.exists() {
                    let pid_str = std::fs::read_to_string(&pid_path)?;
                    match pid_str.trim().parse::<u32>() {
                        Ok(pid) => {
                            let alive = unsafe { libc::kill(pid as i32, 0) == 0 };
                            if alive {
                                println!("running (PID {})", pid);
                            } else {
                                println!("stale PID file (cleaning up)");
                                let _ = std::fs::remove_file(&pid_path);
                                all_ok = false;
                            }
                        }
                        Err(_) => {
                            println!("corrupt PID file");
                            all_ok = false;
                        }
                    }
                } else {
                    println!("stopped");
                }
            }
            _ => {
                println!("  unknown check: {}", c);
            }
        }
    }

    println!();
    if all_ok {
        println!("All checks passed.");
    } else {
        println!("Some checks failed. See above for details.");
    }
    Ok(())
}

fn cmd_update(force: bool) -> Result<(), Box<dyn std::error::Error>> {
    let current = std::env!("CARGO_PKG_VERSION");
    println!("sqmd v{}", current);

    if !force {
        println!("Updating...");
    }

    let repo =
        std::env::var("SQMD_REPO").unwrap_or_else(|_| "/Users/alexmondello/repos/sqmd".to_string());

    println!("Building from source at {}...", repo);
    let status = std::process::Command::new("cargo")
        .args(["build", "--release", "--features", "native"])
        .current_dir(&repo)
        .status()?;
    if status.success() {
        let bin = std::path::Path::new(&repo).join("target/release/sqmd");
        println!("Built: {}", bin.display());
        let home = std::env::var("HOME").unwrap_or_default();
        let dest = PathBuf::from(home).join(".cargo").join("bin").join("sqmd");
        if let Err(e) = std::fs::copy(&bin, &dest) {
            eprintln!("Warning: could not copy to {}: {}", dest.display(), e);
            println!("Manually copy: cp {} /usr/local/bin/sqmd", bin.display());
        } else {
            println!("Installed to {}", dest.display());
        }
    } else {
        eprintln!("Build failed.");
    }
    Ok(())
}

fn cmd_install(version: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let version = version.unwrap_or("latest");
    println!("Installing sqmd {}...", version);

    let repo =
        std::env::var("SQMD_REPO").unwrap_or_else(|_| "/Users/alexmondello/repos/sqmd".to_string());
    let repo_path = std::path::Path::new(&repo);

    if repo_path.exists() {
        println!("Found source at {}", repo);
        let status = std::process::Command::new("cargo")
            .args(["build", "--release", "--features", "native"])
            .current_dir(repo_path)
            .status()?;
        if !status.success() {
            eprintln!("Build failed.");
            std::process::exit(1);
        }
        let bin = repo_path.join("target/release/sqmd");
        if bin.exists() {
            println!("Built: {}", bin.display());
            println!("Install: cp {} /usr/local/bin/sqmd", bin.display());
        }
    } else {
        println!("Source not found at {}. Cloning...", repo);
        let status = std::process::Command::new("git")
            .args(["clone", "https://github.com/aaf2tbz/sqmd.git", &repo])
            .status()?;
        if !status.success() {
            eprintln!("Clone failed.");
            std::process::exit(1);
        }
        println!("Cloned. Run `sqmd install` again to build.");
    }
    Ok(())
}
