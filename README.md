# sqmd

**Code intelligence for AI agents. Drop any project in, get instant semantic search, dependency graphs, and structured recall — no network, no API keys, one binary.**

sqmd parses your codebase with tree-sitter, chunks every function, class, struct, and import into a local SQLite index, then exposes a unified layered search pipeline across FTS5, entity graphs, communities, vector embeddings, and hint vectors through a Unix socket daemon. An LLM asks "how does authentication work" and gets back the exact functions, their signatures, and their callers — not a wall of grep output.

## Benchmark Results

Tested against the Signet codebase (505 TypeScript files, 8,886 chunks, 200 queries):

| Lane | Hit@1 | Hit@3 | Hit@5 | Hit@10 | MRR |
|------|-------|-------|-------|--------|-----|
| **FTS** | 86% | 97.5% | 98.5% | 99.5% | 0.915 |
| **Layered** | 85% | 97% | 98.5% | — | 0.907 |

Layered matches FTS on exact-match queries while adding graph, community, and vector retrieval for ambiguous natural-language queries where FTS would fail.

## Why sqmd

LLMs are bad at reading large codebases. They lose context, hallucinate file paths, and can't navigate import chains. sqmd solves this by giving agents structured, scored access to code:

- **Semantic chunks, not files.** Each function, class, struct, and import is indexed individually with its name, signature, line range, and importance score. An agent gets `authenticate(user, token)` — not a 3,000-line file.
- **Entity graph.** Every named symbol — functions, structs, traits, classes, interfaces — becomes a first-class entity with metadata (kind, signature, parent scope). Entities are linked by structural relationships (extends, implements, contains) and import dependencies, forming a multi-layer graph the agent can traverse.
- **Dependency-aware recall.** Import and call graphs plus entity dependencies let an agent trace "who calls this" and "what does this depend on" across files, traversing both relationship layers bidirectionally.
- **Unified layered search.** A single 5-layer retrieval pipeline that runs FTS, graph expansion, community detection, vector KNN, and hint vector search in sequence. Each layer adds results with tuned multipliers and boosts — no alpha-blending, no broken score normalization.
- **mxbai-embed-large.** 1024-dim embeddings via mixedbread.ai's SOTA model for code retrieval, running locally through Ollama. Outperforms OpenAI's text-embedding-3-large on MTEB while being 20x smaller.
- **Template + LLM hints.** Fast AST-derived hints generated during indexing (no LLM needed) plus optional LLM prospective hints via Ollama for natural-language retrieval cues. Both are FTS-indexed and vector-embedded for semantic hint search.
- **Typed communities.** Beyond directory-based summaries, sqmd detects **module communities** (files connected by imports) and **type-hierarchy communities** (entities connected by extends/implements), providing agent-ready summaries of architectural boundaries.
- **Session summaries.** Knowledge batches automatically produce summary chunks with `contains` relationships to children, providing document-level retrieval surface for fragmented knowledge.
- **Ranked retrieval.** Three-factor scoring (relevance x recency x importance) with diversity dampening means the agent sees the most useful code first, not just the highest keyword match.
- **Token-budgeted context.** `sqmd context` assembles a response within a token budget, expanding dependencies only when budget allows. No more dumping entire files into context windows.

## Quick Start

```bash
# Prerequisites: Ollama running with mxbai-embed-large pulled
ollama pull mxbai-embed-large

cargo build --release                         # ~10MB: FTS5 + graph + chunking
cargo build --release --features embed        # + vector embeddings via Ollama
cargo build --release --features embed,ollama # + LLM prospective hints

cd /path/to/your/project
sqmd init     # creates .sqmd/index.db
sqmd index    # tree-sitter parse -> chunk -> store (incremental on re-runs)
sqmd embed    # generate vector embeddings (mxbai-embed-large via Ollama)
sqmd hints    # generate LLM prospective hints (requires --features ollama + running Ollama)
```

Note: `sqmd index --embed` combines indexing and embedding in one step. After generating hints with `sqmd hints`, re-run `sqmd embed` to embed the new hint text into `hints_vec`.

