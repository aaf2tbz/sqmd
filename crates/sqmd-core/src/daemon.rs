use crate::context::{ContextAssembler, ContextRequest};
use crate::query_cache::QueryCache;
use crate::schema;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct DaemonState {
    pub cache: Mutex<QueryCache>,
    #[cfg(feature = "embed")]
    pub embedder: Mutex<Option<crate::embed::Embedder>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub method: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub fn serve(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME").map_err(|_| "HOME not set")?;
    let sock_path = PathBuf::from(home).join(".sqmd").join("daemon.sock");

    if sock_path.exists() {
        std::fs::remove_file(&sock_path)?;
    }

    let listener = UnixListener::bind(&sock_path)?;
    eprintln!("sqmd daemon listening on {}", sock_path.display());

    let root_owned = root.to_path_buf();
    let state = Arc::new(DaemonState {
        cache: Mutex::new(QueryCache::new()),
        #[cfg(feature = "embed")]
        embedder: Mutex::new(None),
    });

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let r = root_owned.clone();
                let s = state.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &r, &s) {
                        eprintln!("Connection error: {e}");
                    }
                });
            }
            Err(e) => eprintln!("Accept error: {e}"),
        }
    }

    Ok(())
}

fn handle_connection(
    stream: UnixStream,
    root: &Path,
    state: &Arc<DaemonState>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = BufReader::new(&stream);
    let mut writer = &stream;

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let line = line.trim();

    if line.is_empty() {
        return Ok(());
    }

    let request: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            write_response(
                &mut writer,
                Response {
                    ok: false,
                    result: None,
                    error: Some(format!("Invalid JSON: {e}")),
                },
            )?;
            return Ok(());
        }
    };

    let db_path = root.join(".sqmd/index.db");
    if !db_path.exists() {
        write_response(
            &mut writer,
            Response {
                ok: false,
                result: None,
                error: Some("No index found. Run sqmd init + sqmd index.".to_string()),
            },
        )?;
        return Ok(());
    }

    let is_write = matches!(
        request.method.as_str(),
        "ingest" | "ingest_batch" | "forget" | "modify"
    );
    #[allow(unused_variables)]
    let is_embed = cfg!(feature = "embed") && matches!(request.method.as_str(), "embed");

    #[cfg(feature = "embed")]
    let mut db = if is_write || is_embed {
        schema::open(&db_path)?
    } else {
        schema::open_fast(&db_path)?
    };
    #[cfg(not(feature = "embed"))]
    let db = if is_write {
        schema::open(&db_path)?
    } else {
        schema::open_fast(&db_path)?
    };

    let response = match request.method.as_str() {
        "search" => handle_search(&db, &request.params, state),
        "context" => handle_context(&db, &request.params),
        "get" => handle_get(&db, &request.params),
        "stats" => handle_stats(&db),
        "ls" => handle_ls(&db, &request.params),
        "cat" => handle_cat(&db, &request.params),
        "ingest" => handle_ingest(&db, &request.params),
        "ingest_batch" => handle_ingest_batch(&db, &request.params),
        "forget" => handle_forget(&db, &request.params),
        "modify" => handle_modify(&db, &request.params),
        #[cfg(feature = "embed")]
        "embed" => handle_embed(&mut db),
        #[cfg(feature = "embed")]
        "embed_text" => handle_embed_text(&request.params),
        #[cfg(feature = "embed")]
        "embed_batch" => handle_embed_batch(&request.params),
        "communities" => handle_communities(&db, &request.params),
        "community_summary" => handle_community_summary(&db, &request.params),
        "project_summary" => handle_project_summary(&db),
        "supersede_fact" => handle_supersede_fact(&db, &request.params),
        "facts_at" => handle_facts_at(&db, &request.params),
        "fact_history" => handle_fact_history(&db, &request.params),
        "episodes" => handle_episodes(&db, &request.params),
        "episode_stats" => handle_episode_stats(&db),
        "layered_search" => handle_layered_search(&db, &request.params, state),
        _ => Response {
            ok: false,
            result: None,
            error: Some(format!("Unknown method: {}", request.method)),
        },
    };

    write_response(&mut writer, response)?;
    Ok(())
}

