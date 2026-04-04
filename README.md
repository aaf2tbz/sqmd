# sqmd

**Local-first code intelligence for AI agents. A single 5MB Rust binary, zero network.**

sqmd indexes any codebase into a SQLite database of semantically chunked source code with tree-sitter parsing, FTS5 keyword search, vector embeddings, and an import/call relationship graph. Query in milliseconds.

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
       └──► relationships table (imports + contains graph)
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
cargo build --release
# Binary at target/release/sqmd

# Index your project
cd /path/to/your/project
sqmd init          # creates .sqmd/index.db, updates .gitignore
sqmd index         # tree-sitter parse → chunk → store (~38ms for 220 chunks)

# Query
sqmd search "authenticate"           # FTS5 keyword search
sqmd search "error handling" --top 20
sqmd get src/auth.ts:42              # chunk at file:line (renders with language fence)
sqmd deps src/auth.ts                # import dependency graph
sqmd stats                           # files, chunks, relationships, DB size

# Reset and rebuild
sqmd reset && sqmd index
```

## What Gets Indexed

| Chunk Type | Examples | Importance |
|-----------|----------|------------|
| Function | `fn main()`, `def process()`, `const authenticate = ()` | 0.9 |
| Method | `impl Block for Transaction { fn execute() }` | 0.85 |
| Class/Struct/Enum | `struct User`, `class Database`, `enum Result` | 0.85 |
| Interface/Trait/Type | `trait Read`, `interface Handler`, `type Config` | 0.8 |
| Impl block | `impl User { ... }` | 0.7 |
| Module/Section | Top-level unclaimed code, file-level constants | 0.2-0.5 |

Each chunk stores: raw source code, file path, language, line range, name, signature, importance score. Unclaimed lines between declarations are grouped into section chunks (max ~50 lines).

## Languages Supported

| Language | Grammar | Status |
|----------|---------|--------|
| TypeScript | `tree-sitter-typescript` | Shipped |
| TSX | `tree-sitter-typescript` (tsx variant) | Shipped |
| Rust | `tree-sitter-rust` | Shipped |
| Python | `tree-sitter-python` | Shipped |

Other languages fall back to a line-based `FileChunker` that splits at section boundaries.

## Relationships

sqmd extracts two kinds of relationships automatically:

- **`imports`** — cross-file: `import { X } from './path'`, `use crate::module::Item`, `from module import X`
- **`contains`** — intra-file: class→method, impl→method, module→function, trait→method

Query with `sqmd deps <file>` to see both directions (what a file imports + what imports it).

## Architecture

```
sqmd/
├── crates/
│   ├── sqmd-core/          # library
│   │   ├── src/
│   │   │   ├── schema.rs       # SQLite schema + migrations + chunks_vec (non-fatal)
│   │   │   ├── chunk.rs        # Chunk struct + ChunkType + render_md()
│   │   │   ├── chunker.rs      # LanguageChunker trait + FileChunker fallback
│   │   │   ├── index.rs        # Transactional indexer with contains relationships
│   │   │   ├── embed.rs        # ONNX embedding (ort v2 RC)
│   │   │   ├── relationships.rs # Import path resolution + graph queries
│   │   │   ├── files.rs        # Language detection + file walking + hashing
│   │   │   └── languages/
│   │   │       ├── typescript.rs  # TS/TSX chunker + import extraction
│   │   │       ├── rust.ts        # Rust chunker + use/impl extraction
│   │   │       └── python.rs      # Python chunker + import extraction
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
| 2 — Tree-sitter | Done | TS/Rust/Python chunkers, relationships, importance |
| 3 — Incremental | Next | File watcher, hash-based change detection |
| 4 — Embeddings | MVP | Vector search, hybrid scoring |
| 5 — Call graph | Future | Cross-file call graph + traversal |
| 6 — Agent API | Future | Daemon mode, context assembly, token budgets |
| 7 — Signet | Future | Replace LLM-heavy extraction pipeline |

**28 tests, 0 clippy warnings, CI passing.** Binary: ~5MB release build.

## What It Replaces

sqmd is designed to replace LLM-heavy extraction pipelines (like Signet's) where per-session costs include 3-5 LLM calls for transcript extraction, fact extraction, decision-making, and synthesis. sqmd uses deterministic parsing, embedding, and scoring instead — cutting LLM costs by 60-80% with better recall.

## License

MIT
