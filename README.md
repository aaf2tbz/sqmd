Note from developer: This system is currently undergoing massive structural changes, and does not include a visual interface to interact with yet. Please be careful when using sqmd for now. Thanks for trying it!

# sqmd

**Code intelligence for AI agents. Drop any project in, get instant semantic search, dependency graphs, and structured recall — no network, no API keys, one binary.**

sqmd parses your codebase with tree-sitter, chunks every function, class, struct, and import into a local SQLite index, then exposes FTS5 + vector hybrid search, import/call relationship graphs, and token-budgeted context assembly through a Unix socket daemon. An LLM asks "how does authentication work" and gets back the exact functions, their signatures, and their callers — not a wall of grep output.

## Why sqmd

LLMs are bad at reading large codebases. They lose context, hallucinate file paths, and can't navigate import chains. sqmd solves this by giving agents structured, scored access to code:

- **Semantic chunks, not files.** Each function, class, struct, and import is indexed individually with its name, signature, line range, and importance score. An agent gets `authenticate(user, token)` — not a 3,000-line file.
- **Entity graph.** Every named symbol — functions, structs, traits, classes, interfaces — becomes a first-class entity with metadata (kind, signature, parent scope). Entities are linked by structural relationships (extends, implements, contains) and import dependencies, forming a multi-layer graph the agent can traverse.
- **Dependency-aware recall.** Import and call graphs plus entity dependencies let an agent trace "who calls this" and "what does this depend on" across files, traversing both relationship layers bidirectionally.
- **Relational hints.** Prospective search hints are generated from the entity graph (e.g., "items that implement Trait" or "functions in this module"), giving search a +15% relevance boost for structurally connected results.
- **Hybrid search.** FTS5 keyword and 768-dim vector search (nomic-embed-text-v1.5) are alpha-blended with normalized scoring so neither signal dominates. Results that match both signals rank highest; single-signal results get proportional credit.
- **LLM-generated prospective hints.** Optional integration with Ollama (gemma3:4b) generates natural-language retrieval cues per chunk — bridging the semantic gap between how developers search and what code actually says. Run as a separate post-indexing step via `sqmd hints`.
- **Semantic hint retrieval.** Hint text is embedded alongside chunk content, enabling vector KNN search over hints. Hint matches boost existing hybrid scores without overriding them.
- **Typed communities.** Beyond directory-based summaries, sqmd detects **module communities** (files connected by imports) and **type-hierarchy communities** (entities connected by extends/implements), providing agent-ready summaries of architectural boundaries.
- **Session summaries.** Knowledge batches automatically produce summary chunks with `contains` relationships to children, providing document-level retrieval surface for fragmented knowledge.
- **Ranked retrieval.** Three-factor scoring (relevance × recency × importance) with diversity dampening means the agent sees the most useful code first, not just the highest keyword match.
- **Token-budgeted context.** `sqmd context` assembles a response within a token budget, expanding dependencies only when budget allows. No more dumping entire files into context windows.

## Quick Start

```bash
cargo build --release                         # ~10MB: FTS5 + graph + chunking
cargo build --release --features embed        # ~27MB: + vector embeddings + hybrid search
cargo build --release --features embed,ollama # ~27MB: + LLM prospective hints (requires Ollama)

cd /path/to/your/project
sqmd init     # creates .sqmd/index.db
sqmd index    # tree-sitter parse → chunk → store (incremental on re-runs)
sqmd embed    # generate vector embeddings (nomic-embed-text-v1.5, local ONNX)
sqmd hints    # generate LLM prospective hints (requires --features ollama + running Ollama)
```

Note: `sqmd index --embed` combines indexing and embedding in one step. After generating hints with `sqmd hints`, re-run `sqmd embed` to embed the new hint text into `hints_vec`.

Then point your agent at the Unix socket (`sqmd serve`) or use the CLI directly:

