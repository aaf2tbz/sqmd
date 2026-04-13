# sqmd — Development Roadmap

## Overview

A single Rust binary (~10MB) that turns any codebase into a queryable SQLite database of semantically chunked code, with tree-sitter parsing, local embeddings via llama.cpp, FTS5 + vector hybrid search, and an import/call relationship graph. Zero network. Zero external services. Works offline. Exposes 12 MCP tools for AI agent integration.

### Design principle: raw code, derived Markdown

sqmd stores raw source code (`content_raw`) in the database, **not** pre-rendered Markdown. Markdown is derived on demand via `Chunk::render_md()` at query time and returned as a `"markdown"` field in every query response. Agents grab it directly into their prompts; tooling uses the structured fields alongside it.

---

## Phase 0: Spike — COMPLETE

Validated the two riskiest dependencies before committing to the stack.

### Results

- **sqlite-vec**: Compiled statically via the `sqlite-vec` Rust crate. Registered as a process-level singleton via `sqlite3_auto_extension`.
- **llama.cpp**: Works via `llama-cpp-2` crate. Model load ~220ms (cached), inference ~17ms/chunk. mxbai-embed-large GGUF (1024 dims). Metal GPU acceleration on Apple Silicon.

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

- Language chunkers for TypeScript, TSX, Rust, Python, Go, Java, C, C++ via tree-sitter
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
- `index_file()`: single-file re-index for watcher and MCP integration
- File watcher via `notify` crate: recursive watch, 200ms debounce
- Batch tombstone cleanup via `IN (...)` queries

---

## Phase 4: Embeddings + Vector Search — COMPLETE

**Goal:** Semantic search on top of keyword search.

### What shipped

- Native llama.cpp embeddings via `llama-cpp-2` crate (mxbai-embed-large, 1024 dims)
- Hybrid search engine:
  - `fts_search()`: FTS5 with Porter stemming, file/type filters, rank normalization
  - `vec_search()`: KNN via `chunks_vec` (sqlite-vec), cosine distance
  - `layered_search()`: 5-layer pipeline (FTS, graph, communities, vector, hint vector)

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
- Methods: `search`, `cat`, `get`, `ls`, `context`, `stats`, `index_file`, `embed`

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
- Real batch llama.cpp embedding (was a loop calling `embed_one`)
- vec_search filter parity (source_type, agent_id)
- Temporal decay scoring
- Multi-threaded daemon (one connection per client thread)

---

## Phase 9: Performance Overhaul — COMPLETE

**Goal:** Speed, memory, and search quality improvements.

### What shipped

- **Wired dampening + importance boost** into both FTS and layered search
- **mmap + WAL tuning** (256MB mmap, autocheckpoint=1000, 8K page cache)
- **Read-consistent snapshots** (FTS search in BEGIN/COMMIT)
- **Single-pass tree-sitter** (AST reused for import extraction)
- **Fixed N+1 hint merge** (batch `IN (...)` query)
- **Shared daemon Embedder** (`Arc<Mutex<Option<EmbedProvider>>>`)
- **FTS5 Porter stemming** (schema v5 migration)
- **Batch tombstone writes** (hints, entity_attributes, relationships via `IN (...)`)
- **Read-only fast-path** (`open_fast()` for daemon read handlers)
- **In-memory query cache** (100 entries, 10s TTL)
- **Pre-rendered markdown** in search, cat, and get responses

---

## Phase 10: Extended Language Support — COMPLETE

**Goal:** Add HTML, CSS, and additional build-system language support.

### What shipped

- **HTML chunker** (`tree-sitter-html`): Semantic element classification
- **CSS chunker** (`tree-sitter-css`): Selector extraction (rule_set, @media/@keyframes/@supports)
- **C, C++, CMake, QML, Meson, Ruby** chunkers added in prior phases
- Schema v11 (community_type)
- 17 tree-sitter grammars + 21 line-based fallback languages = 38 total

---

## Phase 11: Recall Research Integration — COMPLETE

**Goal:** Apply findings from the Obsidian Vault Recall Eval research to improve sqmd's retrieval quality.

### What shipped

