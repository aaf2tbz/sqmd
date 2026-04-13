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
| 13 — Per-Project Configuration | **COMPLETE** |
| 14 — Index Health & Maintenance | **COMPLETE** |
| 15 — Type-Aware Call Graph | **COMPLETE** |
| 16 — Context Assembly Improvements | **COMPLETE** |
| 17 — Cross-Project Search | **COMPLETE** |
| 18 — Import Resolution Quality | **COMPLETE** |
| 19 — Plugin System | **COMPLETE** |
| 20 — Entity Graph Redesign | **COMPLETE** |

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

### Phase 13: Per-Project Configuration

**Goal:** Allow projects to customize sqmd behavior without environment variables or code changes.

**Motivation:** Currently all tuning knobs are hardcoded (SQLite pragmas, search weights, chunking parameters, importance thresholds). Every project has different needs — a large monorepo needs different mmap/cache sizes than a small library, and a Rust project needs different chunking rules than a Python one.

**Spec:**

Create `.sqmd/config.toml` in the project root, loaded by `init`/`index`/`mcp`/`serve`. Values cascade: CLI flags > env vars > project config > defaults.

```toml
[sqlite]
mmap_size = "256MB"          # default: 256MB
cache_size = "-8000"         # default: 8K pages
wal_autocheckpoint = 1000    # default: 1000
busy_timeout = 5000          # default: 5s

[search]
default_top_k = 10           # default: 10
default_max_tokens = 8000    # default: 8000
graph_boost_base = 0.20      # default: 0.20
graph_boost_decay = 0.50     # default: 0.50

[chunking]
unclaimed_gap = 50           # default: 50 lines (max gap for unclaimed code)
min_importance = 0.0         # minimum importance to keep a chunk

[importance]
# Override default importance weights per chunk type
function = 0.9
class = 0.85
struct = 0.85
interface = 0.8
method = 0.8
import = 0.1
section = 0.2

[hints]
min_importance = 0.5         # default: 0.5 (minimum chunk importance for LLM hints)
max_per_chunk = 3            # default: 3

[embed]
model = "mxbai-embed-large"  # override default embedding model
hint_model = "phi4-mini"     # override default hint model

[watch]
debounce_ms = 200            # default: 200ms
```

**Scope:** ~300 LOC. New `config.rs` module in sqmd-core. Parse on index open, validate, apply to pragmas and search params. No schema migration needed.

---

### Phase 14: Index Health and Maintenance

**Goal:** Detect corruption, clean orphans, reclaim space, and keep indexes healthy over time.

**Motivation:** `sqmd prune` only removes tombstoned chunks from the `chunks` table, leaving orphaned rows in `hints`, `relationships`, `entity_attributes`, `embeddings`, and `entity_dependencies`. No integrity checking exists. WAL files grow unbounded. FTS indexes can fall out of sync with content. There is no way to assess index health.

**Spec:**

New `sqmd maintain` command (alias: `sqmd health`) with subcommands:

```bash
sqmd health                   # run all checks, print report
sqmd health --check integrity # PRAGMA integrity_check
sqmd health --check orphans   # find dangling foreign keys
sqmd health --check fts       # verify FTS content matches chunks
sqmd health --check vec       # verify vector table row counts
sqmd health --check stats     # chunk/relationship/embedding counts
sqmd maintain                 # run all repairs
sqmd maintain --vacuum        # VACUUM to reclaim WAL space
sqmd maintain --analyze       # ANALYZE for query planner stats
sqmd maintain --reindex-fts   # rebuild FTS indexes from chunks
sqmd maintain --clean-orphans # remove dangling rows in all tables
sqmd maintain --compact       # vacuum + analyze + clean orphans
```

**Orphan detection logic:**
```sql
-- Hints pointing to deleted chunks
DELETE FROM hints WHERE chunk_id IN (SELECT id FROM chunks WHERE is_deleted = 1);
-- Relationships with deleted source/target
DELETE FROM relationships WHERE source_id IN (SELECT id FROM chunks WHERE is_deleted = 1)
  OR target_id IN (SELECT id FROM chunks WHERE is_deleted = 1);
-- Entity attributes with deleted chunk_id
DELETE FROM entity_attributes WHERE chunk_id IN (SELECT id FROM chunks WHERE is_deleted = 1);
-- Embeddings for deleted chunks
DELETE FROM embeddings WHERE chunk_id IN (SELECT id FROM chunks WHERE is_deleted = 1);
-- Entity dependencies with missing entities
DELETE FROM entity_dependencies WHERE source NOT IN (SELECT name FROM entities)
  OR target NOT IN (SELECT name FROM entities);
```

