# sqmd

**Local-first code intelligence for AI agents. A single Rust binary, zero network.**

sqmd indexes any codebase into a SQLite database of semantically chunked source code with tree-sitter parsing, FTS5 keyword search, vector embeddings, and an import/call relationship graph. Query in milliseconds.

| Build | Stripped size | What's included |
|-------|--------------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, call graph, daemon |
| `cargo build --release --features embed` | ~27MB | + ONNX Runtime, vector search, hybrid scoring |

## The Problem

AI agents read code one file at a time, grep for keywords without understanding structure, and burn tokens on irrelevant context. There's no fast, offline way to ask "find the auth middleware and everything it depends on" and get a precise, token-efficient answer.

## How sqmd Works

```
source files
    │
    ▼ tree-sitter (per-language AST)
┌──────────────────────────────┐
│  Chunk: raw code + metadata  │
│  name, signature, type,      │
│  line range, importance,     │
│  file path, language         │
└──────┬───────────────────────┘
       │
       ├──► SQLite chunks table (structured data + raw code)
       ├──► FTS5 index (keyword search on code + names)
       ├──► sqlite-vec (768-dim vector embeddings, KNN)
       └──► relationships table (imports + contains + calls)
                   │
                   ▼
          Hybrid Search Engine
          (FTS5 + vector + graph)
                   │
                   ▼
          Chunk::render_md() → on-demand Markdown
                   │
                   ▼
          Agent context injection
```

**Key design choice:** sqmd stores raw source code in the database, not pre-rendered Markdown. Markdown is derived on demand via `Chunk::render_md()` at query time. This keeps the source of truth in the code itself and avoids stale renderings.

## Quick Start

```bash
# Build from source
cargo build --release                          # ~10MB: FTS5 + graph + chunking
cargo build --release --features embed          # ~27MB: + vector embeddings + hybrid search
# Binary at target/release/sqmd

# Index your project
cd /path/to/your/project
sqmd init          # creates .sqmd/index.db, updates .gitignore
sqmd index         # tree-sitter parse → chunk → store

# Search (FTS5 keyword — available in default build)
sqmd search "error handling"               # keyword search
sqmd search "User" --type Struct           # filter by chunk type
sqmd search "config" --file src/config.rs  # filter by file

# With --features embed: hybrid search + embeddings
# sqmd index --embed                        # index + generate vector embeddings
# sqmd embed                                # embed all unembedded chunks
# sqmd search "error handling"               # hybrid (FTS5 + vector)
# sqmd search "authenticate" --keyword       # keyword-only
# sqmd search "parsing" --alpha 0.8          # weight vector over FTS5

# Get a specific chunk
sqmd get src/auth.ts:42       # chunk at file:line (renders with language fence)

# Dependency graph
sqmd deps src/auth.ts         # what this file imports + what imports it
sqmd deps src/auth.ts --depth 2  # traverse 2 levels deep

# Context assembly (for agents)
sqmd context --query "how does auth work" --max-tokens 8000 --deps --dep-depth 1
sqmd context --files src/auth.ts,src/middleware.ts --max-tokens 4000

# Daemon mode (Unix socket)
sqmd serve    # starts background watcher + incremental re-index, listens on ~/.sqmd/daemon.sock

# Watch mode
sqmd watch    # live re-index on file changes

# Reset and rebuild
sqmd reset && sqmd index
```

## What Gets Indexed

| Chunk Type | Examples | Importance |
|-----------|----------|------------|
| Function | `fn main()`, `def process()`, `const authenticate = ()`, `func Handle()` | 0.9 |
| Method | `impl Block for Transaction { fn execute() }`, `func (s *Server) Start()` | 0.85 |
| Class/Struct/Enum | `struct User`, `class Database`, `enum Result`, `type Config struct` | 0.85 |
| Interface/Trait/Type | `trait Read`, `interface Handler`, `type Config` | 0.8 |
| Impl block | `impl User { ... }` | 0.7 |
| Import | `import { X }`, `use crate::module`, `from module import X` | 0.3 |
| Module/Section | Top-level unclaimed code, file-level constants | 0.2-0.5 |

Each chunk stores: raw source code, file path, language, line range, name, signature, importance score. Unclaimed lines between declarations are grouped into section chunks (max ~50 lines).

## Languages Supported

| Language | Grammar | Status |
|----------|---------|--------|
| TypeScript | `tree-sitter-typescript` | Shipped |
| TSX | `tree-sitter-typescript` (tsx variant) | Shipped |
| Rust | `tree-sitter-rust` | Shipped |
| Python | `tree-sitter-python` | Shipped |
| Go | `tree-sitter-go` | Shipped |
| Java | `tree-sitter-java` | Shipped |

