# sqmd

**Code intelligence for AI agents. Drop any project in, get instant semantic search, dependency graphs, and structured recall — no network, no API keys, one binary.**

sqmd parses your codebase with tree-sitter, chunks every function, class, struct, and import into a local SQLite index, then exposes a unified layered search pipeline across FTS5, entity graphs, communities, vector embeddings, and hint vectors. An LLM asks "how does authentication work" and gets back the exact functions, their signatures, and their callers — not a wall of grep output.

## Benchmark Results

Tested against the Signet codebase (505 TypeScript files, 8,886 chunks, 200 queries):

| Lane | Hit@1 | Hit@3 | Hit@5 | Hit@10 | MRR |
|------|-------|-------|-------|--------|-----|
| **FTS** | 86% | 97.5% | 98.5% | 99.5% | 0.915 |
| **Layered** | 86% | 97.5% | 98.5% | 99.5% | 0.915 |

Performance: ~0.55s per query, ~19 q/sec batch throughput. Native llama.cpp with Metal GPU acceleration on Apple Silicon.

See [BENCHMARKING.md](BENCHMARKING.md) for methodology and reproduction steps.

## Table of Contents

- [Why sqmd](#why-sqmd)
- [Quick Start](#quick-start)
- [Connecting to AI Tools](#connecting-to-ai-tools)
- [What Gets Indexed](#what-gets-indexed)
- [Languages](#languages)
- [Search](#search)
- [Architecture](#architecture)
- [Feature Flags](#feature-flags)
- [Supported Platforms](#supported-platforms)
- [Build](#build)
- [Commands](#commands)
- [MCP Server](#mcp-server)
- [Daemon Protocol](#daemon-protocol)
- [Benchmarking](#benchmarking)
- [Changelog](#changelog)
- [Contributing](#contributing)
- [License](#license)

## Why sqmd

LLMs are bad at reading large codebases. They lose context, hallucinate file paths, and can't navigate import chains. sqmd solves this by giving agents structured, scored access to code:

- **Semantic chunks, not files.** Each function, class, struct, and import is indexed individually with its name, signature, line range, and importance score.
- **Entity graph.** Every named symbol becomes a first-class entity linked by structural relationships (extends, implements, contains) and import dependencies.
- **Dependency-aware recall.** Import and call graphs let an agent trace "who calls this" across files, bidirectionally.
- **Unified layered search.** 5-layer pipeline: FTS, graph expansion, community detection, vector KNN, hint vector. No alpha-blending.
- **Native llama.cpp runtime.** Embeddings run locally via llama.cpp with Metal GPU offloading. No external services required.
- **MCP server.** JSON-RPC over stdio — plug sqmd directly into OpenCode, Codex, or Claude Code.
- **Typed communities.** Module communities (files connected by imports) and type-hierarchy communities (extends/implements).
- **Ranked retrieval.** Three-factor scoring (relevance x recency x importance) with diversity dampening.
- **Token-budgeted context.** Assembles responses within a token budget, expanding dependencies only when budget allows.

## Quick Start

```bash
cargo build --release --features native

cd /path/to/your/project
sqmd init           # creates .sqmd/index.db
sqmd index          # tree-sitter parse -> chunk -> store (incremental on re-runs)
sqmd index --embed  # index + generate embeddings in one step
sqmd embed          # generate vector embeddings (mxbai-embed-large via native llama.cpp)
```

sqmd looks for `mxbai-embed-large` GGUF in your local model store (`~/.ollama/models/` or set `SQMD_NATIVE_MODEL` to a GGUF path). For hint generation, set `SQMD_HINT_MODEL` (default: `phi4-mini`) and ensure the GGUF is available.

```bash
sqmd search "error handling"                        # layered search (all 5 layers)
sqmd search "error handling" --keyword              # FTS-only
sqmd search "User" --type Struct                    # filter by chunk type
sqmd context --query "how does auth work" --max-tokens 8000 --deps
sqmd deps src/auth.ts --depth 2                     # trace dependency graph
```

## Connecting to AI Tools

sqmd includes an MCP server that works with OpenCode, Codex, and Claude Code:

```bash
sqmd setup                   # register sqmd in all harness configs
sqmd setup opencode          # OpenCode only (~/.config/opencode/opencode.json)
sqmd setup codex             # Codex only (~/.config/codex/config.json)
sqmd setup claude            # Claude Code only (~/.claude/settings.json)
```

This writes the MCP server config into each tool's settings so agents can call `sqmd search`, `sqmd context`, `sqmd deps`, `sqmd stats`, and `sqmd get` directly.

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
| HTML | `tree-sitter-html` | — | element (semantic landmarks) |
| CSS | `tree-sitter-css` | — | rule_set, media/keyframes/supports |
| CMake | `tree-sitter-cmake` | `find_package`, `add_subdirectory` | function, macro, target, dependency, config |
| QML | `tree-sitter-qmljs` | `import QtQuick 2.15` | component, function, property, import |
| Meson | regex-based | `dependency()`, `subdir()` | target, dependency, function |
| Ruby | `tree-sitter-ruby` | `require '...'` | function, method, class, module, constant |
| YAML | `tree-sitter-yaml` | — | mapping (keyed sections) |
| JSON | `tree-sitter-json` | — | pair (keyed entries) |
| TOML | `tree-sitter-toml-ng` | — | table, table_array, pair |
| Markdown | regex-based | — | section (heading splits) |

All file types use dedicated chunkers. No line-based fallbacks.

## Search

### Layered Search (default)

Five retrieval layers run in sequence, each contributing results with tuned scoring:

1. **FTS5** — Porter-stemmed full-text search. Includes hint boost and graph boost. Short-circuits on 3+ high-confidence hits.
2. **Graph expansion** — 3-hop entity relationship traversal. 0.7x multiplier.
3. **Community summaries** — Module and type-hierarchy community matching. 0.5x multiplier.
4. **Vector KNN** — 1024-dim mxbai-embed-large via native llama.cpp. New results at 0.6x; existing matches get +0.3 boost.
5. **Hint vector** — KNN over embedded hint text. Existing matches +0.2; new at 0.4x.

All results scored with `relevance x recency x importance`, then importance-boosted and diversity-dampened.

### FTS-only (`--keyword`)

Raw FTS5 search without graph, community, or vector layers.

## Architecture

```
source files
    | tree-sitter (per-language grammar -> AST)
    | walk declarations -> named chunks (function, class, struct, ...)
    | extract imports -> relationship edges
    | extract structural relations -> entity_dependencies
    | promote symbols -> entities (knowledge graph nodes)
    | generate template hints -> hints (search anchors)
    | detect communities -> module + type-hierarchy groupings
    | content-hash pipeline (skip / update / tombstone)
    |
    | [optional] sqmd hints -> LLM hints (phi4-mini via native llama.cpp)
    |
    | native llama.cpp -> mxbai-embed-large embeddings (Metal GPU)
    |
SQLite database (schema v13)
    |-- chunks         (raw code + metadata)
    |-- chunks_fts     (FTS5 full-text index)
    |-- chunks_vec     (1024-dim vector index)
    |-- relationships  (imports, contains, calls)
    |-- entity_dependencies (extends, implements, contains)
    |-- entities       (symbol-level graph nodes)
    |-- hints + hints_fts (search hints)
    |-- hints_vec      (1024-dim vector index over hints)
    |-- communities    (module, type-hierarchy summaries)
    +-- episodes       (change provenance)
```

Single-pass parsing with incremental re-indexing via content hashes.

## Feature Flags

| Feature | Dependencies | Purpose |
|---------|-------------|---------|
| `native` (default) | `llama-cpp-2` | Embeddings + text generation via native llama.cpp |
| `native-metal` (default on macOS) | `llama-cpp-2/metal` | + Metal GPU acceleration |

No external services required. All inference runs locally through llama.cpp.

Configuration:
- `SQMD_NATIVE_MODEL` — Path to GGUF file or model name (default: auto-discover `mxbai-embed-large` from model store)
- `OLLAMA_MODELS` — Path to model store (default: `~/.ollama/models`)
- `SQMD_HINT_MODEL` — Hint generation model (default: `phi4-mini`)
- `SQMD_HINT_MODEL_PATH` — Direct path to hint model GGUF

## Supported Platforms

| OS | Architecture | GPU | Feature flag |
|----|-------------|-----|-------------|
| macOS 13+ (Ventura) | Apple Silicon (M1/M2/M3/M4) | Metal | `native-metal` (default) |
| macOS 13+ | Intel | CPU | `native` |
| Linux | x86_64, ARM64 | CPU | `native` |
| Windows (WSL2) | x86_64 | CPU | `native` |

Native Windows support (without WSL2) is not yet tested. All inference runs through llama.cpp — Metal GPU on Apple Silicon, CPU fallback on everything else.

## Build

```bash
cargo build --release               # default: native llama.cpp + Metal GPU (macOS)
cargo build --release --features native  # CPU-only (Linux, macOS Intel, WSL2)
```

Build requirements:
- **Rust** 1.80+ (`rustup`)
- **CMake** (`brew install cmake` on macOS, `sudo apt install cmake` on Linux)
- **C compiler** (Xcode CLI tools on macOS, `build-essential` on Linux)

## Commands

### Indexing

```bash
sqmd init                            # create index at .sqmd/index.db
sqmd index                           # full or incremental index
sqmd index --embed                   # index + generate embeddings
sqmd embed                           # generate embeddings for unembedded chunks
sqmd watch                           # live re-index on file changes
```

### Search & Retrieval

```bash
sqmd search "auth"                   # layered search (all 5 layers)
sqmd search "auth" --keyword         # FTS-only
sqmd search "config" --file src/lib  # file-filtered search
sqmd search "User" --type Struct     # type-filtered search

sqmd deps src/auth.ts                # imports + dependents
sqmd deps src/auth.ts --depth 2      # traverse 2 levels

sqmd context --query "how does X work" --max-tokens 8000 --deps
sqmd context --files a.ts,b.ts --max-tokens 4000
```

### Browsing

```bash
sqmd ls                              # list chunks (tree view)
sqmd ls --type function              # filter by type
sqmd cat 42                          # get chunk by ID
sqmd get src/auth.ts:42              # get chunk at file:line
sqmd stats                           # index statistics
sqmd entities                        # list knowledge graph entities
```

### Lifecycle

```bash
sqmd start                           # start daemon in background
sqmd stop                            # stop running daemon
sqmd serve                           # run daemon in foreground (Unix socket)
sqmd mcp                             # start MCP server (JSON-RPC over stdio)
sqmd setup                           # register sqmd in all AI tool configs
sqmd setup opencode                  # register for OpenCode only
sqmd doctor                          # run diagnostic checks
sqmd update                          # update sqmd to latest version
sqmd install                         # install sqmd from source
```

### Hint Generation

```bash
sqmd hints                           # generate prospective hints (phi4-mini via native llama.cpp)
sqmd hints --min-importance 0.7      # only high-importance chunks
sqmd hints --limit 100               # process at most 100 chunks
```

After generating hints, re-run `sqmd embed` to embed the hint text into `hints_vec`.

## MCP Server

`sqmd mcp` starts a JSON-RPC 2.0 server over stdio for use with AI tools:

```bash
sqmd mcp
```

Exposes 5 tools:

| Tool | Description |
|------|-------------|
| `search` | Layered search with query, top_k, file/type filters |
| `context` | Assemble token-budgeted context with dependency expansion |
| `deps` | Get dependencies and dependents for a file path |
| `stats` | Index statistics (files, chunks, embeddings, relationships) |
| `get` | Get chunk by file path and line number |

Register with `sqmd setup` or manually add to your tool's config:

**OpenCode** (`~/.config/opencode/opencode.json`):
```json
{
  "mcp": {
    "sqmd": {
      "type": "local",
      "command": ["sqmd", "mcp"],
      "enabled": true
    }
  }
}
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

## Benchmarking

See [BENCHMARKING.md](BENCHMARKING.md) for full methodology, reproduction steps, and historical results across datasets.

```bash
cargo run -p sqmd-bench --features native -- compare /path/to/index.db --ground-truth queries.json
```

## Changelog

See [CHANGELOG.md](CHANGELOG.md) for the full version history.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for branch policy, code style, and how to add a new language.

## License

MIT