**Add to `sqmd doctor`:** Run `PRAGMA integrity_check` and basic orphan count as part of the standard diagnostic.

**MCP tool:** Add `health` tool that returns a structured report (corruption status, orphan counts, index size, WAL size, FTS/vector consistency).

**Scope:** ~400 LOC. New `maintain.rs` module. Schema additions: none. CLI: 1 new command group. MCP: 1 new tool.

---

### Phase 15: Type-Aware Call Graph

**Goal:** Replace the blind regex call extractor with tree-sitter-based type-aware call resolution.

**Motivation:** The current `extract_calls()` uses `\w+\(` regex on raw content. This produces massive false positives (string literals, type casts, macro invocations) and false negatives (method calls on imported objects, trait method calls, chained calls). Cross-file call resolution only works for direct name matches against `name_to_id`, missing method dispatch entirely. This is the single biggest quality gap in sqmd's relationship graph.

**Spec:**

Replace `extract_calls()` in `relationships.rs` with a `CallExtractor` trait implemented per language:

```rust
trait CallExtractor {
    fn extract_calls(&self, source: &str, tree: &tree_sitter::Tree, chunk_names: &HashMap<String, i64>)
        -> Vec<(String, String)>;  // (caller_chunk_name, callee_symbol)
}
```

**Per-language extraction:**

| Language | Method | Examples |
|----------|--------|---------|
| **Rust** | `call_expression` node in tree-sitter. Resolve `foo.method()` against impl blocks and trait bounds in the same file + imported items. `Foo::new()` against struct impls. | `processInputBuffer()` → resolved via `self.` receiver or direct name |
| **TypeScript** | `call_expression` node. Resolve `obj.method()` against imported class/interface definitions. Handle `this.method()` as intra-class call. | `authService.validate()` → resolved via import + class member lookup |
| **Python** | `call` node. Resolve `self.method()` and `cls.method()` against class body. Resolve `module.func()` against imports. | `self.authenticate()` → resolved to method in same class |
| **Go** | `call_expression` node. Resolve `pkg.Func()` against imports. Resolve `receiver.Method()` against struct methods. | `client.Do()` → resolved via import + method set |
| **Java/C++** | `method_invocation` / `call_expression`. Resolve against class hierarchy. | `object.method()` → resolved via type hierarchy |

**Resolution strategy (per file, post-chunking):**
1. Build a local name→chunk_id map (existing)
2. Build a local type→members map (new): for each struct/class/impl chunk, extract member names
3. Build an import→external_symbols map (new): for each import chunk, note what names it introduces
4. Walk call expressions in tree-sitter AST:
   - `direct_call(x)`: resolve `x` against name map + imports
   - `method_call(obj, method)`: resolve `obj` to its type, then look up `method` in that type's members
   - `static_call(Type, method)`: resolve against imported types

**Filtering improvements:**
- Skip calls inside string literals (tree-sitter `string` nodes)
- Skip type casts and constructor patterns
- Filter language builtins per-language (e.g., `console.log`, `println!`, `format!`)
- Deduplicate: same caller→callee pair only once per file

**Scope:** ~800 LOC. New `call_extractor.rs` module + per-language implementations in `languages/`. Updates to `index.rs` to use the new extractor. No schema changes.

---

### Phase 16: Context Assembly Improvements

**Goal:** Make dependency expansion in context assembly follow the full relationship graph, not just imports.

**Motivation:** `ContextAssembler::get_related_chunks()` only follows `imports` relationships (a single CTE with `r.rel_type = 'imports'`). This means context assembly misses call chains, type hierarchies, and containment relationships. An agent asking "how does auth work" gets imported modules but not callers of the auth functions.

**Spec:**

Update `get_related_chunks()` in `context.rs`:

