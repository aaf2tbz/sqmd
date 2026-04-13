# Architecture

## System Overview

sqmd is a local-first code index built as a single Rust binary. It parses source files into semantically meaningful chunks, stores them in SQLite with vector embeddings, relationship metadata, and an entity knowledge graph. Serves hybrid search queries to AI agents via MCP (JSON-RPC over stdio) or a Unix socket daemon. All query responses include a pre-rendered `markdown` field for direct agent prompt injection.

## Components

```
+-----------------------------------------------------------------+
|                          sqmd-cli                                |
|  init | index | watch | search | get | cat | deps | context     |
|  entities | entity-deps | prune | serve | stats | hints        |
|  start | stop | mcp | setup | doctor | update | install | reset |
|  diff | ls                                                  |
+---------------------------+-------------------------------------+
                            |
+---------------------------v-------------------------------------+
|                       sqmd-core                                 |
|                                                                 |
|  +----------+ +----------+ +----------+ +----------+           |
|  | chunker  | | embedder | |  search  | |  graph   |           |
|  |          | |          | |          | |          |           |
|  |tree-sitter| | llama   | | FTS5 +   | | import   |           |
|  | 17 langs | | cpp-2   | | hints +  | | call     |           |
|  | + 21    | | native  | | vec0 +   | | contains |           |
|  | line-   | | mxbai   | | hint_vec | | graph    |           |
|  | based   | | phi4    | | graph +  | |          |           |
|  | fallback| | Metal   | | comm.    | |          |           |
|  +----+-----+ +----+-----+ +----+-----+ +----+-----+           |
|       |             |             |             |                |
|  +----v-----------+-v-------------v-------------v-------+       |
|  |                   SQLite                             |       |
|  |                                                      |       |
|  |  files | chunks | chunks_fts | chunks_vec            |       |
|  |  relationships | hints | hints_fts | hints_vec      |       |
|  |  entities | entity_aspects | entity_attributes       |       |
|  |  entity_dependencies | communities | episodes         |       |
|  |  embeddings | schema_version                       |       |
|  |                                                      |       |
|  |  WAL mode | mmap | soft-delete | decision pipeline   |       |
|  +------------------------------------------------------+       |
+-----------------------------------------------------------------+
       |                                          |
+------v------+                              +----v-------+
| MCP server |                              | sqmd-bench  |
| JSON-RPC   |                              |            |
| stdio      |                              | run        |
| 12 tools   |                              | generate   |
+------------+                              | compare    |
                                            +------------+
```

## Data Flow

### Indexing

```
Source file (e.g., src/auth.ts)
    |
    v
Language detection (extension -> Language enum -> tree-sitter grammar)
    |
    v
tree-sitter parse -> AST    [parse ONCE]
    |
    v
AST walker -> extract declarations (functions, classes, types, imports)
    |                           [reuse Tree for import extraction]
    +---> Each declaration -> Chunk { content_raw, signature, line range, metadata }
    |
    +---> Imports -> Relationship { source, target, type: "imports" }
    |
    v
Decision pipeline (content_hash):
    +---> SKIP (unchanged) -> skip
    +---> UPDATE (modified) -> re-chunk, update chunks/relationships
    +---> TOMBSTONE (deleted file) -> soft-delete, queue for purge
    |        [batch IN (...) queries for hints, entity_attributes, relationships]
    |
    v
Insert into SQLite:
    +---> chunks table + chunks_fts (auto-sync via triggers, porter-stemmed)
    +---> relationships table
    +---> entities, entity_aspects, entity_attributes
    +---> hints + hints_fts (prospective indexing, porter-stemmed)
    +---> structural importance (graph density)
    |
    +---> [if native feature] Generate LLM prospective hints (phi4-mini)
    |        -> hints with type='prospective' (importance >= 0.5 only)
    |
    v (if native feature enabled)
llama.cpp embed -> store in chunks_vec + hints_vec
```

### Querying (MCP)