Then point your agent at the Unix socket (`sqmd serve`) or use the CLI directly:

```bash
sqmd search "error handling"                        # layered search (all 5 layers)
sqmd search "error handling" --keyword              # FTS-only
sqmd search "User" --type Struct                    # filter by chunk type
sqmd context --query "how does auth work" --max-tokens 8000 --deps
sqmd deps src/auth.ts --depth 2                     # trace dependency graph
```

## How Agents Use sqmd

1. **Index once.** Run `sqmd index` in the project root. Re-runs are incremental — only changed files are re-parsed.
2. **Search for code.** Ask `sqmd search "database connection pool"` and get back ranked chunks with file paths, line numbers, and surrounding context.
3. **Trace dependencies.** `sqmd deps src/db/pool.rs --depth 2` shows what the pool imports and who imports it, two levels deep.
4. **Assemble context.** `sqmd context --query "how is middleware chained" --max-tokens 4000` returns a token-budgeted bundle of relevant code, automatically expanded with dependency context.

The daemon mode (`sqmd serve`) exposes all of this over a Unix socket with a JSON protocol, so agents can query programmatically without shelling out.

## What Gets Indexed

Every function, method, class, struct, enum, trait, interface, type alias, import, module, and macro definition is extracted as a named chunk with:

- Original source code (raw, not markdown)
- Name and signature (first line, max 120 chars)
- File path, language, line start/end
- Content hash (SHA-256) for incremental updates
- Importance score (0.0-1.0) based on chunk type
- Import relationships (cross-file) and contains relationships (intra-file)
- Template-based hints and (optionally) LLM-generated prospective hints

## Languages

| Language | Grammar | Imports | Chunk types |
|----------|---------|---------|-------------|
| TypeScript / JSX | `tree-sitter-typescript` | `import { X } from '...'` | function, class, interface, type, enum, export |
| TSX | `tree-sitter-typescript` (tsx) | same as TS | same as TS + JSX elements |
| Rust | `tree-sitter-rust` | `use crate::module::Item` | function, struct, enum, trait, impl, mod, const, type, macro |
| Python | `tree-sitter-python` | `from module import X` | function, class, constant |
| Go | `tree-sitter-go` | `"fmt"`, `import ()` blocks | function, method, struct, interface, type |
| Java | `tree-sitter-java` | `import com.example.Class` | method, constructor, class, interface, enum |
| C | `tree-sitter-c` | `#include <...>` / `#include "..."` | function, struct, enum, typedef, macro, constant |
| C++ | `tree-sitter-cpp` | `#include <...>` / `#include "..."` | function, class, struct, enum, namespace, template, type, macro |
| HTML | `tree-sitter-html` | — | element (semantic: header, nav, main, footer, section, article, form) |
| CSS | `tree-sitter-css` | — | rule_set (selectors), media/keyframes/supports (at-rules) |
| CMake | `tree-sitter-cmake` | `find_package`, `add_subdirectory` | function, macro, target, dependency, config |
| QML | `tree-sitter-qmljs` | `import QtQuick 2.15` | component, function, property, import |
| Meson | regex-based (no grammar) | `dependency()`, `subdir()` | target, dependency, function |
| Ruby | `tree-sitter-ruby` | `require '...'` | function, method, class, module, constant |
| YAML | `tree-sitter-yaml` | — | mapping (keyed sections by nesting depth) |
| JSON | `tree-sitter-json` | — | pair (keyed object entries by nesting depth) |
| TOML | `tree-sitter-toml-ng` | — | table, table_array, pair (key-value) |
| Markdown | regex-based (no grammar) | — | section (split by headings, h1-h6) |

All file types are now handled with dedicated chunkers. No more line-based fallbacks.

### HTML Chunker Details

HTML elements are classified by semantic role:

| Element | Chunk type | Rationale |
|---------|-----------|-----------|
| `<html>`, `<body>`, `<head>` | Module | Structural containers |
| `<header>`, `<nav>`, `<main>`, `<footer>`, `<section>`, `<article>`, `<aside>`, `<form>` | Struct | Semantic landmarks |
| `<script>`, `<style>` | Section | Embedded code blocks |
| All others | Section | Generic elements (name extracted, filtered for noise) |