fn write_response(
    writer: &mut impl Write,
    response: Response,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(&response)?;
    writeln!(writer, "{}", json)?;
    writer.flush()?;
    Ok(())
}

fn handle_search(
    db: &Connection,
    params: &serde_json::Value,
    state: &Arc<DaemonState>,
) -> Response {
    let query = params["query"].as_str().unwrap_or("");
    let top_k = params["top_k"].as_u64().unwrap_or(10) as usize;
    let alpha = params["alpha"].as_f64().unwrap_or(0.7);
    let file = params["file"].as_str().map(|s| s.to_string());
    let type_filter = params["type"].as_str().map(|s| s.to_string());
    let source_types: Option<Vec<String>> = params
        .get("source_types")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        });
    let agent_id = params["agent_id"].as_str().map(|s| s.to_string());

    if query.is_empty() {
        return Response {
            ok: false,
            result: None,
            error: Some("Missing query parameter".to_string()),
        };
    }

    if let Some(cached) = {
        let c = state.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.lookup(
            query,
            top_k,
            file.as_deref(),
            type_filter.as_deref(),
            source_types.as_deref(),
            agent_id.as_deref(),
        )
    } {
        return Response {
            ok: true,
            result: Some(
                serde_json::from_str(&serde_json::to_string(&cached).unwrap_or_default())
                    .unwrap_or(serde_json::Value::Array(vec![])),
            ),
            error: None,
        };
    }

    let search_query = crate::search::SearchQuery {
        text: query.to_string(),
        top_k,
        alpha,
        file_filter: file.clone(),
        type_filter: type_filter.clone(),
        source_type_filter: source_types.clone(),
        agent_id_filter: agent_id.clone(),
        ..Default::default()
    };

    #[cfg(feature = "embed")]
    let result = {
        let mut embedder = {
            let mut e = state.embedder.lock().unwrap_or_else(|e| e.into_inner());
            if e.is_none() {
                match crate::embed::Embedder::new() {
                    Ok(emb) => *e = Some(emb),
                    Err(err) => {
                        return Response {
                            ok: false,
                            result: None,
                            error: Some(err.to_string()),
                        }
                    }
                }
            }
            e.take().unwrap()
        };
        match crate::search::hybrid_search(db, &search_query, &mut embedder) {
            Ok(results) => {
                let results_clone = results.clone();
                {
                    let mut c = state.cache.lock().unwrap_or_else(|e| e.into_inner());
                    c.store(
                        query,
                        top_k,
                        file.as_deref(),
                        type_filter.as_deref(),
                        source_types.as_deref(),
                        agent_id.as_deref(),
                        results_clone,
                    );
                }
                {
                    let mut e = state.embedder.lock().unwrap_or_else(|e| e.into_inner());
                    *e = Some(embedder);
                }
                let markdown = crate::search::render_search_markdown(db, &results).ok();
                let mut arr =
                    serde_json::to_value(&results).unwrap_or(serde_json::Value::Array(vec![]));
                if let (Some(md), serde_json::Value::Array(ref mut items)) = (markdown, &mut arr) {
                    for (i, item) in items.iter_mut().enumerate() {
                        if let Some(m) = md.get(i) {
                            item.as_object_mut().map(|obj| {
                                obj.insert(
                                    "markdown".to_string(),
                                    serde_json::Value::String(m.clone()),
                                )
                            });
                        }
                    }
                }
                Response {
                    ok: true,
                    result: Some(arr),
                    error: None,
                }
            }
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        }
    };
    #[cfg(not(feature = "embed"))]
    let result = {
        match crate::search::fts_search(db, &search_query) {
            Ok(results) => {
                let results_clone = results.clone();
                {
                    let mut c = state.cache.lock().unwrap_or_else(|e| e.into_inner());
                    c.store(
                        query,
                        top_k,
                        file.as_deref(),
                        type_filter.as_deref(),
                        source_types.as_deref(),
                        agent_id.as_deref(),
                        results_clone,
                    );
                }
                let markdown = crate::search::render_search_markdown(db, &results).ok();
                let mut arr =
                    serde_json::to_value(&results).unwrap_or(serde_json::Value::Array(vec![]));
                if let (Some(md), serde_json::Value::Array(ref mut items)) = (markdown, &mut arr) {
                    for (i, item) in items.iter_mut().enumerate() {
                        if let Some(m) = md.get(i) {
                            item.as_object_mut().map(|obj| {
                                obj.insert(
                                    "markdown".to_string(),
                                    serde_json::Value::String(m.clone()),
                                )
                            });
                        }
                    }
                }
                Response {
                    ok: true,
                    result: Some(arr),
                    error: None,
                }
            }
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        }
    };

    result
}