```
Agent query: "how does authentication work"
    |
    v
MCP server (JSON-RPC 2.0 over stdio, framed or raw)
    |
    v
BEGIN (read-consistent snapshot)
    |
    +---> FTS5 MATCH (porter-stemmed) -> keyword results
    +---> hints_fts MATCH (porter-stemmed) -> prospective hint results
    |       +---> batch hint merge (single IN (...) query)
    +---> entity graph -> graph_boost_ids (batch entity LIKE + recursive CTE)
    +---> (if native) llama.cpp embed -> query vector
    |       +---> sqlite-vec KNN on chunks_vec -> content vector results
    |       +---> sqlite-vec KNN on hints_vec -> hint vector results
    |
    v
Layered merge (score multipliers per layer)
    |
    v
importance_boost()     (0.7x-1.0x based on chunk importance field)
dampen()               (same-file penalty: 3rd from same file -> 85%)
    |
    v
COMMIT
    |
    v
render_search_markdown() (batch-fetch content_raw, render via Chunk::render_md)
    |
    v
JSON response:
    [
      {
        "chunk_id": 42,
        "file_path": "src/auth.rs",
        "name": "authenticate",
        "score": 0.92,
        ...                  -- structured fields for tooling
        "markdown": "<document>..."  -- ready for agent prompt
      }
    ]
    |
    +---> agents: grab "markdown" directly -> inject into prompt
    +---> tooling: use structured fields (chunk_id, file_path, line_start, ...)
```

### Incremental Update

```
File watcher (notify crate)
    |
    v
File changed (debounced 200ms)
    |
    v
SHA-256 hash comparison vs files table
    |
    +---> Unchanged -> skip (0 mutations)
    +---> Modified  -> decision pipeline (UPDATE)
    +---> Deleted   -> decision pipeline (TOMBSTONE, batch cleanup)
    |
    v
sqmd prune --days N -> purge old tombstones
```

## SQLite Schema

The migration system in `schema.rs` manages upgrades through v14.

### Tables

| Table | Purpose | Schema Version |
|-------|---------|---------------|
| `files` | Source file metadata (path, language, size, hash, mtime) | v1 |
| `chunks` | Semantic code chunks (content_raw, signature, line range, metadata) | v1 |
| `chunks_fts` | FTS5 virtual table for keyword search (porter + unicode61) | v2 |
| `chunks_vec` | sqlite-vec virtual table (1024-dim vector KNN, float32) | v2 |
| `relationships` | Import/call/contains graph edges between chunks | v1 |
| `entities` | Entity knowledge graph nodes (files, structs, functions, etc.) | v3 |
| `entity_aspects` | Entity facets (exports, implementation, constraints) with weights | v3 |
| `entity_attributes` | Chunk-level entity annotations with kind + content | v3 |
| `entity_dependencies` | Entity-level dependency edges with temporal validity | v3+v7 |
| `hints` | Prospective search hints (template + LLM-generated) | v3 |
| `hints_fts` | FTS5 on hints for natural-language query matching | v3 |
| `hints_vec` | sqlite-vec virtual table (1024-dim vector KNN over hints) | v12 |
| `communities` | Directory, module, and type-hierarchy groupings | v6 |
| `episodes` | Change provenance tracking | v8 |
| `embeddings` | Vector blob storage (fallback alongside chunks_vec) | v2 |
| `schema_version` | Migration version tracking | v1 |

### Key Design Decisions

1. **WAL mode + mmap** -- 256MB mmap for reads, WAL autocheckpoint at 1000, 8000-page cache. Agent queries never block indexing.

2. **Content hash dedup** -- SHA-256 of the raw source text for each chunk. Used consistently for both code chunks and knowledge chunks. Decision pipeline uses this to SKIP unchanged chunks on re-index.

3. **FTS5 with Porter stemming** -- `chunks_fts` and `hints_fts` use `tokenize='porter unicode61'`. Improves recall for inflected terms (authenticate/authenticating/authenticated all match).

4. **sqlite-vec vector search** -- `chunks_vec` provides fast KNN on 1024-dim embeddings via mxbai-embed-large. `hints_vec` (schema v12) provides the same for hint text. Both gated behind the `native` feature.

5. **Relationships as first-class data** -- Import, call, and contains relationships extracted during chunking. Stored explicitly for fast recursive CTE traversal.

6. **Raw code, not Markdown** -- `content_raw` stores original source. Markdown derived on demand via `Chunk::render_md()`. Every query response includes a `markdown` field pre-rendered for agent consumption.

7. **Entity knowledge graph** -- Three-level model: entities, aspects, attributes. Bridges structural code understanding with natural-language queries via hints.

8. **Soft-delete with retention** -- Deleted files are tombstoned (not hard-deleted). `sqmd prune --days N` purges old tombstones. Batch cleanup via `IN (...)` queries.

9. **Prospective hint indexing** -- Template-based hints generated for each chunk at index time. LLM-generated hints (via native llama.cpp, phi4-mini) added when the `native` feature is enabled. Both indexed in FTS5 and in `hints_vec`.

