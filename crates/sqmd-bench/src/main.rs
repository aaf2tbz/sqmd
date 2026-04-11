use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "sqmd-bench", version = "0.2.0")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(default_value = ".sqmd/index.db")]
        db_path: PathBuf,
        #[arg(default_value = "layered")]
        mode: String,
    },
    Generate {
        #[arg(default_value = ".sqmd/index.db")]
        db_path: PathBuf,
        #[arg(long, default_value = "100")]
        limit: usize,
        #[arg(long, default_value = "eval_queries.json")]
        output: String,
    },
    Compare {
        #[arg(default_value = ".sqmd/index.db")]
        db_path: PathBuf,
        #[arg(long)]
        ground_truth: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroundTruth {
    id: &'static str,
    query: &'static str,
    category: &'static str,
    target_file: &'static str,
    target_function: &'static str,
    difficulty: Difficulty,
    expected_layers: Vec<&'static str>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Difficulty {
    Easy,
    Medium,
    Hard,
}

#[derive(Debug, Serialize)]
struct QueryResult {
    id: String,
    query: String,
    category: String,
    difficulty: String,
    target_file: String,
    target_function: String,
    found: bool,
    rank: Option<usize>,
    top_file_match: Option<usize>,
    score: f64,
    results_returned: usize,
    layers_hit: Vec<String>,
    entity_found: bool,
    top_result_name: Option<String>,
    top_result_file: Option<String>,
    returned_names: Vec<String>,
}

#[derive(Debug, Serialize)]
struct BenchmarkReport {
    total_queries: usize,
    recall_at_1: f64,
    recall_at_3: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    file_recall_at_10: f64,
    mrr: f64,
    entity_hit_rate: f64,
    avg_results: f64,
    layers_hit_distribution: serde_json::Map<String, serde_json::Value>,
    by_category: serde_json::Map<String, serde_json::Value>,
    by_difficulty: serde_json::Map<String, serde_json::Value>,
    results: Vec<QueryResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EvalQuery {
    chunk_id: i64,
    file_path: String,
    name: Option<String>,
    content_preview: String,
    eval_query: String,
    chunk_type: String,
    language: String,
}

#[derive(Debug, Serialize)]
struct CompareResult {
    total_queries: usize,
    lanes: serde_json::Map<String, serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct LaneMetrics {
    hit_at_1: f64,
    hit_at_3: f64,
    hit_at_5: f64,
    hit_at_10: f64,
    mrr: f64,
}

fn ground_truth() -> Vec<GroundTruth> {
    vec![
        GroundTruth {
            id: "ds-1",
            query: "look up a key for a read operation",
            category: "data_structures",
            target_file: "src/db.c",
            target_function: "lookupKeyReadWithFlags",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-2",
            query: "set a key value pair in a database",
            category: "data_structures",
            target_file: "src/db.c",
            target_function: "setKey",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-3",
            query: "incremental rehash of a dictionary hash table",
            category: "data_structures",
            target_file: "src/dict.c",
            target_function: "dictRehash",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-4",
            query: "add an entry to a hash table",
            category: "data_structures",
            target_file: "src/dict.c",
            target_function: "dictAdd",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-5",
            query: "add an element to a sorted set",
            category: "data_structures",
            target_file: "src/t_zset.c",
            target_function: "zsetAdd",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-6",
            query: "push an element onto a list",
            category: "data_structures",
            target_file: "src/t_list.c",
            target_function: "listTypePush",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-7",
            query: "insert a node into the skip list",
            category: "data_structures",
            target_file: "src/t_zset.c",
            target_function: "zslInsert",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "ds-8",
            query: "add a member to a set",
            category: "data_structures",
            target_file: "src/t_set.c",
            target_function: "setTypeAdd",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ds-9",
            query: "set a field in a hash",
            category: "data_structures",
            target_file: "src/t_hash.c",
            target_function: "hashTypeSet",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "rp-1",
            query: "serialize objects to RDB format",
            category: "persistence",
            target_file: "src/rdb.c",
            target_function: "rdbSaveObject",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "rp-2",
            query: "deserialize an object from an RDB file",
            category: "persistence",
            target_file: "src/rdb.c",
            target_function: "rdbLoadObject",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "rp-3",
            query: "flush the append only file buffer to disk",
            category: "persistence",
            target_file: "src/aof.c",
            target_function: "flushAppendOnlyFile",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "net-1",
            query: "create a new client connection",
            category: "networking",
            target_file: "src/networking.c",
            target_function: "createClient",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "net-2",
            query: "read command data from a client socket",
            category: "networking",
            target_file: "src/networking.c",
            target_function: "readQueryFromClient",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "net-3",
            query: "parse and process commands from client input buffer",
            category: "networking",
            target_file: "src/networking.c",
            target_function: "processInputBuffer",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ps-1",
            query: "publish a message to channel subscribers",
            category: "pubsub",
            target_file: "src/pubsub.c",
            target_function: "pubsubPublishMessage",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "ps-2",
            query: "subscribe a client to a pubsub channel",
            category: "pubsub",
            target_file: "src/pubsub.c",
            target_function: "pubsubSubscribeChannel",
            difficulty: Difficulty::Easy,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "cl-1",
            query: "determine which cluster node should handle a command",
            category: "clustering",
            target_file: "src/cluster.c",
            target_function: "getNodeByQuery",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "cl-2",
            query: "process cluster gossip and heartbeat messages",
            category: "clustering",
            target_file: "src/cluster_legacy.c",
            target_function: "clusterProcessPacket",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "lua-1",
            query: "execute a Lua EVAL command",
            category: "scripting",
            target_file: "src/eval.c",
            target_function: "evalGenericCommand",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "lua-2",
            query: "execute Redis commands from inside a Lua script",
            category: "scripting",
            target_file: "src/script_lua.c",
            target_function: "luaRedisGenericCommand",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "lua-3",
            query: "expose the Redis API to Lua scripting environment",
            category: "scripting",
            target_file: "src/script_lua.c",
            target_function: "luaRegisterRedisAPI",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "exp-1",
            query: "proactively expire keys in the background",
            category: "expiration",
            target_file: "src/expire.c",
            target_function: "activeExpireCycle",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "exp-2",
            query: "handle expiration of keys on access",
            category: "expiration",
            target_file: "src/db.c",
            target_function: "expireIfNeeded",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "rep-1",
            query: "replicate write commands to slave nodes",
            category: "replication",
            target_file: "src/replication.c",
            target_function: "replicationFeedSlaves",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "graph-1",
            query: "what functions call lookupKeyReadWithFlags",
            category: "graph_traversal",
            target_file: "src/db.c",
            target_function: "lookupKeyReadWithFlags",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "graph-2",
            query: "where is the main server event loop",
            category: "graph_traversal",
            target_file: "src/server.c",
            target_function: "aeMain",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts", "community"],
        },
        GroundTruth {
            id: "graph-3",
            query: "how are client connections managed during replication",
            category: "graph_traversal",
            target_file: "src/replication.c",
            target_function: "replicationFeedSlaves",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph", "community"],
        },
        GroundTruth {
            id: "graph-4",
            query: "cluster slot migration between nodes",
            category: "graph_traversal",
            target_file: "src/cluster.c",
            target_function: "migrateSlot",
            difficulty: Difficulty::Hard,
            expected_layers: vec!["fts", "graph"],
        },
        GroundTruth {
            id: "graph-5",
            query: "what data structures are used for command dispatch",
            category: "graph_traversal",
            target_file: "src/server.c",
            target_function: "lookupCommand",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts"],
        },
        GroundTruth {
            id: "comm-1",
            query: "all networking and client handling code",
            category: "community",
            target_file: "src/networking.c",
            target_function: "createClient",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts", "community"],
        },
        GroundTruth {
            id: "comm-2",
            query: "persistence and snapshotting system",
            category: "community",
            target_file: "src/rdb.c",
            target_function: "rdbSaveObject",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts", "community"],
        },
        GroundTruth {
            id: "comm-3",
            query: "module loading and initialization",
            category: "community",
            target_file: "src/module.c",
            target_function: "moduleLoad",
            difficulty: Difficulty::Medium,
            expected_layers: vec!["fts", "community"],
        },
    ]
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Run { db_path, mode }) => cmd_run(&db_path, &mode),
        Some(Commands::Generate {
            db_path,
            limit,
            output,
        }) => cmd_generate(&db_path, limit, &output),
        Some(Commands::Compare {
            db_path,
            ground_truth,
        }) => cmd_compare(&db_path, &ground_truth),
        None => {
            let db_path = std::env::args()
                .nth(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(".sqmd/index.db"));
            let mode = std::env::args()
                .nth(2)
                .unwrap_or_else(|| "layered".to_string());
            cmd_run(&db_path, &mode);
        }
    }
}

fn cmd_run(db_path: &PathBuf, mode: &str) {
    if !db_path.exists() {
        eprintln!("Database not found at {:?}", db_path);
        eprintln!("Usage: sqmd-bench run [path/to/index.db] [fts|layered]");
        std::process::exit(1);
    }

    let db = sqmd_core::schema::open(db_path).expect("Failed to open database");
    let queries = ground_truth();
    let total = queries.len();

    eprintln!("sqmd-bench v0.2.0 | {} queries | mode: {}", total, mode);

    #[cfg(feature = "native")]
    let mut embedder = sqmd_core::embed::make_provider().unwrap();

    let mut results: Vec<QueryResult> = Vec::with_capacity(total);

    for gt in &queries {
        let search_query = sqmd_core::search::SearchQuery {
            text: gt.query.to_string(),
            top_k: 10,
            source_type_filter: Some(vec!["code".to_string()]),
            exclude_path_prefixes: vec![
                "deps/".to_string(),
                "tests/".to_string(),
                "modules/".to_string(),
            ],
            ..Default::default()
        };

        let search_results = match mode {
            "fts" => sqmd_core::search::fts_search(&db, &search_query).unwrap_or_default(),
            "layered" => {
                #[cfg(feature = "native")]
                {
                    sqmd_core::search::layered_search(&db, &search_query, Some(&mut *embedder))
                        .map(|lr| lr.results)
                        .unwrap_or_default()
                }
                #[cfg(not(feature = "native"))]
                {
                    sqmd_core::search::layered_search(&db, &search_query)
                        .map(|lr| lr.results)
                        .unwrap_or_default()
                }
            }
            _ => {
                eprintln!("Unknown mode '{}'. Use 'fts' or 'layered'.", mode);
                std::process::exit(1);
            }
        };

        let mut found = false;
        let mut rank = None;
        let mut top_file_match = None;

        for (i, r) in search_results.iter().enumerate() {
            if r.name.as_deref() == Some(gt.target_function) && r.file_path == gt.target_file {
                found = true;
                rank = Some(i + 1);
                break;
            }
        }

        if !found {
            for (i, r) in search_results.iter().enumerate() {
                if r.file_path == gt.target_file {
                    top_file_match = Some(i + 1);
                    break;
                }
            }
        }

        let top_score = search_results.first().map(|r| r.score).unwrap_or(0.0);

        let entity_found = check_entity_exists(&db, gt.target_function, gt.target_file);

        let layers_hit: Vec<String> = if mode == "layered" {
            #[cfg(feature = "native")]
            {
                sqmd_core::search::layered_search(&db, &search_query, Some(&mut *embedder))
                    .map(|lr| lr.layers_hit)
                    .unwrap_or_default()
            }
            #[cfg(not(feature = "native"))]
            {
                sqmd_core::search::layered_search(&db, &search_query)
                    .map(|lr| lr.layers_hit)
                    .unwrap_or_default()
            }
        } else {
            vec!["fts".to_string()]
        };

        let top_result_name = search_results.first().and_then(|r| r.name.clone());
        let top_result_file = search_results.first().map(|r| r.file_path.clone());
        let returned_names: Vec<String> = search_results
            .iter()
            .take(5)
            .filter_map(|r| r.name.as_ref().map(|n| format!("{}:{}", n, r.file_path)))
            .collect();

        results.push(QueryResult {
            id: gt.id.to_string(),
            query: gt.query.to_string(),
            category: gt.category.to_string(),
            difficulty: format!("{:?}", gt.difficulty).to_lowercase(),
            target_file: gt.target_file.to_string(),
            target_function: gt.target_function.to_string(),
            found,
            rank,
            top_file_match,
            score: top_score,
            results_returned: search_results.len(),
            layers_hit,
            entity_found,
            top_result_name,
            top_result_file,
            returned_names,
        });
    }

    let recall_at_1 = results.iter().filter(|r| r.rank == Some(1)).count() as f64 / total as f64;
    let recall_at_3 = results
        .iter()
        .filter(|r| r.rank.is_some_and(|r| r <= 3))
        .count() as f64
        / total as f64;
    let recall_at_5 = results
        .iter()
        .filter(|r| r.rank.is_some_and(|r| r <= 5))
        .count() as f64
        / total as f64;
    let recall_at_10 = results.iter().filter(|r| r.found).count() as f64 / total as f64;
    let file_recall_at_10 = results
        .iter()
        .filter(|r| r.rank.is_some() || r.top_file_match.is_some())
        .count() as f64
        / total as f64;
    let mrr: f64 = results
        .iter()
        .map(|r| match r.rank {
            Some(n) => 1.0 / n as f64,
            None => 0.0,
        })
        .sum::<f64>()
        / total as f64;
    let avg_results =
        results.iter().map(|r| r.results_returned).sum::<usize>() as f64 / total as f64;
    let entity_hit_rate = results.iter().filter(|r| r.entity_found).count() as f64 / total as f64;

    let mut layers_hit_dist: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    for r in &results {
        for layer in &r.layers_hit {
            let count = layers_hit_dist
                .get(layer)
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;
            layers_hit_dist.insert(
                layer.clone(),
                serde_json::Value::Number(serde_json::Number::from(count + 1)),
            );
        }
    }

    let mut by_category: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();
    let mut by_difficulty: serde_json::Map<String, serde_json::Value> = serde_json::Map::new();

    let categories: Vec<&str> = results
        .iter()
        .map(|r| r.category.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    let difficulties: Vec<&str> = results
        .iter()
        .map(|r| r.difficulty.as_str())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    for cat in &categories {
        let cat_results: Vec<&QueryResult> =
            results.iter().filter(|r| r.category == *cat).collect();
        let cat_total = cat_results.len() as f64;
        let cat_recall = cat_results.iter().filter(|r| r.found).count() as f64 / cat_total;
        let cat_mrr: f64 = cat_results
            .iter()
            .map(|r| match r.rank {
                Some(n) => 1.0 / n as f64,
                None => 0.0,
            })
            .sum::<f64>()
            / cat_total;
        let cat_entity = cat_results.iter().filter(|r| r.entity_found).count() as f64 / cat_total;
        let mut m = serde_json::Map::new();
        m.insert(
            "total".into(),
            serde_json::Value::Number(serde_json::Number::from(cat_results.len())),
        );
        m.insert("recall_at_10".into(), serde_json::Value::from(cat_recall));
        m.insert("mrr".into(), serde_json::Value::from(cat_mrr));
        m.insert(
            "entity_hit_rate".into(),
            serde_json::Value::from(cat_entity),
        );
        by_category.insert(cat.to_string(), serde_json::Value::Object(m));
    }

    for diff in &difficulties {
        let d_results: Vec<&QueryResult> =
            results.iter().filter(|r| r.difficulty == *diff).collect();
        let d_total = d_results.len() as f64;
        let d_recall = d_results.iter().filter(|r| r.found).count() as f64 / d_total;
        let d_mrr: f64 = d_results
            .iter()
            .map(|r| match r.rank {
                Some(n) => 1.0 / n as f64,
                None => 0.0,
            })
            .sum::<f64>()
            / d_total;
        let mut m = serde_json::Map::new();
        m.insert(
            "total".into(),
            serde_json::Value::Number(serde_json::Number::from(d_results.len())),
        );
        m.insert("recall_at_10".into(), serde_json::Value::from(d_recall));
        m.insert("mrr".into(), serde_json::Value::from(d_mrr));
        by_difficulty.insert(diff.to_string(), serde_json::Value::Object(m));
    }

    let report = BenchmarkReport {
        total_queries: total,
        recall_at_1,
        recall_at_3,
        recall_at_5,
        recall_at_10,
        file_recall_at_10,
        mrr,
        entity_hit_rate,
        avg_results,
        layers_hit_distribution: layers_hit_dist,
        by_category,
        by_difficulty,
        results,
    };

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}

fn cmd_generate(db_path: &PathBuf, limit: usize, output: &str) {
    if !db_path.exists() {
        eprintln!("Database not found at {:?}", db_path);
        std::process::exit(1);
    }

    let db = sqmd_core::schema::open(db_path).expect("Failed to open database");

    let mut stmt = db.prepare(
         "SELECT c.id, c.file_path, c.name, c.content_raw, c.chunk_type, c.language, c.importance \
          FROM chunks c WHERE c.is_deleted = 0 AND c.name IS NOT NULL AND c.importance >= 0.2 \
          ORDER BY c.importance DESC LIMIT ?1",
    ).unwrap();

    let rows: Vec<(i64, String, String, String, String, String, f64)> = stmt
        .query_map(rusqlite::params![limit as i64], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();

    let mut eval_queries: Vec<EvalQuery> = Vec::new();

    for (chunk_id, file_path, name, content_raw, chunk_type, language, _importance) in &rows {
        let query = generate_eval_query(name, content_raw);
        let preview = if content_raw.len() > 200 {
            &content_raw[..200]
        } else {
            content_raw
        };
        eval_queries.push(EvalQuery {
            chunk_id: *chunk_id,
            file_path: file_path.clone(),
            name: Some(name.clone()),
            content_preview: preview.to_string(),
            eval_query: query,
            chunk_type: chunk_type.clone(),
            language: language.clone(),
        });
    }

    let json = serde_json::to_string_pretty(&eval_queries).unwrap();
    std::fs::write(output, &json).expect("Failed to write output file");
    eprintln!(
        "Generated {} eval queries to {}",
        eval_queries.len(),
        output
    );
}

fn generate_eval_query(name: &str, content: &str) -> String {
    #[cfg(feature = "ollama")]
    {
        if let Ok(client) = std::panic::catch_unwind(sqmd_core::ollama::OllamaClient::new) {
            if let Ok(hints) = client.generate_prospective_hints(content) {
                if let Some(first) = hints.into_iter().next() {
                    return first;
                }
            }
        }
    }

    let _ = name;
    template_eval_query(name, content)
}

fn template_eval_query(name: &str, content: &str) -> String {
    let first_line = content.lines().next().unwrap_or("");
    let words: Vec<&str> = first_line.split_whitespace().take(6).collect();
    if !words.is_empty() {
        format!("how does {} work", words.join(" "))
    } else {
        format!("find {}", name)
    }
}

fn cmd_compare(db_path: &PathBuf, ground_truth_path: &str) {
    if !db_path.exists() {
        eprintln!("Database not found at {:?}", db_path);
        std::process::exit(1);
    }

    let gt_content =
        std::fs::read_to_string(ground_truth_path).expect("Failed to read ground truth file");
    let eval_queries: Vec<EvalQuery> =
        serde_json::from_str(&gt_content).expect("Failed to parse ground truth JSON");

    let db = sqmd_core::schema::open(db_path).expect("Failed to open database");
    let total = eval_queries.len();

    let mut compare_result = CompareResult {
        total_queries: total,
        lanes: serde_json::Map::new(),
    };

    let lanes = vec!["fts", "layered"];

    for lane_name in &lanes {
        let mut hit_at_1 = 0usize;
        let mut hit_at_3 = 0usize;
        let mut hit_at_5 = 0usize;
        let mut hit_at_10 = 0usize;
        let mut mrr_sum = 0.0f64;

        #[cfg(feature = "native")]
        let mut embedder = sqmd_core::embed::make_provider().unwrap();

        for eq in &eval_queries {
            let search_query = sqmd_core::search::SearchQuery {
                text: eq.eval_query.clone(),
                top_k: 10,
                ..Default::default()
            };

            let search_results = match *lane_name {
                "fts" => sqmd_core::search::fts_search(&db, &search_query).unwrap_or_default(),
                "layered" => {
                    #[cfg(feature = "native")]
                    {
                        sqmd_core::search::layered_search(&db, &search_query, Some(&mut *embedder))
                            .map(|lr| lr.results)
                            .unwrap_or_default()
                    }
                    #[cfg(not(feature = "native"))]
                    {
                        sqmd_core::search::layered_search(&db, &search_query)
                            .map(|lr| lr.results)
                            .unwrap_or_default()
                    }
                }
                _ => Vec::new(),
            };

            let mut rank = None;
            for (i, r) in search_results.iter().enumerate() {
                if r.chunk_id == eq.chunk_id {
                    rank = Some(i + 1);
                    break;
                }
            }

            if let Some(1) = rank {
                hit_at_1 += 1;
            }
            if rank.is_some_and(|r| r <= 3) {
                hit_at_3 += 1;
            }
            if rank.is_some_and(|r| r <= 5) {
                hit_at_5 += 1;
            }
            if rank.is_some_and(|r| r <= 10) {
                hit_at_10 += 1;
            }
            mrr_sum += rank.map(|r| 1.0 / r as f64).unwrap_or(0.0);
        }

        let metrics = LaneMetrics {
            hit_at_1: hit_at_1 as f64 / total as f64,
            hit_at_3: hit_at_3 as f64 / total as f64,
            hit_at_5: hit_at_5 as f64 / total as f64,
            hit_at_10: hit_at_10 as f64 / total as f64,
            mrr: mrr_sum / total as f64,
        };

        compare_result.lanes.insert(
            lane_name.to_string(),
            serde_json::to_value(&metrics).unwrap(),
        );
    }

    println!("{}", serde_json::to_string_pretty(&compare_result).unwrap());
}

fn check_entity_exists(db: &rusqlite::Connection, function_name: &str, file_path: &str) -> bool {
    let result = db.query_row(
        "SELECT COUNT(*) FROM entities WHERE name = ?1 AND file_path = ?2",
        rusqlite::params![function_name, file_path],
        |r| r.get::<_, i64>(0),
    );
    result.unwrap_or(0) > 0
}