fn handle_context(db: &Connection, params: &serde_json::Value) -> Response {
    let query = params["query"].as_str().unwrap_or("").to_string();
    let files: Vec<String> = params["files"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let max_tokens = params["max_tokens"].as_u64().unwrap_or(8000) as usize;
    let include_deps = params["include_deps"].as_bool().unwrap_or(true);
    let dep_depth = params["dep_depth"].as_u64().unwrap_or(1) as usize;
    let top_k = params["top_k"].as_u64().unwrap_or(10) as usize;

    let source_types: Option<Vec<String>> = params["source_types"].as_array().map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    });

    let request = ContextRequest {
        query,
        files,
        max_tokens,
        include_deps,
        dep_depth,
        top_k,
        source_types,
    };

    match ContextAssembler::build(db, &request) {
        Ok(resp) => {
            let json = serde_json::to_value(&resp).unwrap_or_default();
            Response {
                ok: true,
                result: Some(json),
                error: None,
            }
        }
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_get(db: &Connection, params: &serde_json::Value) -> Response {
    let location = params["location"].as_str().unwrap_or("");

    let (file, line) = match location.rsplit_once(':') {
        Some(parts) => parts,
        None => {
            return Response {
                ok: false,
                result: None,
                error: Some("Invalid location format. Use file:line".to_string()),
            }
        }
    };

    let line_num: i64 = match line.parse() {
        Ok(n) => n,
        Err(_) => {
            return Response {
                ok: false,
                result: None,
                error: Some("Invalid line number".to_string()),
            }
        }
    };

    match db.query_row(
        "SELECT line_start, line_end, name, language, content_raw, source_type, importance, chunk_type FROM chunks
         WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2 LIMIT 1",
        rusqlite::params![file, line_num],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, f64>(6)?,
                r.get::<_, String>(7)?,
            ))
        },
    ) {
        Ok((start, end, name, language, content, source_type, importance, chunk_type)) => {
            let chunk = crate::chunk::Chunk {
                file_path: file.to_string(),
                language: language.clone(),
                chunk_type: crate::chunk::ChunkType::from_str_name(&chunk_type)
                    .unwrap_or(crate::chunk::ChunkType::Fact),
                name: name.clone(),
                signature: None,
                line_start: start as usize,
                line_end: end as usize,
                content_raw: content,
                content_hash: String::new(),
                importance,
                source_type: crate::chunk::SourceType::from_str_name(&source_type)
                    .unwrap_or(crate::chunk::SourceType::Code),
                metadata: serde_json::Map::new(),
                agent_id: None,
                tags: None,
                decay_rate: 0.0,
                created_by: None,
            };
            let md = chunk.render_md();
            Response {
                ok: true,
                result: Some(serde_json::json!({
                    "markdown": md,
                    "file_path": file,
                    "name": name,
                    "line_start": start,
                    "line_end": end,
                    "language": language,
                })),
                error: None,
            }
        }
        Err(_) => Response {
            ok: false,
            result: None,
            error: Some(format!("No chunk found at {}", location)),
        },
    }
}

