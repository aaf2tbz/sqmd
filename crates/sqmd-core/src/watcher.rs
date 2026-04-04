use crate::files::Language;
use crate::schema;
use notify::{Event, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub fn watch(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = root.join(".sqmd/index.db");
    if !db_path.exists() {
        eprintln!("No index found. Run `sqmd init` first.");
        std::process::exit(1);
    }

    let mut db = schema::open(&db_path)?;
    let mut indexer = crate::index::Indexer::new(&mut db, root);

    // Full index first
    println!("Initial index...");
    let stats = indexer.index()?;
    println!(
        "Indexed {} files, {} chunks, {} relationships",
        stats.files_indexed, stats.chunks_total, stats.relationships_total
    );

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(root, RecursiveMode::Recursive)?;

    println!("Watching {} for changes... (Ctrl+C to stop)", root.display());

    let mut pending: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let debounce = Duration::from_millis(200);

    loop {
        match rx.recv_timeout(debounce) {
            Ok(Ok(event)) => {
                let kind = &event.kind;
                let is_relevant = matches!(
                    kind,
                    notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Remove(_)
                );
                if !is_relevant {
                    continue;
                }

                for path in &event.paths {
                    let ext = path
                        .extension()
                        .and_then(|e| e.to_str())
                        .map(|e| e.to_lowercase())
                        .unwrap_or_default();
                    let lang = Language::from_extension(&ext);
                    if lang.supported() {
                        // Only track files inside the project root
                        if let Ok(rel) = path.strip_prefix(root) {
                            pending.insert(rel.to_path_buf());
                        }
                    }
                }
            }
            Ok(Err(e)) => eprintln!("Watch error: {e}"),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if !pending.is_empty() {
                    let files: Vec<_> = pending.drain().collect();
                    for file_rel in &files {
                        let abs = root.join(file_rel);
                        match indexer.index_file(&abs) {
                            Ok(Some(r)) if r.was_deleted => {
                                println!("  - removed {}", file_rel.display())
                            }
                            Ok(Some(r)) => {
                                println!(
                                    "  ~ {} ({} chunks, {} rels)",
                                    file_rel.display(),
                                    r.chunks,
                                    r.relationships
                                )
                            }
                            Ok(None) => {}
                            Err(e) => eprintln!("  ! {}: {}", file_rel.display(), e),
                        }
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("Watch channel disconnected.");
                break;
            }
        }
    }

    Ok(())
}
