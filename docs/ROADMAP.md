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

## Phase 3: Incremental Indexing — COMPLETE

**Goal:** Fast incremental updates when files change.

### What shipped

- Rayon parallelism: 4-phase pipeline (walk → read → chunk → write)
  - Phase 1: serial walk + mtime pre-filter against DB
  - Phase 2: parallel file reading + hashing (`into_par_iter`)
  - Phase 3: parallel chunking (CPU-bound tree-sitter)
  - Phase 4: serial DB writes (single connection)
- mtime pre-filter: skip files where mtime unchanged (fast path); content hash verified before writing
- `index_file()`: single-file re-index for watcher integration; handles file deletion gracefully
- File watcher via `notify` crate: recursive watch, 200ms debounce, language-aware filtering, initial full index
- Fixed parent tracking bug: `contains` relationship now compares line ranges (was comparing chunk IDs)
- Dependencies added: `rayon`, `notify`, `tempfile`

### E2E validation

- 33 tests pass (5 new), 0 clippy warnings
- Parallel consistency: 20 files indexed correctly in parallel, re-index skips all

---

## Phase 4: Embeddings + Vector Search — COMPLETE

**Goal:** Semantic search on top of keyword search.

### What shipped

- `Embedder` struct: lazy-loads nomic-embed-text-v1.5 ONNX model (768 dims, unit-normalized), batch embed
- Hybrid search engine (`search.rs`):
  - `fts_search()`: FTS5 with file/type filters, rank normalization to 0..1
  - `vec_search()`: KNN via `chunks_vec` (sqlite-vec), cosine distance
  - `hybrid_search()`: alpha-blended merge (`alpha*vec + (1-alpha)*fts`), single-source penalty (0.8)
  - `embed_unembedded()`: batch embeds unindexed chunks (64/batch, prioritized by importance)
  - `store_embedding()`: writes to both `embeddings` table and `chunks_vec`
- CLI: `sqmd search` (hybrid by default), `sqmd embed`, `sqmd index --embed`, `sqmd stats` (embedding count)

### E2E validation

- 43 tests pass (10 new), 0 clippy warnings
- CI-safe: all embedding tests skip gracefully without model

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

## Dependency Risk Matrix

| Dependency | Risk | Status |
|-----------|------|--------|
| `tree-sitter` + language grammars | Low | Validated (Phase 2) |
| `rusqlite` (bundled) | Low | Shipped (Phase 1) |
| `sqlite-vec` (static compile) | Medium | Validated (Phase 0) — compiled in, non-fatal |
| `ort` v2 RC (ONNX Runtime) | Medium | Validated (Phase 0) — test skips without model |
| `notify` (file watcher) | Low | Shipped (Phase 3) |
| `tiktoken-rs` | Low | Pending (Phase 6) |
| `clap` (derive) | Low | Shipped (Phase 1) |
| `rayon` | Low | Shipped (Phase 3) |

---

## Progress Summary

| Phase | Status |
|-------|--------|
| 0 — Spike | **COMPLETE** |
| 1 — Foundations | **COMPLETE** |
| 2 — Tree-sitter Chunking | **COMPLETE** |
| 3 — Incremental Indexing | **COMPLETE** |
| 4 — Embeddings + Vector Search | **COMPLETE** |
| 5 — Relationship Graph | Future |
| 6 — Agent API + Context Assembly | Future |

**MVP reached after Phase 6.**
