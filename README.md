# sqmd

**Code intelligence for AI agents. Drop any project in, get instant semantic search, dependency graphs, and structured recall — no network, no API keys, one binary.**

sqmd parses your codebase with tree-sitter, chunks every function, class, struct, and import into a local SQLite index, then exposes FTS5 + vector hybrid search, import/call relationship graphs, and token-budgeted context assembly through a Unix socket daemon. An LLM asks "how does authentication work" and gets back the exact functions, their signatures, and their callers — not a wall of grep output.

## Why sqmd

LLMs are bad at reading large codebases. They lose context, hallucinate file paths, and can't navigate import chains. sqmd solves this by giving agents structured, scored access to code:

- **Semantic chunks, not files.** Each function, class, struct, and import is indexed individually with its name, signature, line range, and importance score. An agent gets `authenticate(user, token)` — not a 3,000-line file.
- **Dependency-aware recall.** Import and call graphs let an agent trace "who calls this" and "what does this depend on" across files, traversing the relationship graph bidirectionally.
- **Ranked retrieval.** Three-factor scoring (relevance × recency × importance) with diversity dampening means the agent sees the most useful code first, not just the highest keyword match.
- **Token-budgeted context.** `sqmd context` assembles a response within a token budget, expanding dependencies only when budget allows. No more dumping entire files into context windows.

## Quick Start

```bash
cargo build --release                     # ~10MB: FTS5 + graph + chunking
cargo build --release --features embed    # ~27MB: + vector embeddings + hybrid search

cd /path/to/your/project
sqmd init     # creates .sqmd/index.db
sqmd index    # tree-sitter parse → chunk → store (incremental on re-runs)
```

Note: `sqmd index --embed` generates vector embeddings for hybrid search. This uses nomic-embed-text-v1.5 locally via ONNX and may take a while on large codebases.

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
| CMake | `tree-sitter-cmake` | `find_package`, `add_subdirectory` | function, macro, target, dependency, config |
| QML | `tree-sitter-qmljs` | `import QtQuick 2.15` | component, function, property, import |
| Meson | regex-based (no grammar) | `dependency()`, `subdir()` | target, dependency, function |
| Ruby | `tree-sitter-ruby` | `require '...'` | function, method, class, module, constant |
| YAML | `tree-sitter-yaml` | — | mapping (keyed sections by nesting depth) |
| JSON | `tree-sitter-json` | — | pair (keyed object entries by nesting depth) |
| TOML | `tree-sitter-toml-ng` | — | table, table_array, pair (key-value) |
| Markdown | regex-based (no grammar) | — | section (split by headings, h1–h6) |

All file types are now handled with dedicated chunkers. No more line-based fallbacks.

## Search

sqmd provides three search modes:

- **FTS5 keyword** — Porter-stemmed full-text search across chunk names, signatures, and content. ~20ms.
- **Vector semantic** — 768-dim embeddings via nomic-embed-text-v1.5 (ONNX, quantized, cached locally). Requires `--features embed`.
- **Hybrid** — Alpha-blended merge of both. Default 70% vector / 30% keyword with single-source penalty. ~40ms.

Results are scored with a three-factor formula: `relevance × recency × importance`, then importance-boosted and diversity-dampened (same-file clustering penalty) so the agent sees diverse, high-value results.

Multi-layer retrieval short-circuits: if FTS produces 3+ high-confidence hits, it skips graph and community expansion.

## Architecture

```
source files
    ↓ tree-sitter (per-language grammar → AST)
    ↓ walk declarations → named chunks (function, class, struct, ...)
    ↓ extract imports → relationship edges
    ↓ content-hash decision pipeline (skip / update / tombstone)
    ↓
SQLite database
    ├── chunks         (raw code + metadata)
    ├── chunks_fts     (FTS5 full-text index)
    ├── chunks_vec     (768-dim vector index, optional)
    ├── relationships  (imports, contains, calls)
    ├── entities       (files, structs, functions — knowledge graph)
    ├── hints + hints_fts (prospective search hints)
    ├── communities    (directory-based summaries)
    └── episodes       (change provenance)
```

Single-pass parsing: tree-sitter parses each file once; the AST is reused for both chunking and import extraction. Incremental re-indexing uses content hashes — unchanged files produce zero writes.

## Build & Size

| Build | Size | What's included |
|-------|------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, daemon, 16 languages |
| `cargo build --release --features embed` | ~27MB | + ONNX Runtime, vector search, hybrid scoring |

## Commands

```bash
sqmd init                            # create index at .sqmd/index.db
sqmd index                           # full or incremental index
sqmd index --embed                   # index + generate embeddings

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