Other languages fall back to a line-based `FileChunker` that splits at section boundaries.

## Relationships

sqmd extracts three kinds of relationships automatically:

- **`imports`** — cross-file: `import { X } from './path'`, `use crate::module::Item`, `from module import X`, `"fmt"`, `import java.net.http`
- **`contains`** — intra-file: class→method, impl→method, module→function, trait→method, struct→fields
- **`calls`** — cross-file: regex-based call graph extraction resolved against imported symbols

Query with `sqmd deps <file> --depth N` to traverse the graph bidirectionally.

## Hybrid Search

sqmd blends two search modes with configurable alpha weighting:

- **FTS5** (keyword): fast exact/near-match on code text, function names, signatures
- **Vector KNN** (semantic): cosine similarity via sqlite-vec on 768-dim embeddings (nomic-embed-text-v1.5)

Default: `alpha=0.7` (70% vector, 30% keyword). Single-source penalty (0.8) downranks chunks that appear in only one ranking.

Embeddings use ONNX Runtime (ort) with a quantized model cached at `~/.sqmd/models/`. Auto-downloads on first `sqmd embed` if missing.

## Architecture

```
sqmd/
├── crates/
│   ├── sqmd-core/          # library
│   │   ├── src/
│   │   │   ├── schema.rs       # SQLite schema + migrations + chunks_vec
│   │   │   ├── chunk.rs        # Chunk struct + ChunkType + render_md()
│   │   │   ├── chunker.rs      # LanguageChunker trait + FileChunker fallback
│   │   │   ├── index.rs        # Transactional indexer (chunks + contains + calls)
│   │   │   ├── embed.rs        # ONNX embedding (ort) + auto-download
│   │   │   ├── search.rs       # FTS5 + vector hybrid search engine
│   │   │   ├── relationships.rs # Import resolution + call graph + CTE depth traversal
│   │   │   ├── context.rs      # Context assembly + token budgeting
│   │   │   ├── daemon.rs       # Unix socket daemon + JSON protocol
│   │   │   ├── watcher.rs      # notify file watcher + 200ms debounce
│   │   │   ├── files.rs        # Language detection + file walking + hashing
│   │   │   └── languages/
│   │   │       ├── typescript.rs  # TS/TSX chunker + import extraction + JSX
│   │   │       ├── rust.rs        # Rust chunker + use/impl extraction
│   │   │       ├── python.rs      # Python chunker + import extraction
│   │   │       ├── go.rs          # Go chunker + func/type/import extraction
│   │   │       └── java.rs        # Java chunker + class/interface/enum + imports
│   │   └── Cargo.toml
│   └── sqmd-cli/           # binary (named `sqmd`)
│       └── Cargo.toml
├── docs/
│   ├── ROADMAP.md
│   ├── ARCHITECTURE.md
│   └── schema.sql
└── Cargo.toml
```

## Current Status

| Phase | Status | What it adds |
|-------|--------|-------------|
| 0 — Spike | Done | Validated sqlite-vec + ort |
| 1 — Foundations | Done | Schema, CLI, file ingestion, FTS5 search |
| 2 — Tree-sitter | Done | TS/Rust/Python/Go/Java chunkers, relationships, importance |
| 3 — Incremental | Done | Rayon pipeline, file watcher, hash-based change detection |
| 4 — Embeddings | Done | Vector search, hybrid scoring, model auto-download |
| 5 — Relationship Graph | Done | Cross-file call graph + recursive CTE depth traversal |
| 6 — Agent API | Done | Daemon mode, context assembly, token budgets |

**48 tests (default), 56 tests (embed), 0 clippy warnings, CI passing.**

## How It's Used

sqmd is a standalone SQLite + Markdown file system for code. Plug it into any AI agent, editor, or tool that needs fast, structured access to code. It replaces ad-hoc file reads and grep with semantically chunked, queryable code intelligence.

### Daemon protocol

`sqmd serve` listens on `~/.sqmd/daemon.sock` with a JSON request/response protocol:

```json
{"method": "search", "params": {"query": "authentication", "top_k": 10}}
{"method": "context", "params": {"query": "how does auth work", "max_tokens": 8000, "include_deps": true}}
{"method": "stats", "params": {}}
{"method": "index_file", "params": {"path": "src/main.rs"}}
{"method": "embed", "params": {}}
```

> Note: `embed` method and hybrid search require `--features embed`. Without it, `search` uses FTS5 keyword matching.

## What It Replaces

sqmd replaces ad-hoc file reads, blind grep searches, and manual context stitching. Instead of burning tokens on entire files, agents query sqmd for the exact chunks they need — with dependencies, call graphs, and structure — assembled into token-budgeted Markdown.

## License

MIT