fn handle_stats(db: &Connection) -> Response {
    let files: i64 = db
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap_or(0);
    let chunks: i64 = db
        .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0))
        .unwrap_or(0);
    let rels: i64 = db
        .query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))
        .unwrap_or(0);
    let embedded: i64 = db
        .query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0))
        .unwrap_or(0);

    Response {
        ok: true,
        result: Some(serde_json::json!({
            "files": files,
            "chunks": chunks,
            "relationships": rels,
            "embedded": embedded,
        })),
        error: None,
    }
}

#[cfg(feature = "embed")]
fn handle_embed(db: &mut Connection) -> Response {
    let mut embedder = match crate::embed::Embedder::new() {
        Ok(e) => e,
        Err(e) => {
            return Response {
                ok: false,
                result: None,
                error: Some(format!("{e}")),
            }
        }
    };

    match crate::search::embed_unembedded(db, &mut embedder) {
        Ok(count) => Response {
            ok: true,
            result: Some(serde_json::json!({"embedded": count})),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

#[cfg(feature = "embed")]
fn handle_embed_text(params: &serde_json::Value) -> Response {
    let text = match params["text"].as_str() {
        Some(t) => t,
        None => {
            return Response {
                ok: false,
                result: None,
                error: Some("Missing 'text' parameter".to_string()),
            }
        }
    };

    let mut embedder = match crate::embed::Embedder::new() {
        Ok(e) => e,
        Err(e) => {
            return Response {
                ok: false,
                result: None,
                error: Some(format!("{e}")),
            }
        }
    };

    match embedder.embed_one(text) {
        Ok(vector) => Response {
            ok: true,
            result: Some(serde_json::json!({
                "embedding": vector,
                "dimensions": vector.len(),
                "model": embedder.model_name(),
            })),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

#[cfg(feature = "embed")]
fn handle_embed_batch(params: &serde_json::Value) -> Response {
    let texts: Vec<String> = match params
        .get("texts")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => {
            return Response {
                ok: false,
                result: None,
                error: Some("Missing 'texts' array".to_string()),
            }
        }
    };

    if texts.is_empty() {
        return Response {
            ok: true,
            result: Some(
                serde_json::json!({"embeddings": [], "dimensions": 768, "model": "nomic-embed-text-v1.5"}),
            ),
            error: None,
        };
    }

    let mut embedder = match crate::embed::Embedder::new() {
        Ok(e) => e,
        Err(e) => {
            return Response {
                ok: false,
                result: None,
                error: Some(format!("{e}")),
            }
        }
    };

    let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    match embedder.embed_batch(&text_refs) {
        Ok(vectors) => Response {
            ok: true,
            result: Some(serde_json::json!({
                "embeddings": vectors,
                "dimensions": 768,
                "model": embedder.model_name(),
                "count": vectors.len(),
            })),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_ls(db: &Connection, params: &serde_json::Value) -> Response {
    let file = params["file"].as_str();
    let type_filter = params["type"].as_str();
    let depth = params["depth"].as_u64().unwrap_or(1) as usize;

    match crate::vfs::list_chunks(db, file, type_filter, depth) {
        Ok(entries) => {
            let json = serde_json::to_value(&entries).unwrap_or_default();
            Response {
                ok: true,
                result: Some(json),
                error: None,
            }
        }
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_cat(db: &Connection, params: &serde_json::Value) -> Response {
    let id = params["id"].as_i64().unwrap_or(0);

    match crate::vfs::get_chunk_by_id(db, id) {
        Ok(Some(entry)) => {
            let row: Option<(String, String, f64, Option<String>)> = db
                .query_row(
                    "SELECT content_raw, source_type, importance, tags FROM chunks WHERE id = ?1",
                    rusqlite::params![id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .ok();

            let markdown = row
                .as_ref()
                .map(|(content_raw, source_type, importance, tags_json)| {
                    let tags: Option<Vec<String>> = tags_json
                        .as_ref()
                        .and_then(|t| serde_json::from_str(t).ok());
                    let chunk = crate::chunk::Chunk {
                        file_path: entry.file_path.clone(),
                        language: entry.language.clone(),
                        chunk_type: crate::chunk::ChunkType::from_str_name(&entry.chunk_type)
                            .unwrap_or(crate::chunk::ChunkType::Fact),
                        name: entry.name.clone(),
                        signature: entry.signature.clone(),
                        line_start: (entry.line_start - 1) as usize,
                        line_end: (entry.line_end - 1) as usize,
                        content_raw: content_raw.clone(),
                        content_hash: String::new(),
                        importance: *importance,
                        source_type: crate::chunk::SourceType::from_str_name(source_type)
                            .unwrap_or(crate::chunk::SourceType::Code),
                        metadata: serde_json::Map::new(),
                        agent_id: None,
                        tags,
                        decay_rate: 0.0,
                        created_by: None,
                    };
                    chunk.render_md()
                });

            Response {
                ok: true,
                result: Some(serde_json::json!({
                    "id": entry.id,
                    "file_path": entry.file_path,
                    "language": entry.language,
                    "chunk_type": entry.chunk_type,
                    "name": entry.name,
                    "signature": entry.signature,
                    "line_start": entry.line_start + 1,
                    "line_end": entry.line_end + 1,
                    "content": row.as_ref().map(|r| r.0.clone()).unwrap_or_default(),
                    "markdown": markdown.unwrap_or_default(),
                })),
                error: None,
            }
        }
        Ok(None) => Response {
            ok: false,
            result: None,
            error: Some(format!("No chunk found with id {}", id)),
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

// ── Knowledge ingest handlers ───────────────────────────────────────

fn handle_ingest(db: &Connection, params: &serde_json::Value) -> Response {
    let input: crate::index::KnowledgeChunk = match serde_json::from_value(params.clone()) {
        Ok(v) => v,
        Err(e) => {
            return Response {
                ok: false,
                result: None,
                error: Some(format!("Invalid ingest params: {e}")),
            }
        }
    };

    let ingestor = crate::index::KnowledgeIngestor::new(db);
    match ingestor.ingest(&input) {
        Ok(result) => Response {
            ok: true,
            result: Some(serde_json::to_value(&result).unwrap_or_default()),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_ingest_batch(db: &Connection, params: &serde_json::Value) -> Response {
    let chunks: Vec<crate::index::KnowledgeChunk> = match params
        .get("chunks")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
    {
        Some(v) => v,
        None => {
            return Response {
                ok: false,
                result: None,
                error: Some("Missing 'chunks' array".to_string()),
            }
        }
    };

    let ingestor = crate::index::KnowledgeIngestor::new(db);
    match ingestor.ingest_batch(&chunks) {
        Ok(result) => Response {
            ok: true,
            result: Some(serde_json::to_value(&result).unwrap_or_default()),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_forget(db: &Connection, params: &serde_json::Value) -> Response {
    let chunk_id = match params["id"].as_i64() {
        Some(id) => id,
        None => {
            return Response {
                ok: false,
                result: None,
                error: Some("Missing 'id' parameter".to_string()),
            }
        }
    };

    let ingestor = crate::index::KnowledgeIngestor::new(db);
    match ingestor.forget(chunk_id) {
        Ok(found) => Response {
            ok: true,
            result: Some(serde_json::json!({"deleted": found})),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_modify(db: &Connection, params: &serde_json::Value) -> Response {
    let chunk_id = match params["id"].as_i64() {
        Some(id) => id,
        None => {
            return Response {
                ok: false,
                result: None,
                error: Some("Missing 'id' parameter".to_string()),
            }
        }
    };

    let importance = params["importance"].as_f64();
    let tags: Option<Vec<String>> = params
        .get("tags")
        .and_then(|v| serde_json::from_value(v.clone()).ok());

    let ingestor = crate::index::KnowledgeIngestor::new(db);
    match ingestor.modify(chunk_id, importance, tags) {
        Ok(_) => Response {
            ok: true,
            result: Some(serde_json::json!({"modified": true})),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_communities(db: &Connection, params: &serde_json::Value) -> Response {
    let query = params["query"].as_str().unwrap_or("");
    let top_k = params["top_k"].as_u64().unwrap_or(20) as usize;

    if query.is_empty() {
        match crate::communities::ensure_communities(db) {
            Ok(count) => match crate::communities::regenerate_summaries(db) {
                Ok(updated) => Response {
                    ok: true,
                    result: Some(serde_json::json!({
                        "communities_count": count,
                        "summaries_generated": updated,
                    })),
                    error: None,
                },
                Err(e) => Response {
                    ok: false,
                    result: None,
                    error: Some(e.to_string()),
                },
            },
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        }
    } else {
        match crate::communities::search_communities(db, query, top_k) {
            Ok(communities) => {
                let json =
                    serde_json::to_value(&communities).unwrap_or(serde_json::Value::Array(vec![]));
                Response {
                    ok: true,
                    result: Some(json),
                    error: None,
                }
            }
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        }
    }
}

fn handle_community_summary(db: &Connection, params: &serde_json::Value) -> Response {
    let community_id = params["id"].as_i64().unwrap_or(0);

    if community_id == 0 {
        return Response {
            ok: false,
            result: None,
            error: Some("Missing 'id' parameter".to_string()),
        };
    }

    let chunks = crate::communities::get_community_chunks(db, community_id);
    let community_path: Option<String> = db
        .query_row(
            "SELECT path FROM communities WHERE id = ?1",
            rusqlite::params![community_id],
            |r| r.get::<_, String>(0),
        )
        .ok();

    match (community_path, chunks) {
        (_, Ok(chunks)) => {
            let chunk_list: Vec<serde_json::Value> = chunks
                .iter()
                .map(|(id, fp, name, ct, ls, le)| {
                    serde_json::json!({
                        "chunk_id": id,
                        "file_path": fp,
                        "name": name,
                        "chunk_type": ct,
                        "line_start": ls + 1,
                        "line_end": le + 1,
                    })
                })
                .collect();
            Response {
                ok: true,
                result: Some(serde_json::json!({ "chunks": chunk_list })),
                error: None,
            }
        }
        _ => Response {
            ok: false,
            result: None,
            error: Some(format!("No community found with id {}", community_id)),
        },
    }
}

fn handle_project_summary(db: &Connection) -> Response {
    match crate::communities::get_project_summary(db) {
        Ok(summary) => Response {
            ok: true,
            result: Some(serde_json::json!({ "markdown": summary })),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_supersede_fact(db: &Connection, params: &serde_json::Value) -> Response {
    let source = params.get("source_entity").and_then(|v| v.as_i64());
    let target = params.get("target_entity").and_then(|v| v.as_i64());
    let dep_type = params.get("dep_type").and_then(|v| v.as_str());

    match (source, target, dep_type) {
        (Some(s), Some(t), Some(dt)) => match crate::entities::supersede_dependency(db, s, t, dt) {
            Ok(count) => Response {
                ok: true,
                result: Some(serde_json::json!({ "superseded": count })),
                error: None,
            },
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        },
        _ => Response {
            ok: false,
            result: None,
            error: Some("source_entity, target_entity, and dep_type required".into()),
        },
    }
}

fn handle_facts_at(db: &Connection, params: &serde_json::Value) -> Response {
    let entity_id = params.get("entity_id").and_then(|v| v.as_i64());
    let as_of = params
        .get("as_of")
        .and_then(|v| v.as_str())
        .unwrap_or("now");

    match entity_id {
        Some(eid) => match crate::entities::query_dependencies_at(db, eid, as_of) {
            Ok(facts) => {
                let items: Vec<serde_json::Value> = facts
                    .into_iter()
                    .map(|(src, dt, strength, mentions, vf, vt)| {
                        serde_json::json!({
                            "source_entity": src,
                            "dep_type": dt,
                            "strength": strength,
                            "mentions": mentions,
                            "valid_from": vf,
                            "valid_to": vt,
                        })
                    })
                    .collect();
                Response {
                    ok: true,
                    result: Some(serde_json::json!({ "facts": items })),
                    error: None,
                }
            }
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        },
        None => Response {
            ok: false,
            result: None,
            error: Some("entity_id required".into()),
        },
    }
}

fn handle_fact_history(db: &Connection, params: &serde_json::Value) -> Response {
    let source = params.get("source_entity").and_then(|v| v.as_i64());
    let target = params.get("target_entity").and_then(|v| v.as_i64());
    let dep_type = params.get("dep_type").and_then(|v| v.as_str());

    match (source, target, dep_type) {
        (Some(s), Some(t), Some(dt)) => match crate::entities::get_fact_history(db, s, t, dt) {
            Ok(history) => {
                let items: Vec<serde_json::Value> = history
                    .into_iter()
                    .map(|(id, vf, vt, mentions)| {
                        serde_json::json!({
                            "id": id,
                            "valid_from": vf,
                            "valid_to": vt,
                            "mentions": mentions,
                        })
                    })
                    .collect();
                Response {
                    ok: true,
                    result: Some(serde_json::json!({ "history": items })),
                    error: None,
                }
            }
            Err(e) => Response {
                ok: false,
                result: None,
                error: Some(e.to_string()),
            },
        },
        _ => Response {
            ok: false,
            result: None,
            error: Some("source_entity, target_entity, and dep_type required".into()),
        },
    }
}

fn handle_episodes(db: &Connection, params: &serde_json::Value) -> Response {
    let file_path = params.get("file_path").and_then(|v| v.as_str());
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

    let result = match file_path {
        Some(fp) => crate::episodes::get_file_episodes(db, fp, limit),
        None => crate::episodes::get_recent_episodes(db, limit),
    };

    match result {
        Ok(episodes) => Response {
            ok: true,
            result: Some(serde_json::json!({ "episodes": episodes })),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_episode_stats(db: &Connection) -> Response {
    match crate::episodes::get_episode_stats(db) {
        Ok(stats) => Response {
            ok: true,
            result: Some(serde_json::json!({ "stats": stats })),
            error: None,
        },
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

fn handle_layered_search(
    db: &Connection,
    params: &serde_json::Value,
    state: &Arc<DaemonState>,
) -> Response {
    let query = params["query"].as_str().unwrap_or("");
    let top_k = params["top_k"].as_u64().unwrap_or(10) as usize;

    if query.is_empty() {
        return Response {
            ok: false,
            result: None,
            error: Some("Missing query parameter".to_string()),
        };
    }

    if let Some(cached) = {
        let c = state.cache.lock().unwrap_or_else(|e| e.into_inner());
        c.lookup(query, top_k, None, None, None, None)
    } {
        return Response {
            ok: true,
            result: Some(serde_json::json!({
                "results": cached,
                "layers_hit": ["fts", "cached"],
            })),
            error: None,
        };
    }

    let search_query = crate::search::SearchQuery {
        text: query.to_string(),
        top_k,
        ..Default::default()
    };

    match crate::search::layered_search(db, &search_query) {
        Ok(layered) => {
            let markdown = crate::search::render_search_markdown(db, &layered.results).ok();
            let mut arr =
                serde_json::to_value(&layered.results).unwrap_or(serde_json::Value::Array(vec![]));
            if let (Some(md), serde_json::Value::Array(ref mut items)) = (markdown, &mut arr) {
                for (i, item) in items.iter_mut().enumerate() {
                    if let Some(m) = md.get(i) {
                        item.as_object_mut().map(|obj| {
                            obj.insert("markdown".to_string(), serde_json::Value::String(m.clone()))
                        });
                    }
                }
            }
            Response {
                ok: true,
                result: Some(serde_json::json!({
                    "results": arr,
                    "layers_hit": layered.layers_hit,
                })),
                error: None,
            }
        }
        Err(e) => Response {
            ok: false,
            result: None,
            error: Some(e.to_string()),
        },
    }
}

pub fn query_daemon(request: &Request) -> Result<Response, Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let sock_path = PathBuf::from(home).join(".sqmd").join("daemon.sock");

    let mut stream = UnixStream::connect(&sock_path)?;
    let json = serde_json::to_string(request)?;
    writeln!(stream, "{}", json)?;
    stream.flush()?;

    let mut reader = BufReader::new(&stream);
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: Response = serde_json::from_str(response_line.trim())?;
    Ok(response)
}
