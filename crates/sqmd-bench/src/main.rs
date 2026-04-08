use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GroundTruth {
    id: &'static str,
    query: &'static str,
    category: &'static str,
    target_file: &'static str,
    target_function: &'static str,
    difficulty: Difficulty,
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
    avg_results: f64,
    by_category: serde_json::Map<String, serde_json::Value>,
    by_difficulty: serde_json::Map<String, serde_json::Value>,
    results: Vec<QueryResult>,
}

fn ground_truth() -> Vec<GroundTruth> {
    vec![
        // === Core Data Structures (Easy) ===
        GroundTruth {
            id: "ds-1",
            query: "look up a key for a read operation",
            category: "data_structures",
            target_file: "src/db.c",
            target_function: "lookupKeyReadWithFlags",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "ds-2",
            query: "set a key value pair in a database",
            category: "data_structures",
            target_file: "src/db.c",
            target_function: "setKey",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "ds-3",
            query: "incremental rehash of a dictionary hash table",
            category: "data_structures",
            target_file: "src/dict.c",
            target_function: "dictRehash",
            difficulty: Difficulty::Medium,
        },
        GroundTruth {
            id: "ds-4",
            query: "add an entry to a hash table",
            category: "data_structures",
            target_file: "src/dict.c",
            target_function: "dictAdd",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "ds-5",
            query: "add an element to a sorted set",
            category: "data_structures",
            target_file: "src/t_zset.c",
            target_function: "zsetAdd",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "ds-6",
            query: "push an element onto a list",
            category: "data_structures",
            target_file: "src/t_list.c",
            target_function: "listTypePush",
            difficulty: Difficulty::Easy,
        },
        // === Replication / Persistence (Medium) ===
        GroundTruth {
            id: "rp-1",
            query: "serialize objects to RDB format",
            category: "persistence",
            target_file: "src/rdb.c",
            target_function: "rdbSaveObject",
            difficulty: Difficulty::Medium,
        },
        GroundTruth {
            id: "rp-2",
            query: "deserialize an object from an RDB file",
            category: "persistence",
            target_file: "src/rdb.c",
            target_function: "rdbLoadObject",
            difficulty: Difficulty::Medium,
        },
        GroundTruth {
            id: "rp-3",
            query: "flush the append only file buffer to disk",
            category: "persistence",
            target_file: "src/aof.c",
            target_function: "flushAppendOnlyFile",
            difficulty: Difficulty::Medium,
        },
        // === Networking (Medium) ===
        GroundTruth {
            id: "net-1",
            query: "create a new client connection",
            category: "networking",
            target_file: "src/networking.c",
            target_function: "createClient",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "net-2",
            query: "read command data from a client socket",
            category: "networking",
            target_file: "src/networking.c",
            target_function: "readQueryFromClient",
            difficulty: Difficulty::Medium,
        },
        GroundTruth {
            id: "net-3",
            query: "parse and process commands from client input buffer",
            category: "networking",
            target_file: "src/networking.c",
            target_function: "processInputBuffer",
            difficulty: Difficulty::Medium,
        },
        // === Pub/Sub & Clustering (Hard) ===
        GroundTruth {
            id: "ps-1",
            query: "publish a message to channel subscribers",
            category: "pubsub",
            target_file: "src/pubsub.c",
            target_function: "pubsubPublishMessage",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "ps-2",
            query: "subscribe a client to a pubsub channel",
            category: "pubsub",
            target_file: "src/pubsub.c",
            target_function: "pubsubSubscribeChannel",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "ps-3",
            query: "determine which cluster node should handle a command",
            category: "clustering",
            target_file: "src/cluster.c",
            target_function: "getNodeByQuery",
            difficulty: Difficulty::Hard,
        },
        // === Lua Scripting (Hard) ===
        GroundTruth {
            id: "lua-1",
            query: "execute a Lua EVAL command",
            category: "scripting",
            target_file: "src/eval.c",
            target_function: "evalGenericCommand",
            difficulty: Difficulty::Medium,
        },
        GroundTruth {
            id: "lua-2",
            query: "execute Redis commands from inside a Lua script",
            category: "scripting",
            target_file: "src/script_lua.c",
            target_function: "luaRedisGenericCommand",
            difficulty: Difficulty::Hard,
        },
        GroundTruth {
            id: "lua-3",
            query: "expose the Redis API to Lua scripting environment",
            category: "scripting",
            target_file: "src/script_lua.c",
            target_function: "luaRegisterRedisAPI",
            difficulty: Difficulty::Hard,
        },
        // === Bonus: Expiration (Hard) ===
        GroundTruth {
            id: "exp-1",
            query: "proactively expire keys in the background",
            category: "expiration",
            target_file: "src/expire.c",
            target_function: "activeExpireCycle",
            difficulty: Difficulty::Medium,
        },
        GroundTruth {
            id: "exp-2",
            query: "handle expiration of keys on access",
            category: "expiration",
            target_file: "src/db.c",
            target_function: "expireIfNeeded",
            difficulty: Difficulty::Medium,
        },
        // === Bonus: Skip List (Hard) ===
        GroundTruth {
            id: "sl-1",
            query: "insert a node into the sorted set skip list",
            category: "data_structures",
            target_file: "src/t_zset.c",
            target_function: "zslInsert",
            difficulty: Difficulty::Hard,
        },
        // === Bonus: Set/Hash (Easy) ===
        GroundTruth {
            id: "set-1",
            query: "add a member to a set",
            category: "data_structures",
            target_file: "src/t_set.c",
            target_function: "setTypeAdd",
            difficulty: Difficulty::Easy,
        },
        GroundTruth {
            id: "hash-1",
            query: "set a field in a hash",
            category: "data_structures",
            target_file: "src/t_hash.c",
            target_function: "hashTypeSet",
            difficulty: Difficulty::Easy,
        },
        // === Bonus: Replication (Medium) ===
        GroundTruth {
            id: "rep-1",
            query: "replicate write commands to slave nodes",
            category: "replication",
            target_file: "src/replication.c",
            target_function: "replicationFeedSlaves",
            difficulty: Difficulty::Medium,
        },
        // === Bonus: Cluster internals (Hard) ===
        GroundTruth {
            id: "cl-1",
            query: "process cluster gossip and heartbeat messages",
            category: "clustering",
            target_file: "src/cluster_legacy.c",
            target_function: "clusterProcessPacket",
            difficulty: Difficulty::Hard,
        },
    ]
}