1. **Multi-relationship expansion**: Follow `imports`, `calls`, `contains`, `extends`, `implements` — not just `imports`:
   ```sql
   WITH rel_graph AS (
     SELECT target_id, rel_type FROM relationships
     WHERE source_id = ? AND rel_type IN ('imports','calls','contains','extends','implements')
     UNION
     SELECT source_id, rel_type FROM relationships
     WHERE target_id = ? AND rel_type IN ('calls','contains')
   )
   ```
2. **Entity graph integration**: When a chunk belongs to an entity with rich dependencies, pull in related entity members (e.g., other methods on the same struct).
3. **Community-aware boosting**: When expanding context, prefer chunks from the same community (module) as the query results. Add a `community_id` join to the expansion CTE.
4. **Relevance-weighted expansion**: Instead of assigning all dep chunks a flat 0.5 score, score them by relationship type: `calls` > `imports` > `contains` > `extends` > `implements`. A callee is more relevant to understanding a function than an import.
5. **Configurable depth and limits**: Make dep expansion depth and max chunks configurable via project config (from Phase 13) rather than hardcoded `LIMIT 50`.
6. **Token-budget-aware expansion**: Stop expanding when the accumulated tokens approach the budget, prioritizing higher-scored relationships first.

**Scope:** ~200 LOC changes in `context.rs`. No schema changes. Depends on Phase 15 for quality call graph data.

---

### Phase 17: Cross-Project Search

**Goal:** Search across multiple sqmd indices from a single MCP server or CLI invocation.

**Motivation:** Developers work across multiple repositories. An agent helping with a monorepo-with-dependencies needs to search both the main repo and library repos. Currently sqmd operates on a single `.sqmd/index.db`.

**Spec:**

**CLI interface:**
```bash
sqmd search "authentication" --project ~/signetai,~/sqmd
sqmd context --query "how does auth work" --project ~/signetai,~/sqmd,~/signet-sqmd
```

**MCP interface:** New `project` parameter on `search`, `context`, `deps` tools:
```json
{"name": "search", "arguments": {"query": "auth", "project": "/Users/dev/repo-a,/Users/dev/repo-b"}}
```

**Implementation:**

Option A — **SQLite ATTACH**: Open the primary database and `ATTACH DATABASE` for each additional index. Run queries with `UNION ALL` across databases:
```sql
SELECT *, 'repo-a' as project_name FROM main.chunks WHERE ...
UNION ALL
SELECT *, 'repo-b' as project_name FROM repo_b.chunks WHERE ...
```
- Pro: Single query, single result set, reuses all existing search logic
- Con: ATTACH requires all databases to use the same schema version

Option B — **Parallel queries**: Open each database separately, run the same query in parallel (via rayon), merge and re-rank results:
- Pro: Schema version independence, simpler error handling
- Con: More memory, merge logic needed for score normalization

**Recommendation:** Start with Option A (ATTACH) since all sqmd indices use the same schema. Fall back to Option B if schema mismatches are detected.

**Project registry:** Create `~/.sqmd/projects.toml` for named project aliases:
```toml
[projects.signet]
path = "/Users/dev/signetai"

[projects.sqmd]
path = "/Users/dev/sqmd"

[projects.signet-sqmd]
path = "/Users/dev/signet-sqmd"
```

Then: `sqmd search "auth" --project signet,sqmd`

**Scope:** ~500 LOC. New `multi_project.rs` module. CLI argument parsing changes. MCP tool parameter additions. No schema changes.

---

### Phase 18: Import Resolution Quality

**Goal:** Improve import path resolution to handle re-exports, workspace aliases, barrel files, and language-specific package managers.

**Motivation:** Import resolution currently uses heuristic path guessing (`resolve_module_path()` in `relationships.rs`). It tries appending extensions (`.ts`, `.rs`, etc.) and `index.ts`/`mod.rs` conventions, but misses: TypeScript path aliases (`@foo/bar`), Rust workspace crates, Python namespace packages, re-export chains, and default exports.

**Spec:**

**TypeScript:**
- Parse `tsconfig.json` `compilerOptions.paths` for alias resolution (`@components/*` → `src/components/*`)
- Handle re-exports: `export { X } from './module'` creates an import relationship for the source module
- Handle `export type` — optionally create relationships (configurable, off by default)
- Handle dynamic `import()` — create a relationship (configurable, off by default)

**Rust:**
- Parse `Cargo.toml` `[dependencies]` and `[workspace.members]` for workspace crate resolution
- Map `use crate::foo::bar` → `crates/foo/src/bar.rs` within the workspace
- Handle `pub use` re-exports as transitive import relationships
- Handle `extern crate` declarations

