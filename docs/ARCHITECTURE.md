# Architecture

## System Overview

sqmd is a local-first code index built as a single Rust binary. It parses source files into semantically meaningful chunks, stores them in SQLite with vector embeddings, relationship metadata, and an entity knowledge graph. Serves hybrid search queries to AI agents in under 20ms. All query responses include a pre-rendered `markdown` field for direct agent prompt injection.

## Components

```
+-------------------------------------------------------------+
|                      sqmd-cli                                |
|  init | index | watch | search | get | cat | deps | context|
|  entities | entity-deps | prune | serve | stats              |
+---------------------------+---------------------------------+
                            |
+---------------------------v---------------------------------+
|                     sqmd-core                                |
|                                                              |
|  +----------+ +----------+ +----------+ +----------+       |
|  | chunker  | | embedder | |  search  | |  graph   |       |
|  |          | |          | |          | |          |       |
|  |tree-sitter| |  ONNX    | | FTS5 +   | | import   |       |
|  | per-lang | | nomic    | | hints +  | | call     |       |
|  | AST walk | | q8 model | | vec0 +   | | contains |       |
|  |          | |          | | graph +  | | graph    |       |
|  |          | |          | | cache    | |          |       |
|  |          | |          | | markdown | |          |       |
|  +----+-----+ +----+-----+ +----+-----+ +----+-----+       |
|       |             |             |             |            |
|  +----v-----------+-v-------------v-------------v-------+   |
|  |                   SQLite                           |    |
|  |                                                    |    |
|  |  files | chunks | chunks_fts | chunks_vec          |    |
|  |  relationships | hints | hints_fts                |    |
|  |  entities | entity_aspects | entity_attributes     |    |
|  |  entity_dependencies                             |    |
|  |                                                    |    |
|  |  WAL mode | mmap | soft-delete | decision pipeline |    |
|  +----------------------------------------------------+    |
+-------------------------------------------------------------+
```

## Data Flow

### Indexing

```
Source file (e.g., src/auth.ts)
    |
    v
Language detection (extension -> tree-sitter grammar)
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
    v (if embed feature enabled)
ONNX embed -> store in chunks_vec
```

### Querying

```
Agent query: "how does authentication work"
    |
    v
Query cache (LRU, 256 entries, 10s TTL)
    +---> HIT: return cached results (structured + markdown)
    +---> MISS: continue
    |
    v
BEGIN (read-consistent snapshot)
    |
    +---> FTS5 MATCH (porter-stemmed) -> keyword results
    +---> hints_fts MATCH (porter-stemmed) -> prospective hint results
    |       +---> batch hint merge (single IN (...) query)
    +---> entity graph -> graph_boost_ids (batch entity LIKE + recursive CTE)
    +---> (if embed) ONNX embed -> query vector -> sqlite-vec KNN
    |
    v
Normalize + alpha-blend (default 70% vector / 30% keyword)
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

### Single-Chunk Queries (cat, get)

```
cat 42 / get src/auth.rs:42
    |
    v
Fetch chunk from DB (id or file:line lookup)
    |
    v
Build Chunk struct from DB row
    |
    v
Chunk::render_md() -> markdown string
    |
    v
JSON response: { ...fields..., "content": "...", "markdown": "..." }
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

See [`schema.sql`](./schema.sql) for the base DDL. Schema v5 adds Porter stemming for FTS5.

### Tables

| Table | Purpose |
|-------|---------|
| `files` | Source file metadata (path, language, hash, mtime) |
| `chunks` | Semantic code chunks (content_raw, signature, line range, metadata) |
| `chunks_fts` | FTS5 virtual table for keyword search (porter + unicode61) |
| `relationships` | Import/call/contains graph edges between chunks |
| `chunks_vec` | sqlite-vec virtual table (vector KNN search) |
| `entities` | Entity knowledge graph nodes (files, structs, functions, etc.) |
| `entity_aspects` | Entity facets (exports, implementation, constraints) |
| `entity_attributes` | Chunk-level entity annotations with kind + content |
| `entity_dependencies` | Entity-level dependency edges with mention counts |
| `hints` | Prospective search hints bridging semantic gap |
| `hints_fts` | FTS5 on hints for natural-language query matching (porter + unicode61) |

### Key Design Decisions

1. **WAL mode + mmap** -- 256MB mmap for reads, WAL autocheckpoint at 1000, 8000-page cache. Agent queries never block indexing.

2. **Content hash dedup** -- SHA-256 of the raw source text for each chunk. Used consistently for both code chunks and knowledge chunks. Decision pipeline uses this to SKIP unchanged chunks on re-index.

3. **FTS5 with Porter stemming** -- `chunks_fts` and `hints_fts` use `tokenize='porter unicode61'`. Improves recall for inflected terms (authenticate/authenticating/authenticated all match).

4. **sqlite-vec vector search** -- `chunks_vec` provides fast KNN on 768-dim embeddings. Feature-gated behind `--features embed`.

5. **Relationships as first-class data** -- Import, call, and contains relationships extracted during chunking. Stored explicitly for fast recursive CTE traversal.

6. **Raw code, not Markdown** -- `content_raw` stores original source. Markdown derived on demand via `Chunk::render_md()`. Every query response includes a `markdown` field pre-rendered for agent consumption.

