use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::io::Write;

#[derive(Parser)]
#[command(name = "sqmd", version, about = "SQLite + Markdown code index for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
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
        #[arg(long)]
        keyword: bool,
    },
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
    },
    /// Watch for file changes and re-index incrementally
    Watch {
        /// Project root directory
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() {
    let cli = Cli::parse();
    let result = run(cli);
    if let Err(e) = result {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Init => cmd_init(),
        Commands::Index { path, embed } => cmd_index(&path, embed),
        Commands::Search { query, top_k, alpha, file, r#type, keyword } => {
            cmd_search(&query, top_k, alpha, file, r#type, keyword)
        }
        Commands::Embed => cmd_embed(),
        Commands::Stats => cmd_stats(),
        Commands::Get { location } => cmd_get(&location),
        Commands::Reset => cmd_reset(),
        Commands::Deps { path } => cmd_deps(&path),
        Commands::Watch { path } => cmd_watch(&path),
    }
}

fn db_path() -> PathBuf {
    PathBuf::from(".sqmd/index.db")
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

fn cmd_index(root: &Path, do_embed: bool) -> Result<(), Box<dyn std::error::Error>> {
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

    if do_embed {
        println!();
        cmd_embed_with_db(&mut db)?;
    }

    Ok(())
}

fn cmd_embed() -> Result<(), Box<dyn std::error::Error>> {
    let mut db = ensure_db()?;
    cmd_embed_with_db(&mut db)
}

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
    alpha: f64,
    file_filter: Option<String>,
    type_filter: Option<String>,
    keyword_only: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let search_query = sqmd_core::search::SearchQuery {
        text: query.to_string(),
        top_k,
        alpha,
        file_filter,
        type_filter,
        ..Default::default()
    };

    let results = if keyword_only {
        sqmd_core::search::fts_search(&db, &search_query)?
    } else {
        let mut embedder = sqmd_core::embed::Embedder::new()?;
        sqmd_core::search::hybrid_search(&db, &search_query, &mut embedder)?
    };

    if results.is_empty() {
        println!("No results for: {}", query);
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

fn cmd_stats() -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let files: i64 = db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let chunks: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
    let rels: i64 = db.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))?;
    let embedded: i64 = db.query_row(
        "SELECT COUNT(*) FROM embeddings",
        [],
        |r| r.get(0),
    )?;
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
    let db_size = std::fs::metadata(db_path())?.len();

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

fn cmd_get(location: &str) -> Result<(), Box<dyn std::error::Error>> {
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
            println!("Chunk: {} (lines {}-{})", name, start + 1, end + 1);
            println!("```{}", language);
            println!("{}", content);
            println!("```");
        }
        None => {
            println!("No chunk found at {}:{}", file, line);
        }
    }

    drop(db);
    Ok(())
}

fn cmd_deps(file_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let imports = sqmd_core::relationships::get_dependencies(&db, file_path)?;
    let dependents = sqmd_core::relationships::get_dependents(&db, file_path)?;

    if imports.is_empty() && dependents.is_empty() {
        println!("No relationships found for {}", file_path);
        return Ok(());
    }

    if !imports.is_empty() {
        println!("Dependencies of {} (imports):\n", file_path);
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
