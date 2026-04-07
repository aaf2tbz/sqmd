# sqmd — Development Roadmap

## Overview

A single Rust binary (~10MB) that turns any codebase into a queryable SQLite database of semantically chunked code, with tree-sitter parsing, local embeddings, FTS5 + vector hybrid search, and an import/call relationship graph. Zero network. Zero external services. Works offline.

### Design principle: raw code, derived Markdown

sqmd stores raw source code (`content_raw`) in the database, **not** pre-rendered Markdown. Markdown is derived on demand via `Chunk::render_md()` at query time and returned as a `"markdown"` field in every query response. Agents grab it directly into their prompts; tooling uses the structured fields alongside it.

---

## Phase 0: Spike — COMPLETE

Validated the two riskiest dependencies before committing to the stack.

### Results

- **sqlite-vec**: Compiled statically via the `sqlite-vec` Rust crate. Registered as a process-level singleton via `sqlite3_auto_extension`.
- **ort v2.0.0-rc.12**: Works. Model load ~220ms (cached), inference ~17ms/chunk. nomic-embed-text-v1.5 q8 ONNX model (768 dims) cached at `~/.sqmd/models/`.

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

- Language chunkers for TypeScript, TSX, Rust, Python, Go, Java via tree-sitter
- `LanguageChunker` trait with graceful fallback to `FileChunker` on parse failure
- Chunk types: Function, Method, Class, Struct, Enum, Interface, Type, Impl, Module, Section
- Single-pass parsing: AST reused for both chunking and import extraction
- Import/contains relationship extraction
- Importance scoring: `ChunkType::importance()` (functions=0.9, classes=0.85, sections=0.2)
- Schema migration system (versioned, reads `schema_version` table)

---

## Phase 3: Incremental Indexing — COMPLETE

**Goal:** Fast incremental updates when files change.

### What shipped

- Rayon parallelism: 4-phase pipeline (walk -> read -> chunk -> write)
- mtime pre-filter: skip files where mtime unchanged; content hash verified before writing
- `index_file()`: single-file re-index for watcher integration
- File watcher via `notify` crate: recursive watch, 200ms debounce
- Batch tombstone cleanup via `IN (...)` queries

---

## Phase 4: Embeddings + Vector Search — COMPLETE

**Goal:** Semantic search on top of keyword search.

### What shipped

- `Embedder` struct: lazy-loads nomic-embed-text-v1.5 ONNX model (768 dims)
- Hybrid search engine:
  - `fts_search()`: FTS5 with Porter stemming, file/type filters, rank normalization
  - `vec_search()`: KNN via `chunks_vec` (sqlite-vec), cosine distance
  - `hybrid_search()`: alpha-blended merge, single-source penalty (0.8)
- Adaptive padding to nearest multiple-of-64 (not power-of-two)
- Real batched ONNX inference (single forward pass)

---

## Phase 5: Relationship Graph — COMPLETE

**Goal:** Import/call dependency graph for traversal queries.

### What shipped

- `extract_calls()`: regex-based call graph extraction
- Cross-file resolution via import relationships
- Recursive CTE depth traversal
- `sqmd deps --depth N` CLI command

---

## Phase 6: Agent API + Context Assembly — COMPLETE

**Goal:** Turn sqmd into something agents can query programmatically.

### What shipped

- `ContextAssembler`: token-budgeted context assembly
- `sqmd context --query --files --max-tokens --deps --dep-depth`
- Unix socket daemon (`~/.sqmd/daemon.sock`) with JSON protocol
- Methods: `search`, `cat`, `get`, `ls`, `context`, `stats`, `ingest`, `ingest_batch`, `forget`, `modify`, `embed`, `embed_text`, `embed_batch`

---

## Phase 7: Knowledge Store — COMPLETE

**Goal:** Unified code + knowledge store with external ingest API.

### What shipped

- Schema v4: source_type, agent_id, tags, decay_rate, last_accessed, created_by
- 6 new chunk types: fact, summary, decision, preference, entity_description, document_section
- `KnowledgeIngestor`: `ingest()`, `ingest_batch()`, `forget()`, `modify()` with content-hash dedup
- CLI commands: `sqmd ingest`, `sqmd forget`, `sqmd modify`

---

## Phase 8: Production Hardening — COMPLETE

**Goal:** Fix correctness issues and improve production readiness.

### What shipped

- Asymmetric retrieval (`search_query:` / `search_document:` prefixes)
- Real batch ONNX embedding (was a loop calling `embed_one`)
- vec_search filter parity (source_type, agent_id)
- Temporal decay scoring
- Multi-threaded daemon (one connection per client thread)

---

## Phase 9: Performance Overhaul — COMPLETE

**Goal:** Speed, memory, and search quality improvements for v1.2.0.

### What shipped

- **Wired dampening + importance boost** into both FTS and hybrid search
- **mmap + WAL tuning** (256MB mmap, autocheckpoint=1000, 8K page cache)
- **Read-consistent snapshots** (FTS search in BEGIN/COMMIT)
- **Single-pass tree-sitter** (AST reused for import extraction)
- **Fixed N+1 hint merge** (batch `IN (...)` query)
- **Adaptive embedding padding** (multiple-of-64, single-pass tokenization)
- **Shared daemon Embedder** (`Arc<Mutex<Option<Embedder>>>`)
- **FTS5 Porter stemming** (schema v5 migration)
- **Batch tombstone writes** (hints, entity_attributes, relationships via `IN (...)`)
- **Read-only fast-path** (`open_fast()` for daemon read handlers)
- **LRU query cache** (256 entries, 10s TTL)
- **Pre-rendered markdown** in search, cat, and get responses

### E2E validation

- 70 tests (default), 78 tests (embed), 0 clippy warnings
- CI green on all jobs (build + test + clippy, default + embed)

---

## Progress Summary

| Phase | Status |
|-------|--------|
| 0 — Spike | **COMPLETE** |
| 1 — Foundations | **COMPLETE** |
| 2 — Tree-sitter Chunking | **COMPLETE** |
| 3 — Incremental Indexing | **COMPLETE** |
| 4 — Embeddings + Vector Search | **COMPLETE** |
| 5 — Relationship Graph | **COMPLETE** |
| 6 — Agent API + Context Assembly | **COMPLETE** |
| 7 — Knowledge Store | **COMPLETE** |
| 8 — Production Hardening | **COMPLETE** |
| 9 — Performance Overhaul | **COMPLETE** |

**v1.0.0** — all phases through 8 complete.
**v1.1.0** — production hardening for signet-sqmd integration.
**v1.2.0** — performance overhaul, markdown output, CI reliability.

---

## Dependency Risk Matrix

| Dependency | Risk | Status |
|-----------|------|--------|
| `tree-sitter` + language grammars | Low | Shipped (Phase 2) |
| `rusqlite` (bundled) | Low | Shipped (Phase 1) |
| `sqlite-vec` (static compile) | Medium | Shipped — compiled in, non-fatal |
| `ort` v2 RC (ONNX Runtime) | Medium | Shipped — feature-gated |
| `notify` (file watcher) | Low | Shipped (Phase 3) |
| `rayon` | Low | Shipped (Phase 3) |
| `chrono` | Low | Shipped (Phase 8) |
| `clap` (derive) | Low | Shipped (Phase 1) |
