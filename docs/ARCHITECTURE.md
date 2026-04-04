# Architecture

## System Overview

sqmd is a local-first code index built as a single Rust binary. It parses source files into semantically meaningful chunks, stores them in SQLite with vector embeddings, relationship metadata, and an entity knowledge graph. Serves hybrid search queries to AI agents in under 20ms.

## Components

```
+-------------------------------------------------------------+
|                      sqmd-cli                                |
|  init | index | watch | search | get | deps | context |      |
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
|  |          | |          | | graph    | | graph    |       |
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
|  |  WAL mode | soft-delete | decision pipeline        |    |
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
tree-sitter parse -> AST
    |
    v
AST walker -> extract declarations (functions, classes, types, imports)
    |
    +---> Each declaration -> Chunk { content_raw, signature, line range, metadata }
    |
    +---> Imports -> Relationship { source, target, type: "imports" }
    |
    v
Decision pipeline (content_hash):
    +---> SKIP (unchanged) -> skip
    +---> UPDATE (modified) -> re-chunk, update chunks/relationships
    +---> TOMBSTONE (deleted file) -> soft-delete, queue for purge
    |
    v
Insert into SQLite:
    +---> chunks table + chunks_fts (auto-sync via triggers)
    +---> relationships table
    +---> entities, entity_aspects, entity_attributes
    +---> hints + hints_fts (prospective indexing)
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
Query text
    +---> FTS5 MATCH -> keyword results [{id, rank}]
    +---> hints_fts MATCH -> prospective hint results [{id, score}]
    +---> entity graph -> graph_boost_ids [{chunk_id}]
    +---> (if embed) ONNX embed -> query vector -> sqlite-vec KNN
         |
         v
Normalize scores -> hybrid merge (alpha = 0.7)
    |
    v
Top-K results -> fetch context chunks
    |
    v
Optional: graph traversal (dependency expansion)
    |
    v
Token-count -> trim to budget
    |
    v
Assemble Markdown document -> return to agent
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
    +---> Deleted   -> decision pipeline (TOMBSTONE)
    |
    v
sqmd prune --days N -> purge old tombstones
```

## SQLite Schema

See [`schema.sql`](./schema.sql) for the base DDL. Schema v3 adds entity and soft-delete tables.

### Tables

| Table | Purpose |
|-------|---------|
| `files` | Source file metadata (path, language, hash, mtime) |
| `chunks` | Semantic code chunks (content_raw, signature, line range, metadata) |
| `chunks_fts` | FTS5 virtual table for keyword search |
| `relationships` | Import/call/contains graph edges between chunks |
| `chunks_vec` | sqlite-vec virtual table (vector KNN search) |
| `entities` | Entity knowledge graph nodes (files, structs, functions, etc.) |
| `entity_aspects` | Entity facets (exports, implementation, constraints) |
| `entity_attributes` | Chunk-level entity annotations with kind + content |
| `entity_dependencies` | Entity-level dependency edges with mention counts |
| `hints` | Prospective search hints bridging semantic gap |
| `hints_fts` | FTS5 on hints for natural-language query matching |

### Key Design Decisions

1. **WAL mode** -- Enables concurrent reads during writes. Agent queries never block indexing.

2. **Content hash dedup** -- SHA-256 of the raw source text for each chunk. Decision pipeline uses this to SKIP unchanged chunks on re-index.

3. **FTS5 content-sync triggers** -- `chunks_fts` stays in sync with `chunks` automatically via INSERT/UPDATE/DELETE triggers.

4. **sqlite-vec vector search** -- `chunks_vec` provides fast KNN on 768-dim embeddings. Feature-gated behind `--features embed`.

5. **Relationships as first-class data** -- Import, call, and contains relationships extracted during chunking. Stored explicitly for fast recursive CTE traversal.

6. **Raw code, not Markdown** -- `content_raw` stores original source. Markdown derived on demand via `Chunk::render_md()`.

7. **Entity knowledge graph** -- Three-level model: entities, aspects, attributes. Bridges structural code understanding with natural-language queries via hints.

8. **Soft-delete with retention** -- Deleted files are tombstoned (not hard-deleted). `sqmd prune --days N` purges old tombstones. Prevents accidental data loss during re-indexes.

9. **Prospective hint indexing** -- Natural-language hints generated for each chunk (e.g., "how does authenticate work") and indexed in FTS5. Bridges the semantic gap between code names and natural-language queries.

10. **Structural importance** -- Graph density (in-degree, contains count, constraint count) boosts chunk importance scores. Highly-depended-upon code ranks higher in search.

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
- **TOMBSTONE** -- file deleted, soft-delete all chunks

### Entity Model

Three-level knowledge graph:
1. **Entities** -- Files, structs, classes, functions (deduped by canonical name)
2. **Aspects** -- Facets like "exports", "implementation", "constraints"
3. **Attributes** -- Chunk-level annotations linking entities to code chunks

### Graph Boost

Search results are boosted by entity graph density. Chunks belonging to highly-depended-upon entities rank higher. The boost expands transitively: if "AuthModule" imports "DatabasePool", both get boosted.

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
|   |   |   +-- schema.rs          # SQLite DDL + migrations (v3)
|   |   |   +-- chunk.rs           # Chunk struct + ChunkType + render_md()
|   |   |   +-- chunker.rs         # LanguageChunker trait + FileChunker fallback
|   |   |   +-- index.rs           # Transactional indexer + decision pipeline
|   |   |   +-- entities.rs        # Entity/aspect/attribute model + hints + graph boost
|   |   |   +-- embed.rs           # ONNX embedding (ort) + BPE tokenizer
|   |   |   +-- search.rs          # FTS5 + hints + vector + hybrid search
|   |   |   +-- relationships.rs   # Import resolution + call graph + CTE traversal
|   |   |   +-- context.rs         # Token-budgeted context assembly
|   |   |   +-- vfs.rs             # Virtual file system: list, get, diff, tree
|   |   |   +-- daemon.rs          # Unix socket daemon + JSON protocol
|   |   |   +-- watcher.rs         # File watcher + debounce
|   |   |   +-- files.rs           # Language detection + file walking + hashing
|   |   |   +-- languages/
|   |   |       +-- typescript.rs  # TS/TSX chunker + import extraction
|   |   |       +-- rust.rs        # Rust chunker + use/impl extraction
|   |   |       +-- python.rs      # Python chunker + import extraction
|   |   |       +-- go.rs          # Go chunker + func/type/import extraction
|   |   |       +-- java.rs        # Java chunker + class/interface/enum
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
+-- .sqmd/                         # (gitignored) runtime data
    +-- index.db
```
