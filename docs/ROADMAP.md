# sqmd — Development Roadmap

## Overview

A single Rust binary (~5MB) that turns any codebase into a queryable SQLite database of semantically chunked code, with tree-sitter parsing, local embeddings, FTS5 + vector hybrid search, and an import/call relationship graph. Zero network. Zero external services. Works offline.

### Design principle: raw code, derived Markdown

sqmd stores raw source code (`content_raw`) in the database, **not** pre-rendered Markdown. Markdown is derived on demand via `Chunk::render_md()` at query time. This avoids stale renderings, keeps the source of truth in the code itself, and lets consumers choose their own rendering format.

---

## Phase 0: Spike — COMPLETE

Validated the two riskiest dependencies before committing to the stack.

### Results

- **sqlite-vec**: Cannot be loaded as a `.dylib` extension from source (linking issues), but compiles statically via the `sqlite-vec` Rust crate (`cc::Build`). Registered as a process-level singleton via `sqlite3_auto_extension`. The `chunks_vec` virtual table is created non-fatally in schema init.
- **ort v2.0.0-rc.12**: Works. Model load ~220ms (cached), inference ~17ms/chunk. nomic-embed-text-v1.5 q8 ONNX model (768 dims, 522MB) cached at `~/.sqmd/models/`. Test skips gracefully when model absent (CI-safe).

---

## Phase 1: Foundations — COMPLETE

**Goal:** Project scaffold, SQLite schema, CLI skeleton, basic file ingestion.

### What shipped

- Cargo workspace: `sqmd-core` (library) + `sqmd-cli` (binary named `sqmd`)
- SQLite schema with WAL mode, FTS5, relationships table, auto-sync triggers
- File walking with `.gitignore`-aware skipping, language detection, SHA-256 hashing
- CLI commands: `init`, `index`, `search`, `stats`, `get`, `reset`, `deps`
- Release binary: 4.2MB (macOS universal)

---

## Phase 2: Tree-sitter Chunking — COMPLETE

**Goal:** Parse source files into semantically meaningful chunks.

### What shipped

- Language chunkers for TypeScript, Rust, Python via tree-sitter
- `LanguageChunker` trait with graceful fallback to `FileChunker` on parse failure
- Chunk types: Function, Method, Class, Struct, Enum, Interface, Type, Impl, Module, Section
- `content_raw` stored in DB (not `content_md`); Markdown derived via `Chunk::render_md()`
- Import relationship extraction for all three languages
- `contains` relationship extraction (class→method, impl→method, module→function)
- Importance scoring: `ChunkType::importance()` (functions=0.9, classes=0.85, sections=0.2, etc.)
- Context lines: 2-3 lines before each chunk (decorators, comments) via O(k) `rsplit`
- Schema migration system (versioned, reads `schema_version` table)

### Key learnings

- tree-sitter node kind names differ from docs (e.g., Rust: `function_item` not `fn_item`)
- `tree_sitter::Language` does not implement `Copy`; must use `.clone()`
- TypeScript import specifiers are nested 3 levels deep; must walk recursively
- Python `decorated_definition` wraps decorated methods; `expression_statement` wraps top-level assignments
- FTS5 tokenization: `unicode61` only (no Porter stemmer) — code identifiers don't stem well

### E2E validation

- Indexing sqmd itself: 220 chunks, 13 relationships in ~38ms
- 28 tests pass, 0 clippy warnings

---

## Phase 3: Incremental Indexing — NEXT

**Goal:** Fast incremental updates when files change via `notify` file watcher.

### 3.1 Change detection

- Compare file `mtime` + `hash` against `files` table
- Three states: `unchanged` (skip), `modified` (re-chunk), `deleted` (remove all chunks + relationships)
- Parallel file processing via `rayon`

### 3.2 File watcher

- `notify` crate (kqueue on macOS, inotify on Linux, ReadDirectoryChanges on Windows)
- 200ms debounce window (coalesce rapid save events)
- On change: hash check → if different, re-index that single file
- On delete: cascade remove from `files`, `chunks`, `relationships`, `embeddings`, `chunks_fts`

### 3.3 Connection management

- SQLite WAL mode for concurrent reads during writes
- 1 writer connection, N reader connections
- Embeddings written asynchronously without blocking reads

### 3.4 Performance targets

| Metric | Target |
|--------|--------|
| Per-file parse + chunk | <50ms |
| Per-file with embedding | <100ms |
| Full index (10k files, cold) | <60s |
| Incremental (single file) | <200ms |

---

## Phase 4: Embeddings + Vector Search (MVP milestone)

**Goal:** Semantic search on top of keyword search.

### 4.1 Embedding pipeline (`sqmd-core/src/embed.rs`)

- `ort` crate for ONNX Runtime (v2 RC — already validated in spike)
- Model: `nomic-embed-text-v1.5` (768-dim, q8 quantized, 522MB)
- First-run download from HuggingFace, cached in `~/.sqmd/models/`
- Batch embedding for throughput (process N chunks per ONNX session)

### 4.2 Vector storage

`sqlite-vec` compiled statically (validated in spike):
```sql
CREATE VIRTUAL TABLE chunks_vec USING vec0(embedding float[768]);
```

### 4.3 Hybrid search engine (`sqmd-core/src/search.rs`)