Recognized extensions: `.html`, `.htm`

### CSS Chunker Details

| Construct | Chunk type | Rationale |
|-----------|-----------|-----------|
| `rule_set` (e.g., `.container { ... }`) | Struct | Named selectors with declarations |
| `@media`, `@keyframes`, `@supports`, `@layer` | Module | At-rule blocks with scope |
| Comments | Section | Top-level annotations |

Recognized extensions: `.css`, `.scss`, `.sass`, `.less`

## Search

sqmd provides a unified layered search pipeline with an optional FTS-only mode:

### Layered Search (default)

Five retrieval layers run in sequence, each contributing results with tuned scoring:

1. **FTS5** — Porter-stemmed full-text search across chunk names, signatures, and content. Includes hint boost and graph boost for structurally connected results. Short-circuits if 3+ high-confidence hits found.
2. **Graph expansion** — Traverses entity relationships from FTS query matches (3-hop CTE). Results scored at 0.7x to reflect derived relevance.
3. **Community summaries** — Matches query against module and type-hierarchy communities. Results scored at 0.5x.
4. **Vector KNN** — 1024-dim embeddings via mxbai-embed-large (Ollama). New results scored at 0.6x; existing matches get a +0.3 boost. Requires `--features embed`.
5. **Hint vector** — KNN search over embedded hint text. Existing matches get a +0.2 boost; new results at 0.4x. Requires `--features embed` + `sqmd embed` after `sqmd hints`.

All results are scored with three-factor formula: `relevance x recency x importance`, then importance-boosted and diversity-dampened (same-file clustering penalty).

### FTS-only (`--keyword`)

Raw FTS5 search without graph, community, or vector layers. Useful for exact-match queries.

## Architecture

```
source files
    | tree-sitter (per-language grammar -> AST)
    | walk declarations -> named chunks (function, class, struct, ...)
    | extract imports -> relationship edges
    | extract structural relations -> entity_dependencies (extends, implements, contains)
    | promote symbols -> entities (first-class knowledge graph nodes)
    | generate template hints -> hints (prospective search anchors)
    | detect communities -> module + type-hierarchy groupings
    | content-hash decision pipeline (skip / update / tombstone)
    |
    | [optional, separate step] sqmd hints -> LLM prospective hints (Ollama / phi4-mini)
    |
SQLite database (schema v13)
    |-- chunks         (raw code + metadata)
    |-- chunks_fts     (FTS5 full-text index)
    |-- chunks_vec     (1024-dim vector index, optional)
    |-- relationships  (imports, contains, calls)
    |-- entity_dependencies (structural: extends, implements, contains)
    |-- entities       (symbol-level: files, structs, functions, traits)
    |-- hints + hints_fts (typed relational + prospective search hints)
    |-- hints_vec      (1024-dim vector index over hint text, optional)
    |-- communities    (directory, module, type-hierarchy summaries)
    +-- episodes       (change provenance)
```

Single-pass parsing: tree-sitter parses each file once; the AST is reused for chunking, import extraction, and structural relationship extraction. Incremental re-indexing uses content hashes — unchanged files produce zero writes.

## Feature Flags

| Feature | Dependencies | Purpose |
|---------|-------------|---------|
| `embed` | `ureq` | Vector embeddings (mxbai-embed-large, 1024-dim, via Ollama) |
| `ollama` | `ureq` | LLM prospective hint generation via Ollama API |

Configuration:
- `OLLAMA_HOST` — Ollama server URL (default: `http://localhost:11434`)
- `SQMD_EMBED_MODEL` — Model for embeddings (default: `mxbai-embed-large`)
- `SQMD_HINT_MODEL` — Model for prospective hint generation (default: `phi4-mini`)

## Build & Size

| Build | Size | What's included |
|-------|------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, daemon, 18 languages |
| `cargo build --release --features embed` | ~10MB | + vector search via Ollama |
| `cargo build --release --features embed,ollama` | ~10MB | + LLM hint generation via `sqmd hints` |

## Commands

