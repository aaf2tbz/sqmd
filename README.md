# sqmd

**Code intelligence for AI agents. Drop any project in, get semantic search, dependency graphs, structured recall, and review-ready context — local-first, no API keys, one binary.**

sqmd parses your codebase with tree-sitter, chunks functions/classes/structs/imports into a local SQLite index, then exposes layered search across FTS5, entity graphs, communities, vector embeddings, and hint vectors. An agent can ask "how does authentication work?" and get the exact functions, signatures, paths, callers, and dependency context it needs.

It is built for agent workflows:

- Keep a local code index fresh while you edit.
- Ask MCP tools for search, context, dependencies, chunks, and stats.
- Re-index changed files from the agent without leaving the conversation.
- Start embeddings in the background and poll progress instead of blocking.
- Use the bundled `sqmd-review` skill for local pre-push review that checks the real git diff first and uses sqmd as extra codebase context.
- Work from linked git worktrees while reusing the main worktree's `.sqmd/index.db`.

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
- [Skills](#skills)
- [Supported Harnesses](#supported-harnesses)
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
cargo build --release

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

Using git worktrees? Run `sqmd init` and `sqmd index` once in the main checkout. When an MCP client starts from a linked worktree, sqmd can find that main-worktree index and still interpret changed file paths relative to the worktree/project root.

## Skills

sqmd ships with ready-to-use skills that plug into OpenCode, Codex, and Claude Code. Copy a skill directory into your tool's skills folder:

```bash
# OpenCode
cp -r skills/sqmd-review ~/.config/opencode/skills/

# Codex
cp -r skills/sqmd-review ~/.codex/skills/

# Claude Code
cp -r skills/sqmd-review ~/.claude/skills/
```

### sqmd-review

Git-connected code review using sqmd-indexed codebase context and GitHub PR history.
Runs locally before pushing to prevent bot review comments before they happen. Adapted
from [pr-reviewer](https://github.com/NicholaiVogel/pr-reviewer).

**How it works:**

1. Detects scope (staged, uncommitted, branch diff, commit, or PR-linked via `gh api`)
2. Reads prior bot review comments and tracks dismissed/rebutted/addressed findings
3. Assembles context from the git diff, `sqmd_deps` blast radius, and `sqmd_search`
   structural search (FTS + entity graph — no embeddings needed)
4. Runs a structured review with anti-hallucination rules and a convention checklist
5. **Iterates** — fixes findings, re-reviews, repeats until verdict is `no_issues`
6. Only then commits and pushes

The goal is **zero-comment pushes**: every issue the remote bot would flag is caught
and fixed locally first. The review reads prior PR comments so dismissed findings
are never re-flagged.

**Works in two modes:**
- **Offline** — uncommitted/staged/branch diffs, no GitHub connection needed
- **PR-aware** — linked PR fetches prior review comments via `gh api` for
  dismissal tracking and verification against stated PR goals

Useful prompts:

- "review my changes"
- "review before push"
- "review this commit"
- "review PR #42"
- "self-review"

## Supported Harnesses

sqmd exposes an MCP server (JSON-RPC 2.0 over stdio) that plugs into AI coding tools. All harnesses get the same tools: `search`, `context`, `deps`, `stats`, `get`, `index_file`, `embed`, `embed_start`, `embed_progress`, `embed_stop`, `ls`, and `cat`.

| Harness | Config path | Format | Setup command |
|---------|------------|--------|---------------|
| **OpenCode** | `~/.config/opencode/opencode.json` | JSON | `sqmd setup opencode` |
| **Codex** | `~/.codex/config.toml` | TOML | `sqmd setup codex` |
| **Claude Code** | `~/.claude/settings.json` | JSON | `sqmd setup claude` |
| **Cursor** | `<project-root>/.cursor/mcp.json` | JSON | `sqmd setup cursor` |

Run `sqmd setup` to register all four at once. Note: Cursor writes to the project root, not your home directory.

### OpenCode

```json
{
  "mcp": {
    "sqmd": {
      "type": "local",
      "command": ["/absolute/path/to/sqmd", "mcp"],
      "enabled": true
    }
  }
}
```

### Codex

```toml
[mcp_servers.sqmd]
command = "/absolute/path/to/sqmd"
args = ["mcp"]
```

### Claude Code

```json
{
  "mcpServers": {
    "sqmd": {
      "command": "/absolute/path/to/sqmd",
      "args": ["mcp"]
    }
  }
}
```

### Cursor

Cursor reads MCP config from the project root (not your home directory). Run `sqmd setup cursor` from within your project:

```json
{
  "mcpServers": {
    "sqmd": {
      "command": "/absolute/path/to/sqmd",
      "args": ["mcp"]
    }
  }
}
```

### Raw JSON-RPC

Any tool that speaks MCP over stdio can use sqmd directly:

```bash
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"my-tool","version":"1.0"}}}' | sqmd mcp
```

Supports both raw JSON lines and `Content-Length:` framed transport.

### Worktrees and Index Discovery

sqmd stores its index at `<project>/.sqmd/index.db`. If you run agents from git worktrees, the MCP server handles the common setup where the index lives in the main worktree:

```bash
cd /path/to/main-checkout
sqmd init
sqmd index

git worktree add ../feature-branch -b feature-branch
cd ../feature-branch
sqmd mcp
```

When launched from the linked worktree, sqmd asks git for the common git directory, finds the main checkout's `.sqmd/index.db`, and still treats file paths as project-relative. When `.sqmd/` lives at `~/.sqmd/` (global index), the MCP server falls back to CWD for path resolution to prevent broken relative paths.

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

### Tree-sitter grammars (17 languages)

| Language | Grammar | Imports | Chunk types |
|----------|---------|---------|-------------|
| TypeScript / JSX | `tree-sitter-typescript` | `import { X } from '...'` | function, class, interface, type, enum, export |
| TSX | `tree-sitter-typescript` (tsx) | same as TS | same as TS + JSX elements |
| JavaScript | `tree-sitter-typescript` | `import ...` / `require(...)` | function, class, export |
| Rust | `tree-sitter-rust` | `use crate::module::Item` | function, struct, enum, trait, impl, mod, const, type, macro |
| Python | `tree-sitter-python` | `from module import X` | function, class, constant |
| Go | `tree-sitter-go` | `"fmt"`, `import ()` blocks | function, method, struct, interface, type |
| Java | `tree-sitter-java` | `import com.example.Class` | method, constructor, class, interface, enum |
| C | `tree-sitter-c` | `#include <...>` / `#include "..."` | function, struct, enum, typedef, macro, constant |
| C++ | `tree-sitter-cpp` | `#include <...>` / `#include "..."` | function, class, struct, enum, namespace, template, type, macro |
| Ruby | `tree-sitter-ruby` | `require '...'` | function, method, class, module, constant |
| HTML | `tree-sitter-html` | — | element (semantic landmarks) |
| CSS | `tree-sitter-css` | — | rule_set, media/keyframes/supports |
| CMake | `tree-sitter-cmake` | `find_package`, `add_subdirectory` | function, macro, target, dependency, config |
| QML | `tree-sitter-qmljs` | `import QtQuick 2.15` | component, function, property, import |
| YAML | `tree-sitter-yaml` | — | mapping (keyed sections) |
| JSON | `tree-sitter-json` | — | pair (keyed entries) |
| TOML | `tree-sitter-toml-ng` | — | table, table_array, pair |
| Markdown | regex-based | — | section (heading splits) + fenced code blocks |
| Meson | regex-based | `dependency()`, `subdir()` | target, dependency, function |

### Line-based chunking (additional 21 languages)

Languages without dedicated tree-sitter grammars still get indexed using line-based chunking. These include: SCSS/SASS, Shell, SQL, Dockerfile, Makefile, Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, XML/SVG, GraphQL, Protobuf, JSX, and JavaScript (fallback).

### Extension coverage

| Language | Extensions |
|----------|-----------|
| TypeScript | `.ts` |
| TSX | `.tsx` |
| JavaScript | `.js`, `.mjs`, `.cjs` |
| JSX | `.jsx` |
| Rust | `.rs` |
| Python | `.py`, `.pyi` |
| Go | `.go` |
| Java | `.java` |
| C | `.c`, `.h` |
| C++ | `.cpp`, `.cc`, `.cxx`, `.hpp`, `.hxx`, `.hh` |
| CMake | `.cmake`, `CMakeLists.txt` |
| QML | `.qml` |
| Meson | `meson.build`, `meson_options.txt` |
| Ruby | `.rb` |
| Markdown | `.md`, `.mdx` |
| JSON | `.json`, `.jsonc` |
| YAML | `.yml`, `.yaml` |
| TOML | `.toml` |
| HTML | `.html`, `.htm` |
| CSS | `.css`, `.less` |
| SCSS | `.scss`, `.sass` |
| Shell | `.sh`, `.bash`, `.zsh`, `.fish` |
| SQL | `.sql` |
| Dockerfile | `Dockerfile`, `dockerfile` |
| Makefile | `Makefile`, `makefile`, `GNUmakefile` |
| Kotlin | `.kt`, `.kts` |
| Swift | `.swift` |
| C# | `.cs` |
| PHP | `.php` |
| Lua | `.lua` |
| Dart | `.dart` |
| Scala | `.scala`, `.sc` |
| Haskell | `.hs` |
| Elixir | `.ex`, `.exs` |
| Zig | `.zig` |
| XML | `.xml`, `.svg`, `.xsl`, `.xslt` |
| GraphQL | `.graphql`, `.gql` |
| Protobuf | `.proto` |

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
SQLite database (schema v14)
    |-- files            (path, language, hash, mtime)
    |-- chunks           (raw code + metadata)
    |-- chunks_fts       (FTS5 full-text index)
    |-- chunks_vec       (1024-dim vector index, float32)
    |-- relationships    (imports, contains, calls)
    |-- entity_dependencies (extends, implements, contains)
    |-- entities         (symbol-level graph nodes)
    |-- entity_aspects   (exports, implementation, constraints)
    |-- entity_attributes (chunk-level entity annotations)
    |-- hints + hints_fts (search hints)
    |-- hints_vec        (1024-dim vector index over hints)
    |-- communities      (module, type-hierarchy summaries)
    |-- embeddings       (vector blob fallback)
    |-- episodes         (change provenance)
    +-- schema_version   (migration tracking)
```

Single-pass parsing with incremental re-indexing via content hashes.

## Feature Flags

| Feature | Dependencies | Purpose |
|---------|-------------|---------|
| `native` (default) | `llama-cpp-2` | Embeddings + hint generation via native llama.cpp |
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
cargo build --release                              # default: native llama.cpp + Metal GPU (macOS)
cargo build --release --no-default-features        # no native features (no embeddings/hints)
cargo build --release --features native             # CPU-only (Linux, macOS Intel, WSL2)
```

Build requirements:
- **Rust** 1.85+ (edition 2024)
- **CMake** (`brew install cmake` on macOS, `sudo apt install cmake` on Linux)
- **C compiler** (Xcode CLI tools on macOS, `build-essential` on Linux)

## Commands

### Indexing

```bash
sqmd init                            # create index at .sqmd/index.db
sqmd index                           # full or incremental index
sqmd index --embed                   # index + generate embeddings
sqmd index src/auth.ts               # index a single file
sqmd embed                           # generate embeddings for unembedded chunks
sqmd watch                           # live re-index on file changes
```

#### Excluding files

sqmd respects `.gitignore` rules and hardcodes common exclusions (`node_modules`, `target`, `.git`, `dist`, `build`, etc.). To add custom exclusions, create a `.sqmdignore` file in your project root using the same format as `.gitignore`:

```gitignore
# .sqmdignore
vendor/
*_test.go
*.generated.ts
docs/generated/
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
sqmd ls --file src/auth.ts           # filter by file
sqmd ls --language rust              # filter by language
sqmd cat 42                          # get chunk by ID
sqmd get src/auth.ts:42              # get chunk at file:line
sqmd stats                           # index statistics
sqmd entities                        # list knowledge graph entities
sqmd entity-deps MyStruct            # show entity dependencies
sqmd diff 2026-04-01T00:00:00Z       # chunks modified since timestamp
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
sqmd doctor --check embed            # check embedding setup specifically
sqmd update                          # update sqmd to latest version
sqmd install                         # install sqmd from source
sqmd reset                           # delete the index
sqmd prune 30                        # purge soft-deleted chunks older than 30 days
```

### Hint Generation

```bash
sqmd hints                           # generate prospective hints (phi4-mini via native llama.cpp)
sqmd hints --min-importance 0.7      # only high-importance chunks
sqmd hints --limit 100               # process at most 100 chunks
```

Safe to re-run — duplicate hints are automatically ignored (deduped on chunk_id + hint_text).

After generating hints, re-run `sqmd embed` to embed the hint text into `hints_vec`.

## MCP Server

`sqmd mcp` starts a JSON-RPC 2.0 server over stdio for use with AI tools:

```bash
sqmd mcp
```

Exposes 12 tools:

| Tool | Description |
|------|-------------|
| `search` | Layered search with query, top_k, file/type/source filters |
| `context` | Assemble token-budgeted context with dependency expansion |
| `deps` | Get dependencies and dependents for a file path |
| `stats` | Index statistics (files, chunks, embeddings, entities, communities) |
| `get` | Get chunk by file path and line number |
| `index_file` | Index a single file or all changed files (incremental) |
| `embed` | Embed unembedded chunks via local llama.cpp (blocking, up to batch_size) |
| `embed_start` | Start embedding in a background thread |
| `embed_progress` | Poll embedding progress, percentage, progress bar, and ETA |
| `embed_stop` | Request a graceful stop after the current batch |
| `ls` | List chunks with file/type/language filters |
| `cat` | Get full chunk content by ID |

Agents can index new files, keep embeddings up-to-date, and browse the codebase entirely through MCP — no CLI needed.

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