10. **Structural importance** -- Graph density (in-degree, contains count, constraint count) boosts chunk importance scores. Highly-depended-upon code ranks higher in search.

11. **Native llama.cpp inference** -- All LLM inference runs through llama.cpp. mxbai-embed-large (1024 dims) for embeddings, phi4-mini for hint generation. Metal GPU offloading on Apple Silicon. No ONNX, no Ollama dependency.

12. **Temporal decay** -- Knowledge chunks can have a `decay_rate` (exponential decay per day since `last_accessed`). Search scores multiplied by decay factor, clamped to [0.1, 1.0].

13. **Filter parity** -- Both FTS5 and vector search respect the same filter set: `file`, `type`, `source_type`, `agent_id`.

14. **Multi-threaded daemon** -- Each client connection spawns its own thread. Read handlers use `open_fast()` (read-only, no migration check). Write handlers use `open()`. Shared `DaemonState` with `Arc<Mutex>` for query cache and embedder.

15. **Query cache** -- In-memory cache (100 entries, 10s TTL) deduplicates repeated agent searches within the TTL window.

16. **Read-consistent snapshots** -- FTS search runs inside `BEGIN`/`COMMIT` so all phases see the same data state.

17. **MCP server** -- JSON-RPC 2.0 over stdio supporting both `Content-Length:` framed and raw JSON line-delimited transport. Falls back to CWD for project root when `.sqmd/` is at `~/.sqmd/` to prevent broken relative path resolution.

## Embedding Model

**Model:** `mxbai-embed-large` (GGUF format)
**Dimensions:** 1024
**Runtime:** llama.cpp via `llama-cpp-2` crate
**GPU:** Metal acceleration on Apple Silicon (`native-metal` feature)
**Cache:** `~/.ollama/models/` (auto-discovered) or `SQMD_NATIVE_MODEL`

Feature-gated behind `native`.

## LLM Hint Generation

**Model:** `phi4-mini` (configurable via `SQMD_HINT_MODEL`)
**Runtime:** llama.cpp via `llama-cpp-2` crate (same native runtime as embeddings)
**Config:** `SQMD_HINT_MODEL_PATH` for direct GGUF path

Feature-gated behind `native`. Generates prospective hints per chunk at index time for chunks with `importance >= 0.5`. Hints stored with `hint_type='prospective'` for search routing.

## Pipeline Intelligence

### Decision Pipeline

Each file goes through a 3-way decision on re-index:
- **SKIP** -- content hash matches, zero mutations
- **UPDATE** -- content changed, re-chunk and update
- **TOMBSTONE** -- file deleted, soft-delete all chunks (batch `IN (...)`)

### Hint Generation Pipeline

Hints are generated at two levels:

1. **Template hints** (always) -- Static patterns: "how does {name} work", "{name} implementation", "code in {filename}". For memory chunks: proper nouns, quoted strings, date patterns, key noun phrases.

2. **LLM prospective hints** (optional, `native` feature) -- Natural-language queries the LLM predicts someone would search for. Prompted with chunk content. More effective for indirect/paraphrased searches.

3. **Relational hints** (always) -- Generated from entity graph: "X implements Y", "X contains Y", "X calls Y", plus reverse directions. Typed with `hint_type` for search routing.

### Entity Model

Three-level knowledge graph:
1. **Entities** -- Files, structs, classes, functions (deduped by canonical name)
2. **Aspects** -- Facets like "exports", "implementation", "constraints" (with weights)
3. **Attributes** -- Chunk-level annotations linking entities to code chunks

### Graph Boost

Search results boosted by entity graph density. Chunks belonging to highly-depended-upon entities rank higher. Boost expands transitively.

## Performance Budget

| Operation | Budget | Notes |
|-----------|--------|-------|
| Cold model load | <2s | One-time per daemon lifetime |
| Per-file parse + chunk | <50ms | tree-sitter is fast |
| Per-file embed | <10ms | After model is loaded |
| Incremental re-index | <200ms | Single file change |
| FTS5 query | <3ms | Keyword search |
| Hint query | <5ms | Prospective FTS5 match |
| Vector search | <5ms | 100k chunks |
| Hint vector search | <5ms | Parallel with content vector |
| Hybrid search | <20ms | Combined + merge + graph boost + hint vec |
| Graph traversal (depth 2) | <50ms | Recursive CTE |
| Context assembly | <5ms | Token-count + trim |