```bash
sqmd init                            # create index at .sqmd/index.db
sqmd index                           # full or incremental index
sqmd index --embed                   # index + generate embeddings
sqmd embed                           # generate embeddings for unembedded chunks
sqmd hints                           # generate LLM prospective hints (requires ollama feature)
sqmd hints --min-importance 0.7      # only high-importance chunks
sqmd hints --limit 100               # process at most 100 chunks

sqmd search "auth"                   # layered search (all 5 layers)
sqmd search "auth" --keyword         # FTS-only
sqmd search "config" --file src/lib  # file-filtered search

sqmd deps src/auth.ts                # imports + dependents
sqmd deps src/auth.ts --depth 2      # traverse 2 levels

sqmd context --query "how does X work" --max-tokens 8000 --deps
sqmd context --files a.ts,b.ts --max-tokens 4000

sqmd ls                              # list chunks
sqmd ls --type function              # filter by type
sqmd cat 42                          # get chunk by ID
sqmd get src/auth.ts:42              # get chunk at file:line
sqmd stats                           # index statistics

sqmd serve                           # Unix socket daemon
sqmd watch                           # live re-index on file changes
```

## Benchmarking

sqmd includes a benchmark harness (`sqmd-bench`) with multiple subcommands:

```bash
# Run existing ground-truth queries against an index
cargo run -p sqmd-bench --features embed,ollama -- run /path/to/index.db layered

# Generate eval queries from indexed chunks (uses Ollama if available)
cargo run -p sqmd-bench -- generate /path/to/index.db --output queries.json

# Compare retrieval lanes head-to-head
cargo run -p sqmd-bench --features embed,ollama -- compare /path/to/index.db --ground-truth queries.json
```

The `compare` subcommand runs queries through fts and layered lanes and computes Hit@1, Hit@3, Hit@5, and MRR for each, producing a side-by-side comparison table.

### Benchmark methodology

- **Dataset**: Real TypeScript codebase (Signet) — 505 files, 8,886 chunks, 3,547 relationships
- **Queries**: 200 randomly sampled function/method/class/interface names, lowercased with spaces
- **Evaluation**: Hit@K (is the target chunk in top K results?) and MRR (mean reciprocal rank)

## Changelog

### v3.0.0

- **Unified layered search.** Replaced hybrid search with a single 5-layer pipeline: FTS, graph expansion, community summaries, vector KNN, hint vector. No more `--alpha` flag or broken score normalization.
- **mxbai-embed-large.** Switched from nomic-embed-text (768d) to mxbai-embed-large (1024d). MTEB SOTA for code retrieval at 670MB. Embed truncation reduced to 1500 chars for 512-token context. Batch size reduced to 8 to prevent Ollama API errors on large chunks.
- **Schema v13.** Auto-migration recreates vector tables at 1024 dimensions. Existing embeddings cleared — requires `sqmd embed` after upgrade.
- **Signet benchmark.** 86% Hit@1, 98.5% Hit@5, 99.5% Hit@10 on 200 queries against 8,886 chunks.
- **Removed.** `hybrid_search()`, `merge_results()`, `merge_hint_vec_results()`, `--alpha` CLI flag, ONNX runtime, nomic-embed-text prefix system.

### v2.2.0

- Shortened LLM hint prompts, increased embed batch size
- Switched embeddings to Ollama nomic-embed-text, removed ONNX runtime
- Fixed hybrid search scoring, decoupled Ollama hints from indexing

## Daemon Protocol

`sqmd serve` listens on `~/.sqmd/daemon.sock`:

```json
{"method": "search", "params": {"query": "authentication", "top_k": 10}}
{"method": "layered_search", "params": {"query": "how does auth work", "top_k": 10}}
{"method": "context", "params": {"query": "how does auth work", "max_tokens": 8000, "include_deps": true}}
{"method": "deps", "params": {"path": "src/auth.ts", "depth": 2}}
{"method": "index_file", "params": {"path": "src/main.rs"}}
{"method": "stats", "params": {}}
```

All responses are JSON. Add `--json` to any CLI command for machine-readable output.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch policy, code style, and how to add a new language.

## License

MIT