- **Semantic hint retrieval** (Gap 1): `hints_vec` virtual table (schema v12) enables vector KNN search over hint text
- **LLM prospective hints** (Gap 2): Native llama.cpp (phi4-mini) generates natural-language retrieval cues per chunk. Only for chunks with `importance >= 0.5`
- **Eval harness generalization** (Gap 3): `sqmd-bench` restructured with `run`, `generate`, `compare` subcommands
- **Session summaries** (Gap 4): `ingest_batch()` generates summary chunks with `contains` edges to children
- **Native runtime unification**: Both embeddings (mxbai-embed-large) and hints (phi4-mini) use llama.cpp via `llama-cpp-2`

---

## Phase 12: MCP Server + Agent Workflows — COMPLETE

**Goal:** Expose sqmd as an MCP server for AI coding tools, add agent workflow features.

### What shipped

- **MCP server** (`sqmd mcp`): JSON-RPC 2.0 over stdio, 12 tools
- **Both transport modes**: `Content-Length:` framed and raw JSON line-delimited
- **Harness setup** (`sqmd setup`): Auto-configure OpenCode, Codex, Claude Code, Cursor
- **Background embedding**: `embed_start`, `embed_progress`, `embed_stop` MCP tools
- **Worktree support**: Index discovery via `git rev-parse --git-common-dir`
- **Project root safety**: CWD fallback when `.sqmd/` resolves to home directory
- **sqmd-review skill**: Iterative git-connected code review with prior review awareness
- **24 CLI subcommands**: Full lifecycle management from init to update
- **Doctor command**: Diagnostic checks for index, embed, model, mcp, daemon
- **Diff command**: Show chunks modified since a timestamp

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
| 10 — Extended Language Support | **COMPLETE** |
| 11 — Recall Research Integration | **COMPLETE** |
| 12 — MCP Server + Agent Workflows | **COMPLETE** |

**v1.0.0** — phases 0-8.
**v1.1.0** — production hardening for signet-sqmd integration.
**v1.2.0** — performance overhaul, markdown output, CI reliability.
**v2.0.0** — HTML/CSS languages, semantic hint retrieval, LLM prospective hints, eval harness, session summaries, schema v12.
**v3.0.0** — ONNX replaced with native llama.cpp, MCP server, harness setup, background embedding.
**v3.3.0** — worktree support, MCP index discovery, sqmd-review v2.
**v3.4.0** — sqmd-review rewrite with git-connected iterative workflow.
**v3.4.1** — MCP project root safety fix, tombstoning prevention.

---

## Future Directions

### Entity Graph Redesign

See [design/entity-graph-redesign.md](design/entity-graph-redesign.md) for the full proposal. Key goals:

- Symbol-level entities (per-function/class, not per-file)
- Auto-populated `entity_dependencies` during indexing
- Merged graph layers (entity_deps as single source of truth)
- Graph-driven relational hints
- Community graph upgrade (module + type-hierarchy clusters)

### Potential New Features

- **Per-project config**: `.sqmd/config.toml` for language-specific settings, custom importance weights
- **Cross-project search**: Search across multiple `.sqmd/index.db` files
- **Web UI**: Browser-based code exploration dashboard
- **Plugin system**: Custom chunkers, search layers, and post-processing hooks

---

## Dependency Risk Matrix

| Dependency | Risk | Status |
|-----------|------|--------|
| `tree-sitter` + 17 language grammars | Low | Shipped (Phase 2, 10) |
| `rusqlite` (bundled) | Low | Shipped (Phase 1) |
| `sqlite-vec` (static compile) | Low | Shipped — compiled in, non-fatal |
| `llama-cpp-2` | Medium | Shipped — feature-gated (`native`) |
| `notify` (file watcher) | Low | Shipped (Phase 3) |
| `rayon` | Low | Shipped (Phase 3) |
| `chrono` | Low | Shipped (Phase 8) |
| `clap` (derive) | Low | Shipped (Phase 1) |
| `dirs` | Low | Shipped (Phase 12) |
| `sha2` | Low | Shipped (Phase 1) |
| `serde` / `serde_json` | Low | Shipped (Phase 1) |
| `ignore` | Low | Shipped (Phase 3) |
| `walkdir` | Low | Shipped (Phase 1) |