fn main() {
    let db_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".sqmd/index.db"));

    if !db_path.exists() {
        eprintln!("Database not found at {:?}", db_path);
        eprintln!("Usage: sqmd-bench [path/to/index.db]");
        std::process::exit(1);
    }

    let db = sqmd_core::schema::open(&db_path).expect("Failed to open database");
    let queries = ground_truth();
    let total = queries.len();

    let mut results: Vec<QueryResult> = Vec::with_capacity(total);

    for gt in &queries {
        let search_query = sqmd_core::search::SearchQuery {
            text: gt.query.to_string(),
            top_k: 10,
            alpha: 0.7,
            source_type_filter: Some(vec!["code".to_string()]),
            ..Default::default()
        };

        let search_results = sqmd_core::search::fts_search(&db, &search_query).unwrap_or_default();

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
        let mut m = serde_json::Map::new();
        m.insert(
            "total".into(),
            serde_json::Value::Number(serde_json::Number::from(cat_results.len())),
        );
        m.insert("recall_at_10".into(), serde_json::Value::from(cat_recall));
        m.insert("mrr".into(), serde_json::Value::from(cat_mrr));
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
        avg_results,
        by_category,
        by_difficulty,
        results,
    };

    println!("{}", serde_json::to_string_pretty(&report).unwrap());
}
