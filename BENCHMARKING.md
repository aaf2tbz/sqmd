# Benchmarking

sqmd includes a benchmark harness (`sqmd-bench`) for measuring retrieval quality against real codebases.

## Latest Results

### Signet Codebase — v3.0.0 (native llama.cpp)

| Metric | FTS | Layered |
|--------|-----|---------|
| Hit@1 | 86% | 86% |
| Hit@3 | 97.5% | 97.5% |
| Hit@5 | 98.5% | 98.5% |
| Hit@10 | 99.5% | 99.5% |
| MRR | 0.915 | 0.915 |

**Dataset**: 505 TypeScript files, 8,886 chunks, 3,547 relationships, 200 queries
**Embedding model**: mxbai-embed-large (1024-dim) via native llama.cpp
**Performance**: ~0.55s per query, ~19 q/sec
**Date**: 2026-04-11

### Metrics

- **Hit@K**: Is the target chunk present in the top K results? (K = 1, 3, 5, 10)
- **MRR** (Mean Reciprocal Rank): Average of `1/rank` across all queries. A perfect score of 1.0 means every target was ranked first.

### Query Generation

Two approaches:

1. **Name-derived queries** (fast, no LLM needed): Randomly sample function/method/class/interface names from indexed chunks, lowercase with spaces replacing separators. These are FTS-friendly and test exact/near-exact recall.

2. **LLM-generated queries** (slow, requires Ollama): Use `sqmd-bench generate` with a running Ollama instance to produce natural-language queries describing what each chunk does. Better for testing semantic retrieval.

### Evaluation

Each query targets a specific chunk (the chunk whose name generated the query). The search function returns top-K results, and we check if the target chunk appears in those results at any position.

## Running Benchmarks

### Prerequisites

```bash
# Build bench with native feature
cargo build -p sqmd-bench --features native --release
```

### Generate Queries

```bash
# Name-derived queries (fast, no LLM)
sqlite3 /path/to/index.db "
SELECT json_group_array(json_object(
  'eval_query', q, 'chunk_id', cid, 'file_path', fp,
  'name', n, 'chunk_type', ct,
  'content_preview', SUBSTR(COALESCE(content_raw,''),1,120),
  'language', COALESCE(language,'unknown')
))
FROM (
  SELECT LOWER(REPLACE(REPLACE(REPLACE(REPLACE(name,'_',' '),'-',' '),'.',' '),'$',' ')) AS q,
    id AS cid, file_path AS fp, name AS n, chunk_type AS ct
  FROM chunks WHERE name IS NOT NULL
    AND chunk_type IN ('function','method','class','interface')
    AND is_deleted = 0 AND LENGTH(name) > 3
    AND source_type = 'code'
  ORDER BY RANDOM() LIMIT 200
);" > queries.json

# LLM-generated queries (slow, requires ollama feature)
cargo run -p sqmd-bench --features native,ollama -- generate /path/to/index.db --output queries.json
```

### Run Comparison

```bash
# FTS vs Layered head-to-head
cargo run -p sqmd-bench --features native -- compare /path/to/index.db --ground-truth queries.json

# Single lane
cargo run -p sqmd-bench --features native -- run /path/to/index.db layered
cargo run -p sqmd-bench --features native -- run /path/to/index.db fts
```

### Expected Output

```json
{
  "total_queries": 200,
  "lanes": {
    "fts": {
      "hit_at_1": 0.86,
      "hit_at_3": 0.975,
      "hit_at_5": 0.985,
      "hit_at_10": 0.995,
      "mrr": 0.915
    },
    "layered": {
      "hit_at_1": 0.86,
      "hit_at_3": 0.975,
      "hit_at_5": 0.985,
      "hit_at_10": 0.995,
      "mrr": 0.915
    }
  }
}
```

## Reproducing the Signet Benchmark

```bash
# 1. Copy source files from signet packages
mkdir -p /path/to/signet-bench
cp -R ~/signetai/packages/daemon/src /path/to/signet-bench/daemon-src
cp -R ~/signetai/packages/core/src /path/to/signet-bench/core-src
cp -R ~/signetai/packages/cli/src /path/to/signet-bench/cli-src
cp -R ~/signetai/packages/sdk/src /path/to/signet-bench/sdk-src
cp -R ~/signetai/packages/connector-opencode/src /path/to/signet-bench/connector-opencode-src

# 2. Index
cd /path/to/signet-bench
sqmd init
sqmd index

# 3. Embed (may take 20-30 min for ~9000 chunks)
sqmd embed

# 4. Generate queries and run
# (use the SQL query from above)
cargo run -p sqmd-bench --features native -- compare .sqmd/index.db --ground-truth queries.json
```