**Python:**
- Parse `pyproject.toml` `[project]` for package name → directory mapping
- Handle `__init__.py` re-exports as transitive imports
- Resolve relative imports (`.foo`, `..bar`) against package structure

**Go:**
- Parse `go.mod` `module` declaration for module prefix resolution
- Resolve `import "pkg/path"` against the module prefix + filesystem

**General:**
- Re-export chain resolution (follow re-exports up to N hops, configurable, default 3)
- Cache resolved paths per index session to avoid re-resolving on every re-index

**Scope:** ~600 LOC across `relationships.rs` + per-language files. No schema changes. Depends on config system (Phase 13) for toggle flags.

---

### Phase 19: Plugin System

**Goal:** Allow users to extend sqmd with custom chunkers, search layers, and post-processing hooks without modifying sqmd itself.

**Motivation:** Teams with proprietary languages, custom frameworks, or specialized search needs currently have no way to extend sqmd. Adding a new language requires modifying sqmd source. Custom post-processing (e.g., redacting secrets, enriching chunks with external metadata) is impossible.

**Spec:**

**Plugin manifest:** `.sqmd/plugins.toml`
```toml
[[plugin]]
name = "sql-parser"
type = "chunker"
command = ["node", "/path/to/sql-plugin.mjs"]
extensions = [".sql", ".pgsql"]
languages = ["SQL", "PostgreSQL"]

[[plugin]]
name = "secret-redactor"
type = "post-index"
command = ["python3", "/path/to/redact.py"]
priority = 10
```

**Plugin types:**

| Type | Interface | When called |
|------|-----------|-------------|
| `chunker` | stdin: raw source → stdout: JSON chunks | During indexing, for matching extensions |
| `search-layer` | stdin: query + results → stdout: re-ranked results | After search, before rendering |
| `post-index` | stdin: file path → exit code 0/1 | After each file is indexed |
| `pre-search` | stdin: query string → stdout: rewritten query | Before search execution |

**Chunker plugin protocol (JSON over stdin/stdout):**
```json
// stdin
{"file_path": "src/foo.custom", "content": "...", "language": "SQL"}

// stdout
{
  "chunks": [
    {
      "name": "select_users",
      "chunk_type": "function",
      "line_start": 1,
      "line_end": 15,
      "content_raw": "SELECT * FROM users...",
      "signature": "SELECT * FROM users",
      "imports": []
    }
  ]
}
```

**Sandboxing:** Plugins run as child processes with a timeout (default: 10s per file). Stderr is captured and logged. Non-zero exit is logged but does not abort indexing.

**Scope:** ~500 LOC. New `plugin.rs` module. Plugin discovery on `init`/`index`/`mcp`. Integration with `index.rs` chunking pipeline and `search.rs` result pipeline. No schema changes.

---

### Phase 20: Entity Graph Redesign

**Goal:** Symbol-level entities, auto-populated entity_dependencies, merged graph layers, graph-driven hints.

**Motivation:** The entity graph is architecturally incomplete. Entities are created at file granularity only. `entity_dependencies` has `imports`/`calls` gaps. Seven of 14 relationship types in the schema are never populated. Graph expansion search contributes near-zero results because the graph is too shallow.

**Spec:** See [design/entity-graph-redesign.md](design/entity-graph-redesign.md) for the full proposal.

Key deliverables:
1. Symbol-level entities (per-function/class/struct, not per-file)
2. Auto-populate all entity_dependency types during indexing (`imports`, `calls`, `contains`, `extends`, `implements`, `overrides`)
3. Merge graph layers — `entity_dependencies` becomes single source of truth, `relationships` materialized from it
4. Graph-driven relational hints (caller/callee/member/heir/importer)
5. Community graph upgrade (module + type-hierarchy clusters with structural summaries)
6. Context assembly uses entity_deps instead of relationships (depends on Phase 16)

**Dependencies:** Requires Phase 15 (type-aware call graph) and Phase 18 (import resolution quality) for high-quality entity edges.

**Scope:** ~1500 LOC across `index.rs`, `entities.rs`, `relationships.rs`, `search.rs`, `context.rs`, `communities.rs`. Schema migration v15.

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
