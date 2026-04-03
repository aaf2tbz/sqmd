# Architecture

## System Overview

sqmd is a local-first code index built as a single Rust binary. It parses source files into semantically meaningful Markdown chunks, stores them in SQLite with vector embeddings and relationship metadata, and serves hybrid search queries to AI agents in under 20ms.

## Components

```
┌─────────────────────────────────────────────────────────┐
│                      sqmd-cli                           │
│  init | index | watch | search | get | deps | stats     │
└──────────────────────┬──────────────────────────────────┘
                       │
┌──────────────────────▼──────────────────────────────────┐
│                     sqmd-core                           │
│                                                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌────────┐ │
│  │ chunker  │  │ embedder │  │  search  │  │ graph  │ │
│  │          │  │          │  │          │  │        │ │
│  │tree-sitter│  │  ONNX    │  │ FTS5 +   │  │ import │ │
│  │ per-lang │  │ nomic    │  │ sqlite-vec│  │ call   │ │
│  │ AST walk │  │ q8 model │  │ hybrid   │  │ graph  │ │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └───┬────┘ │
│       │             │             │            │       │
│  ┌────▼─────────────▼─────────────▼────────────▼────┐  │
│  │                   SQLite                          │  │
│  │                                                   │  │
│  │  files | chunks | chunks_fts | embeddings |       │  │
│  │  relationships | chunks_vec                        │  │
│  │                                                   │  │
│  │  WAL mode · single file · zero config              │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

## Data Flow

### Indexing

```
Source file (e.g., src/auth.ts)
    │
    ▼
Language detection (extension → tree-sitter grammar)
    │
    ▼
tree-sitter parse → AST
    │
    ▼
AST walker → extract declarations (functions, classes, types, imports)
    │
    ├──► Each declaration → Chunk { content_md, signature, line range, metadata }
    │
    ├──► Imports → Relationship { source, target, type: "imports" }
    │
    ▼
SHA-256 content hash per chunk → dedup check
    │
    ▼
Insert into SQLite:
    ├── chunks table
    ├── chunks_fts (auto-sync via triggers)
    ├── relationships table
    └── queue for embedding
         │
         ▼
    ONNX embed → store in embeddings / chunks_vec
```

### Querying

```
Agent query: "how does authentication work"
    │
    ▼
Query text
    ├──► ONNX embed → query vector
    ├──► FTS5 MATCH query → keyword results [{id, rank}]
    └──► sqlite-vec KNN → vector results [{id, distance}]
         │
         ▼
Normalize scores → hybrid merge (alpha = 0.7)
    │
    ▼
Top-K results → fetch context chunks (±1 sibling)
    │
    ▼
Optional: graph traversal (dependency expansion)
    │
    ▼
Token-count (tiktoken-rs) → trim to budget
    │
    ▼
Assemble Markdown document → return to agent
```

### Incremental Update

```
File watcher (notify crate)
    │
    ▼
File changed (debounced 200ms)
    │
    ▼
SHA-256 hash comparison vs files table
    │
    ├── Unchanged → skip
    ├── Modified  → re-chunk, update chunks/embeddings/relationships
    └── Deleted   → cascade delete from all tables
```

## SQLite Schema

See [`schema.sql`](./schema.sql) for the complete DDL.

### Tables

| Table | Purpose |
|-------|---------|
| `files` | Source file metadata (path, language, hash, mtime) |
| `chunks` | Semantic code chunks (content_md, signature, line range, metadata) |
| `chunks_fts` | FTS5 virtual table for keyword search |
| `relationships` | Import/call graph edges between chunks |
| `embeddings` | Vector embeddings (BLOB storage, fallback) |
| `chunks_vec` | sqlite-vec virtual table (primary vector search) |

### Key Design Decisions

1. **WAL mode** — Enables concurrent reads during writes. Agent queries never block indexing.

2. **Content hash dedup** — SHA-256 of the raw source text for each chunk. Prevents duplicate inserts when re-indexing unchanged files.

3. **FTS5 content-sync triggers** — `chunks_fts` stays in sync with `chunks` automatically via INSERT/UPDATE/DELETE triggers. No manual reindexing needed.

4. **Dual vector storage** — `chunks_vec` (sqlite-vec, fast KNN) as primary, `embeddings` table (BLOB) as fallback. Both populated during indexing; query path decides which to use.

5. **Relationships as first-class data** — Not derived at query time. Import and call relationships are extracted during chunking and stored explicitly. Enables fast recursive graph traversal via CTEs.

## Embedding Model

**Model:** `nomic-embed-text-v1.5` (q8 quantized)
**Dimensions:** 768
**Size:** ~50MB
**Runtime:** ONNX Runtime (ort crate)
**Cache:** `~/.sqmd/models/`

Batch embedding processes chunks in groups of 64 for throughput. Model stays loaded in memory after first use (~300MB RSS).

## Performance Budget

| Operation | Budget | Notes |
|-----------|--------|-------|
| Cold model load | <2s | One-time per daemon lifetime |
| Per-file parse + chunk | <50ms | tree-sitter is fast |
| Per-file embed | <10ms | After model is loaded |
| Incremental re-index | <200ms | Single file change |
| FTS5 query | <3ms | Keyword search |
| Vector search | <5ms | 100k chunks |
| Hybrid search | <20ms | Combined + merge |
| Graph traversal (depth 2) | <50ms | Recursive CTE |
| Context assembly | <5ms | Token-count + trim |
| Idle daemon | ~15MB RAM | File watcher + SQLite connection |

## File Structure

```
sqmd/
├── Cargo.toml
├── crates/
│   ├── sqmd-core/
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── schema.rs          # SQLite DDL
│   │       ├── chunk.rs           # Chunker trait + Chunk struct
│   │       ├── languages/         # Per-language tree-sitter implementations
│   │       │   ├── typescript.rs
│   │       │   ├── rust.rs
│   │       │   └── python.rs
│   │       ├── embed.rs           # ONNX embedding pipeline
│   │       ├── search.rs          # Hybrid search engine
│   │       ├── graph.rs           # Relationship graph + traversal
│   │       ├── context.rs         # Token-budgeted context assembly
│   │       └── watcher.rs         # File watcher + incremental index
│   └── sqmd-cli/
│       └── src/
│           └── main.rs            # CLI commands
├── docs/
│   ├── ROADMAP.md
│   ├── WHAT_IT_IS.md
│   ├── ARCHITECTURE.md
│   └── schema.sql
└── .sqmd/                         # (gitignored) runtime data
    └── index.db
```
