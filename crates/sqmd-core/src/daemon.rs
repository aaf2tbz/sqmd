use crate::context::{ContextAssembler, ContextRequest};
use crate::schema;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};

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

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                if let Err(e) = handle_connection(stream, root) {
                    eprintln!("Connection error: {e}");
                }
            }
            Err(e) => eprintln!("Accept error: {e}"),
        }
    }

    Ok(())
}

fn handle_connection(stream: UnixStream, root: &Path) -> Result<(), Box<dyn std::error::Error>> {
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
            write_response(&mut writer, Response {
                ok: false,
                result: None,
                error: Some(format!("Invalid JSON: {e}")),
            })?;
            return Ok(());
        }
    };

    let db_path = root.join(".sqmd/index.db");
    if !db_path.exists() {
        write_response(&mut writer, Response {
            ok: false,
            result: None,
            error: Some("No index found. Run sqmd init + sqmd index.".to_string()),
        })?;
        return Ok(());
    }

    #[cfg(feature = "embed")]
    let mut db = schema::open(&db_path)?;
    #[cfg(not(feature = "embed"))]
    let db = schema::open(&db_path)?;
    let response = match request.method.as_str() {
        "search" => handle_search(&db, &request.params),
        "context" => handle_context(&db, &request.params),
        "get" => handle_get(&db, &request.params),
        "stats" => handle_stats(&db),
        #[cfg(feature = "embed")]
        "embed" => handle_embed(&mut db),
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

fn handle_search(db: &Connection, params: &serde_json::Value) -> Response {
    let query = params["query"].as_str().unwrap_or("");
    let top_k = params["top_k"].as_u64().unwrap_or(10) as usize;
    let alpha = params["alpha"].as_f64().unwrap_or(0.7);
    let file = params["file"].as_str().map(|s| s.to_string());
    let type_filter = params["type"].as_str().map(|s| s.to_string());

    if query.is_empty() {
        return Response {
            ok: false,
            result: None,
            error: Some("Missing query parameter".to_string()),
        };
    }

    let search_query = crate::search::SearchQuery {
        text: query.to_string(),
        top_k,
        alpha,
        file_filter: file,
        type_filter,
        ..Default::default()
    };

    #[cfg(feature = "embed")]
    let result = {
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
        match crate::search::hybrid_search(db, &search_query, &mut embedder) {
            Ok(results) => {
                let serialized = serde_json::to_string(&results).unwrap_or_default();
                Response {
                    ok: true,
                    result: Some(serde_json::from_str(&serialized).unwrap_or(serde_json::Value::Array(vec![]))),
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
                let serialized = serde_json::to_string(&results).unwrap_or_default();
                Response {
                    ok: true,
                    result: Some(serde_json::from_str(&serialized).unwrap_or(serde_json::Value::Array(vec![]))),
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
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
        .unwrap_or_default();
    let max_tokens = params["max_tokens"].as_u64().unwrap_or(8000) as usize;
    let include_deps = params["include_deps"].as_bool().unwrap_or(true);
    let dep_depth = params["dep_depth"].as_u64().unwrap_or(1) as usize;
    let top_k = params["top_k"].as_u64().unwrap_or(10) as usize;

    let request = ContextRequest {
        query,
        files,
        max_tokens,
        include_deps,
        dep_depth,
        top_k,
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
        "SELECT line_start, line_end, name, language, content_raw FROM chunks
         WHERE file_path = ?1 AND line_start <= ?2 AND line_end >= ?2 LIMIT 1",
        rusqlite::params![file, line_num],
        |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<String>>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?)),
    ) {
        Ok((start, end, name, language, content)) => {
            let mut md = String::new();
            md.push_str(&format!(
                "### `{}`\n\n**File:** `{}` | **Lines:** {}-{} | **Type:** chunk\n\n```{}\n{}\n```\n",
                name.as_deref().unwrap_or("(unnamed)"),
                file,
                start + 1,
                end + 1,
                language,
                content,
            ));
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
    let files: i64 = db.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
    let chunks: i64 = db.query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get(0)).unwrap_or(0);
    let rels: i64 = db.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0)).unwrap_or(0);
    let embedded: i64 = db.query_row("SELECT COUNT(*) FROM embeddings", [], |r| r.get(0)).unwrap_or(0);

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