```bash
sqmd search "error handling"                        # keyword search
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
- Importance score (0.0–1.0) based on chunk type
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
| Markdown | regex-based (no grammar) | — | section (split by headings, h1–h6) |

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

sqmd provides three search modes, with a fourth optional lane when hint embeddings are available:

- **FTS5 keyword** — Porter-stemmed full-text search across chunk names, signatures, and content. ~20ms.
- **Vector semantic** — 768-dim embeddings via nomic-embed-text-v1.5 (ONNX, quantized, cached locally). Requires `--features embed`.
- **Hybrid** — Normalized alpha-blended merge of FTS and vector scores. Both signals are scaled to [0,1] before blending so neither dominates due to raw score magnitude. Default 70% vector / 30% keyword. Results with both signals rank highest; single-signal results get proportional credit. ~40ms.
- **Hint vector** (optional) — When hint embeddings exist, an additional KNN search over hint text runs in parallel with content vector search. Hint matches additively boost existing hybrid scores (25% of max score × hint relevance) rather than overriding them.

Results are scored with a three-factor formula: `relevance × recency × importance`, then importance-boosted and diversity-dampened (same-file clustering penalty) so the agent sees diverse, high-value results.

Multi-layer retrieval short-circuits: if FTS produces 3+ high-confidence hits, it skips graph and community expansion.

### Recall Evaluation

Retrieval quality was validated against findings from the [Obsidian Vault Recall Eval](https://github.com/Signet-AI/signetai/blob/nicholaivogel/tune-prompt-submit-recall/docs/research/technical/RESEARCH-OBSIDIAN-VAULT-RECALL-EVAL.md), which tested 5 retrieval lanes on 385 real notes. The key findings that shaped sqmd's search architecture:

| Finding | sqmd implementation |
|---------|-------------------|
| Hybrid retrieval beats lexical-only by +12.5% Hit@1 | `hybrid_search()` with alpha-blended FTS + vector |
| Note-level retrieval beats chunked on knowledge | Session summaries in `ingest_batch()` |
| Prospective hint FTS adds +0.8% Hit@1 | `hints_fts` merged in `fts_search_inner()` |
| Semantic retrieval over hints adds +2.6% Hit@1 | `hints_vec` KNN merged in `hybrid_search()` |

## Architecture

```
source files
    ↓ tree-sitter (per-language grammar → AST)
    ↓ walk declarations → named chunks (function, class, struct, ...)
    ↓ extract imports → relationship edges
    ↓ extract structural relations → entity_dependencies (extends, implements, contains)
    ↓ promote symbols → entities (first-class knowledge graph nodes)
    ↓ generate template hints → hints (prospective search anchors)
    ↓ detect communities → module + type-hierarchy groupings
    ↓ content-hash decision pipeline (skip / update / tombstone)
    ↓
    ↓ [optional, separate step] sqmd hints → LLM prospective hints (Ollama / gemma3:4b)
    ↓
SQLite database (schema v12)
    ├── chunks         (raw code + metadata)
    ├── chunks_fts     (FTS5 full-text index)
    ├── chunks_vec     (768-dim vector index, optional)
    ├── relationships  (imports, contains, calls)
    ├── entity_dependencies (structural: extends, implements, contains)
    ├── entities       (symbol-level: files, structs, functions, traits)
    ├── hints + hints_fts (typed relational + prospective search hints)
    ├── hints_vec      (768-dim vector index over hint text, optional)
    ├── communities    (directory, module, type-hierarchy summaries)
    └── episodes       (change provenance)
```

Single-pass parsing: tree-sitter parses each file once; the AST is reused for chunking, import extraction, and structural relationship extraction. Incremental re-indexing uses content hashes — unchanged files produce zero writes.

## Feature Flags

| Feature | Dependencies | Purpose |
|---------|-------------|---------|
| `embed` | `ort`, `ndarray`, `ureq`, `tokenizers` | Vector embeddings (nomic-embed-text-v1.5, ONNX) |
| `ollama` | `ureq` | LLM prospective hint generation via Ollama API |

These are intentionally separate:
- **`embed`** runs nomic-embed-text-v1.5 locally via ONNX for vector search
- **`ollama`** calls a local Ollama server (default: gemma3:4b) for generating natural-language retrieval cues via the `sqmd hints` command (decoupled from indexing for performance)

Configuration:
- `OLLAMA_HOST` — Ollama server URL (default: `http://localhost:11434`)
- `SQMD_HINT_MODEL` — Model for prospective hint generation (default: `gemma3:4b`)

## Build & Size

| Build | Size | What's included |
|-------|------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, daemon, 18 languages |
| `cargo build --release --features embed` | ~27MB | + ONNX Runtime, vector search, hybrid scoring |
| `cargo build --release --features embed,ollama` | ~27MB | + LLM hint generation via `sqmd hints` |

## Commands

```bash
sqmd init                            # create index at .sqmd/index.db
sqmd index                           # full or incremental index
sqmd index --embed                   # index + generate embeddings
sqmd embed                           # generate embeddings for unembedded chunks
sqmd hints                           # generate LLM prospective hints (requires ollama feature)
sqmd hints --min-importance 0.7      # only high-importance chunks
sqmd hints --limit 100               # process at most 100 chunks

sqmd search "auth"                   # keyword search
sqmd search "config" --file src/lib  # file-filtered search
sqmd search "parse" --alpha 0.8      # hybrid search (vector weight)

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
cargo run -p sqmd-bench -- run /path/to/index.db layered

# Generate eval queries from indexed chunks (uses Ollama if available)
cargo run -p sqmd-bench -- generate /path/to/index.db --output queries.json

# Compare retrieval lanes head-to-head
cargo run -p sqmd-bench -- compare /path/to/index.db --ground-truth queries.json
```

The `compare` subcommand runs queries through multiple lanes (fts, layered, hybrid) and computes Hit@1, Hit@3, Hit@5, and MRR for each, producing a side-by-side comparison table.

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