## File Structure

```
sqmd/
+-- Cargo.toml
+-- README.md
+-- CHANGELOG.md
+-- CONTRIBUTING.md
+-- BENCHMARKING.md
+-- crates/
|   +-- sqmd-core/
|   |   +-- src/
|   |   |   +-- lib.rs
|   |   |   +-- schema.rs          # SQLite DDL + migrations (v1-v14)
|   |   |   +-- chunk.rs           # Chunk struct + ChunkType + SourceType + render_md()
|   |   |   +-- chunker.rs         # LanguageChunker trait + FileChunker fallback
|   |   |   +-- index.rs           # Transactional indexer + decision pipeline
|   |   |   +-- entities.rs        # Entity/aspect/attribute model + hints + graph boost
|   |   |   +-- embed.rs           # EmbedProvider trait + vector encoding + cosine similarity
|   |   |   +-- native.rs          # llama.cpp runtime: mxbai-embed-large + phi4-mini
|   |   |   +-- search.rs          # FTS5 + hints + hint_vec + vector + layered + render
|   |   |   +-- relationships.rs   # Import resolution + call graph + CTE traversal
|   |   |   +-- context.rs         # Token-budgeted context assembly
|   |   |   +-- dampening.rs       # Diversity dampening + importance boost
|   |   |   +-- query_cache.rs     # In-memory query cache (100 entries, 10s TTL)
|   |   |   +-- mcp_server.rs      # MCP JSON-RPC server (stdio, 12 tools)
|   |   |   +-- daemon.rs          # Unix socket daemon + JSON protocol + shared embedder
|   |   |   +-- watcher.rs         # File watcher + debounce
|   |   |   +-- files.rs           # Language detection (38 variants) + file walking + hashing
|   |   |   +-- vfs.rs             # Virtual file system: list, get, diff, tree
|   |   |   +-- communities.rs     # Directory, module, and type-hierarchy communities
|   |   |   +-- episodes.rs        # Change provenance tracking
|   |   |   +-- languages/
|   |   |       +-- typescript.rs  # TS/TSX chunker + import extraction
|   |   |       +-- rust.rs        # Rust chunker + use/impl extraction
|   |   |       +-- python.rs      # Python chunker + import extraction
|   |   |       +-- go.rs          # Go chunker + func/type/import extraction
|   |   |       +-- java.rs        # Java chunker + class/interface/enum
|   |   |       +-- c.rs           # C chunker + #include extraction
|   |   |       +-- cpp.rs         # C++ chunker + template/namespace support
|   |   |       +-- html.rs        # HTML chunker (semantic elements, script/style)
|   |   |       +-- css.rs         # CSS chunker (rule_set, at-rules)
|   |   |       +-- ruby.rs        # Ruby chunker + require extraction
|   |   |       +-- cmake.rs       # CMake chunker + target/dependency extraction
|   |   |       +-- qml.rs         # QML chunker + component extraction
|   |   |       +-- meson.rs       # Meson regex chunker
|   |   |       +-- markdown.rs    # Markdown regex chunker (headings)
|   |   |       +-- json.rs        # JSON chunker (keyed pairs)
|   |   |       +-- yaml.rs        # YAML chunker (keyed mappings)
|   |   |       +-- toml.rs        # TOML chunker (tables, arrays, pairs)
|   |   +-- Cargo.toml             # features: native, native-metal
|   +-- sqmd-cli/
|   |   +-- src/
|   |   |   +-- main.rs            # CLI commands (24 subcommands)
|   |   +-- Cargo.toml             # features: native, native-metal (default: native-metal)
|   +-- sqmd-bench/
|       +-- src/
|       |   +-- main.rs            # Benchmark: run, generate, compare subcommands
|       +-- Cargo.toml             # features: native, native-metal (default: native-metal)
+-- docs/
|   +-- ARCHITECTURE.md
|   +-- ROADMAP.md
|   +-- WHAT_IT_IS.md
|   +-- design/
|       +-- entity-graph-redesign.md
+-- skills/
|   +-- sqmd-review/
|       +-- SKILL.md
|       +-- references/
|           +-- review-schema.md
+-- references/
|   +-- memorybench/              # External reference submodule
+-- .github/workflows/
    +-- rust.yml                   # CI: build + test + clippy
    +-- bump-version.yml           # Auto-bump from CHANGELOG
    +-- release.yml                # GitHub Release on version bump
```