```rust
pub struct SearchQuery {
    pub text: String,
    pub top_k: usize,           // default 20
    pub alpha: f64,             // 0.7 = 70% vector, 30% keyword
    pub file_filter: Option<String>,
    pub type_filter: Option<ChunkType>,
    pub min_score: f64,         // default 0.3
}

pub struct SearchResult {
    pub chunk: Chunk,
    pub score: f64,
    pub vec_distance: Option<f64>,
    pub fts_rank: Option<f64>,
    pub context_chunks: Vec<Chunk>,
}
```

### 4.4 Hybrid scoring algorithm

1. Embed query text → query vector
2. FTS5 MATCH query → normalized scores
3. vec0 KNN search → normalized distances
4. Merge: `hybrid_score = alpha * vec_score + (1 - alpha) * fts_score`
5. Single-source penalty: if a result only appears in one index, multiply by 0.8
6. Fetch 1-2 adjacent chunks (line proximity) as context
7. Return top-K

### 4.5 Performance targets

| Metric | Target |
|--------|--------|
| Single chunk embed | <10ms |
| Batch embed (64 chunks) | <200ms |
| Vector search (100k chunks) | <5ms |
| FTS5 search | <3ms |
| Full hybrid query | <20ms |

---

## Phase 5: Relationship Graph (Future)

**Goal:** Import/call dependency graph for traversal queries.

### 5.1 Import extraction — DONE (Phase 2)

Already implemented for TypeScript, Rust, and Python. Resolves relative paths, `crate::`, `super::`, `self::`.

### 5.2 Call graph extraction (best-effort, static)

- Within function bodies, find identifiers matching known function/method names
- Cross-file resolution via import relationships
- Inherently approximate for dynamic languages — treated as hints, not proofs

### 5.3 Graph queries (`sqmd-core/src/graph.rs`)

```rust
pub fn get_dependencies(db: &Connection, chunk_id: i64, depth: usize) -> Vec<Chunk>
pub fn get_dependents(db: &Connection, chunk_id: i64, depth: usize) -> Vec<Chunk>
pub fn get_path(db: &Connection, from: i64, to: i64) -> Vec<Chunk>
```

Recursive CTE traversal in SQL.

### 5.4 Graph-augmented search

When a chunk matches a search query, automatically include its direct dependencies (configurable depth). "I searched for auth middleware and got the whole auth flow."

---

## Phase 6: Agent API + Context Assembly (Future)

**Goal:** Turn sqmd into something agents can query programmatically.

### 6.1 Daemon mode (`sqmd serve`)

- Unix socket (`~/.sqmd/daemon.sock`)
- JSON request/response protocol
- Auto-start on first query, stay resident
- Background file watcher + incremental re-index

### 6.2 Query protocol

```json
{
    "method": "search",
    "params": {
        "query": "how does authentication work",
        "top_k": 10,
        "include_deps": true,
        "dep_depth": 1
    }
}
```

### 6.3 Context assembly (`sqmd-core/src/context.rs`)

Given a query or working files:

1. Search for relevant chunks
2. Fetch surrounding context chunks (±1 sibling)
3. If `include_deps`, fetch dependency graph chunks
4. Token-count via `tiktoken-rs` (cl100k base)
5. Trim to budget (default: 8000 tokens)
6. Render as a single Markdown document for context injection

---

## Phase 7: Signet Integration (Future)

**Goal:** Replace Signet's LLM-heavy extraction pipeline with sqmd.

### 7.1 Replace extraction worker

- Current: transcript → LLM extract → LLM decide → write (3 calls)
- New: transcript → sqmd chunk → embed → deterministic dedup → write (0 calls)

### 7.2 Replace decision worker

- Content hash dedup (exact match)
- Cosine similarity dedup (threshold 0.95)
- Importance scoring: recency + turn density + error count (from transcript structure)

### 7.3 Replace synthesis worker

- Query sqmd for top-scored recent chunks
- Template-based MEMORY.md assembly (no LLM render)
- Optional: single lightweight LLM pass for prose smoothing

### 7.4 Migration path

- Add `chunks` table to existing `memories.db`
- Parallel indexing (sqmd + legacy) during transition
- Switch read path to sqmd chunks
- Deprecate legacy extraction pipeline

---

## Dependency Risk Matrix

| Dependency | Risk | Status |
|-----------|------|--------|
| `tree-sitter` + language grammars | Low | Validated (Phase 2) |
| `rusqlite` (bundled) | Low | Shipped (Phase 1) |
| `sqlite-vec` (static compile) | Medium | Validated (Phase 0) — compiled in, non-fatal |
| `ort` v2 RC (ONNX Runtime) | Medium | Validated (Phase 0) — test skips without model |
| `notify` (file watcher) | Low | Pending (Phase 3) |
| `tiktoken-rs` | Low | Pending (Phase 6) |
| `clap` (derive) | Low | Shipped (Phase 1) |
| `rayon` | Low | Pending (Phase 3) |

---

## Progress Summary

| Phase | Status |
|-------|--------|
| 0 — Spike | **COMPLETE** |
| 1 — Foundations | **COMPLETE** |
| 2 — Tree-sitter Chunking | **COMPLETE** |
| 3 — Incremental Indexing | Next |
| 4 — Embeddings + Vector Search | MVP milestone |
| 5 — Relationship Graph | Future |
| 6 — Agent API + Context Assembly | Future |
| 7 — Signet Integration | Future |

**MVP (usable by agents) after Phase 4.**