7. **Entity knowledge graph** -- Three-level model: entities, aspects, attributes. Bridges structural code understanding with natural-language queries via hints.

8. **Soft-delete with retention** -- Deleted files are tombstoned (not hard-deleted). `sqmd prune --days N` purges old tombstones. Batch cleanup via `IN (...)` queries.

9. **Prospective hint indexing** -- Natural-language hints generated for each chunk and indexed in FTS5. Bridges the semantic gap between code names and natural-language queries.

10. **Structural importance** -- Graph density (in-degree, contains count, constraint count) boosts chunk importance scores. Highly-depended-upon code ranks higher in search.

11. **Asymmetric retrieval** -- Query embeddings use `search_query:` prefix and document embeddings use `search_document:` prefix (nomic-embed-text-v1.5).

12. **Real batch ONNX inference** -- `embed_batch` stacks inputs into `[N, seq_len]` tensors for a single forward pass. Padded to nearest multiple-of-64.

13. **Temporal decay** -- Knowledge chunks can have a `decay_rate` (exponential decay per day since `last_accessed`). Search scores multiplied by decay factor, clamped to [0.1, 1.0].

14. **Filter parity** -- Both FTS5 and vector search respect the same filter set: `file`, `type`, `source_type`, `agent_id`.

15. **Multi-threaded daemon** -- Each client connection spawns its own thread. Read handlers use `open_fast()` (read-only, no migration check). Write handlers use `open()`. Shared `DaemonState` with `Arc<Mutex>` for query cache and embedder.

16. **Query cache** -- LRU cache (256 entries, 10s TTL) deduplicates repeated agent searches within the TTL window.

17. **Read-consistent snapshots** -- FTS search runs inside `BEGIN`/`COMMIT` so all phases see the same data state.

## Embedding Model

**Model:** `nomic-embed-text-v1.5` (q8 quantized)
**Dimensions:** 768
**Size:** ~50MB (model) + tokenizer
**Runtime:** ONNX Runtime (ort crate)
**Cache:** `~/.sqmd/models/`

Feature-gated behind `--features embed`. Default binary is ~10MB; embed binary is ~27MB.

## Pipeline Intelligence

### Decision Pipeline

Each file goes through a 3-way decision on re-index:
- **SKIP** -- content hash matches, zero mutations
- **UPDATE** -- content changed, re-chunk and update
- **TOMBSTONE** -- file deleted, soft-delete all chunks (batch `IN (...)`)

### Entity Model

Three-level knowledge graph:
1. **Entities** -- Files, structs, classes, functions (deduped by canonical name)
2. **Aspects** -- Facets like "exports", "implementation", "constraints"
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
| Hybrid search | <20ms | Combined + merge + graph boost |
| Graph traversal (depth 2) | <50ms | Recursive CTE |
| Context assembly | <5ms | Token-count + trim |

## File Structure

```
sqmd/
+-- Cargo.toml
+-- crates/
|   +-- sqmd-core/
|   |   +-- src/
|   |   |   +-- lib.rs
|   |   |   +-- schema.rs          # SQLite DDL + migrations (v5, porter stemmer)
|   |   |   +-- chunk.rs           # Chunk struct + ChunkType + SourceType + render_md()
|   |   |   +-- chunker.rs         # LanguageChunker trait + FileChunker fallback
|   |   |   +-- index.rs           # Transactional indexer + decision pipeline + KnowledgeIngestor
|   |   |   +-- entities.rs        # Entity/aspect/attribute model + hints + graph boost
|   |   |   +-- embed.rs           # ONNX embedding (ort) + BPE tokenizer + auto-download
|   |   |   +-- search.rs          # FTS5 + hints + vector + hybrid + decay + render_search_markdown
|   |   |   +-- relationships.rs   # Import resolution + call graph + CTE traversal
|   |   |   +-- context.rs         # Token-budgeted context assembly
|   |   |   +-- dampening.rs       # Diversity dampening + importance boost
|   |   |   +-- query_cache.rs     # LRU query cache (256 entries, 10s TTL)
|   |   |   +-- vfs.rs             # Virtual file system: list, get, diff, tree
|   |   |   +-- daemon.rs          # Unix socket daemon + JSON protocol + shared embedder
|   |   |   +-- watcher.rs         # File watcher + debounce
|   |   |   +-- files.rs           # Language detection + file walking + hashing
|   |   |   +-- languages/
|   |   |       +-- typescript.rs  # TS/TSX chunker + import extraction
|   |   |       +-- rust.rs        # Rust chunker + use/impl extraction
|   |   |       +-- python.rs      # Python chunker + import extraction
|   |   |       +-- go.rs          # Go chunker + func/type/import extraction
|   |   |       +-- java.rs        # Java chunker + class/interface/enum
|   |   +-- tests/
|   |   |   +-- knowledge_integration.rs
|   |   +-- Cargo.toml
|   +-- sqmd-cli/
|       +-- src/
|       |   +-- main.rs            # CLI commands
|       +-- Cargo.toml
+-- docs/
|   +-- ROADMAP.md
|   +-- ARCHITECTURE.md
|   +-- WHAT_IT_IS.md
|   +-- schema.sql
+-- .github/workflows/rust.yml
```
