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
    },
    /// Search the index
    Search {
        /// Search query
        query: String,
        /// Maximum results
        #[arg(short, long, default_value = "10")]
        top_k: usize,
    },
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
        Commands::Index { path } => cmd_index(&path),
        Commands::Search { query, top_k } => cmd_search(&query, top_k),
        Commands::Stats => cmd_stats(),
        Commands::Get { location } => cmd_get(&location),
        Commands::Reset => cmd_reset(),
        Commands::Deps { path } => cmd_deps(&path),
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
    println!("  {} total chunks", stats.chunks_total);

    Ok(())
}

fn cmd_search(query: &str, top_k: usize) -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;
    let mut stmt = db.prepare(
        "SELECT c.file_path, c.name, c.line_start, c.line_end, c.chunk_type, snippet(chunks_fts, 0, '>>>', '<<<', '...', 24)
         FROM chunks_fts f JOIN chunks c ON f.rowid = c.id
         WHERE chunks_fts MATCH ?1
         ORDER BY f.rank
         LIMIT ?2",
    )?;

    let rows: Vec<(String, Option<String>, i64, i64, String, String)> = stmt
        .query_map(rusqlite::params![query, top_k as i64], |r| {
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

    if rows.is_empty() {
        println!("No results for: {}", query);
        return Ok(());
    }

    println!("Found {} results for \"{}\":\n", rows.len(), query);
    for (i, (path, name, start, end, chunk_type, snippet)) in rows.iter().enumerate() {
        println!("{}. [{}] {}:{}-{} {}",
            i + 1,
            chunk_type,
            path,
            start + 1,
            end + 1,
            name.as_deref().unwrap_or("")
        );
        println!("   {}\n", snippet);
    }

    drop(stmt);
    drop(db);
    Ok(())
}

fn cmd_stats() -> Result<(), Box<dyn std::error::Error>> {
    let db = ensure_db()?;

    let files: i64 = db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let chunks: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))?;
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
    println!("Files indexed: {}", files);
    println!("Total chunks:  {}", chunks);
    println!("DB size:       {} KB", db_size / 1024);
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
    let result: Option<(i64, i64, String, String)> = db.query_row(
        "SELECT line_start, line_end, name, content_md FROM chunks
         WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2
         LIMIT 1",
        rusqlite::params![file, line_num],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).ok();

    match result {
        Some((start, end, name, content)) => {
            println!("Chunk: {} (lines {}-{})", name, start + 1, end + 1);
            println!("{}", content);
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
    // Also remove WAL/SHM
    let wal = path.with_extension("db-wal");
    let shm = path.with_extension("db-shm");
    let _ = std::fs::remove_file(&wal);
    let _ = std::fs::remove_file(&shm);
    println!("Index reset. Run `sqmd index` to rebuild.");
    Ok(())
}
