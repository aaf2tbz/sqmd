# sqmd

**Local-first code intelligence for AI agents. A single Rust binary, zero network.**

sqmd indexes any codebase into a SQLite database of semantically chunked source code with tree-sitter parsing, FTS5 keyword search, vector embeddings, and an import/call relationship graph. Zero external services. Works offline.

| Build | Stripped size | What's included |
|-------|--------------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, call graph, daemon |
| `cargo build --release --features embed` | ~27MB | + ONNX Runtime, vector search, hybrid scoring |

## Benchmarks

All measurements taken on the sqmd codebase itself (27 files, 20 `.rs` source files, ~219 KB raw source).

### Storage Size

| Method | Size | vs raw source | What you get |
|--------|------|--------------|--------------|
| Raw source files | 214 KB | 1.0x | Files on disk, nothing indexed |
| LSP symbol JSON | 53 KB | 0.2x | Names + positions only, no content |
| Tree-sitter JSON AST | 642-1071 KB | 3-5x | Full AST, no search, no relationships |
| Flat markdown dump | 216 KB | 1.0x | All code in one file, no structure, no search |
| Full context markdown | 387 KB | 1.8x | All code + metadata, no search, no query |
| JSONL vector store (no embeddings) | 408 KB | 1.9x | Documents + metadata, needs external DB |
| JSONL + vector embeddings (768d) | 1,660 KB | 7.8x | Vectors + content, needs Chroma/Pinecone |
| **sqmd (default)** | **904 KB** | **4.2x** | Chunks + FTS5 + relationships + contains + call graph |
| **sqmd (with embeddings)** | **~2.5 MB** | **11.7x** | All above + 768-dim vector search + hybrid scoring |

sqmd at 4.2x raw source stores everything needed: chunked code with names, signatures, types, importance scores, FTS5 full-text index, import/call/contains relationships, and a query engine. No external services.

### Token Efficiency

| Approach | Tokens an LLM must process | Notes |
|----------|--------------------------|-------|
| Read every file | ~55,000 | Dumps entire codebase into context |
| Flat markdown dump | ~55,000 | Same content, marginal formatting overhead |
| Full context markdown | ~99,000 | Worse -- metadata bloat, no filtering |
| JSONL + embeddings | ~425,000 | Never meant for direct LLM consumption |
| **sqmd selective query (10 chunks)** | **~4,000** | **96% fewer tokens than full dump** |

The point: you don't need to read the whole index. sqmd queries return only the relevant chunks -- with dependencies -- inside a token budget. A 10-chunk context window (~4K tokens) typically answers most "how does X work" questions.

### Query Speed

| Approach | Latency | Notes |
|----------|---------|-------|
| `grep -R` | ~10ms | Linear scan, no structure awareness |
| `ripgrep` | ~19ms | Faster grep, still no structure |
| `sqmd fts_search` | ~20ms | Structured results with chunk types, names, line ranges |
| `sqmd hybrid_search` | ~40ms | FTS5 + vector KNN combined |

## The Problem

AI agents read code one file at a time, grep for keywords without understanding structure, and burn tokens on irrelevant context. There's no fast, offline way to ask "find the auth middleware and everything it depends on" and get a precise, token-efficient answer.

## How sqmd Works

```
source files
    |
    v tree-sitter (per-language AST)
+------------------------------+
|  Chunk: raw code + metadata  |
|  name, signature, type,      |
|  line range, importance,     |
|  file path, language         |
+------+-----------------------+
       |
       +--> SQLite chunks table (structured data + raw code)
       +--> FTS5 index (keyword search on code + names)
       +--> sqlite-vec (768-dim vector embeddings, KNN)
       +--> relationships table (imports + contains + calls)
                    |
                    v
           Hybrid Search Engine
           (FTS5 + vector + graph)
                    |
                    v
           Chunk::render_md() -> on-demand Markdown
                    |
                    v
           Agent context injection
```

**Key design choice:** sqmd stores raw source code in the database, not pre-rendered Markdown. Markdown is derived on demand via `Chunk::render_md()` at query time. This keeps the source of truth in the code itself and avoids stale renderings.

## Quick Start

```bash
cargo build --release                          # ~10MB: FTS5 + graph + chunking
cargo build --release --features embed          # ~27MB: + vector embeddings + hybrid search

cd /path/to/your/project
sqmd init          # creates .sqmd/index.db, updates .gitignore
sqmd index         # tree-sitter parse -> chunk -> store
```

## Commands

### Indexing

```bash
sqmd init                        # create index at .sqmd/index.db
sqmd index                       # full index (current directory)
sqmd index ./src                 # index a subdirectory
sqmd index --embed               # index + generate vector embeddings (requires --features embed)
sqmd embed                       # embed any unembedded chunks
sqmd reset                       # delete index and start fresh
```

### Searching

```bash
sqmd search "error handling"                 # keyword search (FTS5)
sqmd search "User" --type Struct             # filter by chunk type
sqmd search "config" --file src/config.rs    # filter by file
sqmd search "parsing" --alpha 0.8            # vector weight (requires embed feature)
sqmd search "authenticate" --keyword         # keyword-only (skip vector)
```

### Browsing

```bash
sqmd ls                              # list all top-level chunks
sqmd ls --depth 2                    # hierarchical tree (2 levels)
sqmd ls --file src/auth.ts           # chunks from one file
sqmd ls --type function              # filter by type
sqmd cat 42                          # get chunk by database ID
sqmd get src/auth.ts:42              # get chunk at file:line
sqmd diff "2025-01-01T00:00:00"      # chunks modified since timestamp
```

### Dependencies & Context

```bash
sqmd deps src/auth.ts                    # imports + dependents
sqmd deps src/auth.ts --depth 2          # traverse 2 levels deep
sqmd context --query "how does auth work" --max-tokens 8000 --deps
sqmd context --files src/auth.ts,src/middleware.ts --max-tokens 4000
```

### Daemon & Watch

```bash
sqmd serve    # background daemon on ~/.sqmd/daemon.sock
sqmd watch    # live re-index on file changes
```

### Global Flags

```bash
sqmd --json stats           # JSON output (works on stats, search, get, ls, cat, diff)
sqmd --json search "auth"   # machine-readable results
```

## What Gets Indexed

| Chunk Type | Examples | Importance |
|-----------|----------|------------|
| Function | `fn main()`, `def process()`, `const authenticate = ()`, `func Handle()` | 0.9 |
| Method | `impl Block for Transaction { fn execute() }`, `func (s *Server) Start()` | 0.9 |
| Class/Struct/Enum | `struct User`, `class Database`, `enum Result`, `type Config struct` | 0.85 |
| Interface/Trait/Type | `trait Read`, `interface Handler`, `type Config` | 0.8 |
| Impl block | `impl User { ... }` | 0.8 |
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

- **`imports`** -- cross-file: `import { X } from './path'`, `use crate::module::Item`, `from module import X`, `"fmt"`, `import java.net.http`
- **`contains`** -- intra-file: class->method, impl->method, module->function, trait->method, struct->fields
- **`calls`** -- cross-file: regex-based call graph extraction resolved against imported symbols

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
+-- crates/
|   +-- sqmd-core/          # library
|   |   +-- src/
|   |   |   +-- schema.rs        # SQLite schema + migrations + chunks_vec
|   |   |   +-- chunk.rs         # Chunk struct + ChunkType + render_md()
|   |   |   +-- chunker.rs       # LanguageChunker trait + FileChunker fallback
|   |   |   +-- index.rs         # Transactional indexer (chunks + contains + calls)
|   |   |   +-- embed.rs         # ONNX embedding (ort) + BPE tokenizer + auto-download
|   |   |   +-- search.rs        # FTS5 + vector hybrid search engine
|   |   |   +-- relationships.rs  # Import resolution + call graph + CTE depth traversal
|   |   |   +-- context.rs       # Context assembly + token budgeting
|   |   |   +-- vfs.rs           # Virtual file system: list, get, diff, tree rendering
|   |   |   +-- daemon.rs        # Unix socket daemon + JSON protocol
|   |   |   +-- watcher.rs       # notify file watcher + 200ms debounce
|   |   |   +-- files.rs         # Language detection + file walking + hashing
|   |   |   +-- languages/
|   |   |       +-- typescript.rs  # TS/TSX chunker + import extraction + JSX
|   |   |       +-- rust.rs        # Rust chunker + use/impl extraction
|   |   |       +-- python.rs      # Python chunker + import extraction
|   |   |       +-- go.rs          # Go chunker + func/type/import extraction
|   |   |       +-- java.rs        # Java chunker + class/interface/enum + imports
|   |   +-- Cargo.toml
|   +-- sqmd-cli/           # binary (named `sqmd`)
|       +-- Cargo.toml
+-- docs/
|   +-- ROADMAP.md
|   +-- ARCHITECTURE.md
|   +-- WHAT_IT_IS.md
|   +-- schema.sql
+-- .github/workflows/rust.yml
+-- Cargo.toml
```

## Current Status

All 6 phases complete. 60 tests (embed), 52 tests (default), 0 clippy warnings, CI passing.

| Phase | What it adds |
|-------|-------------|
| 0 -- Spike | Validated sqlite-vec + ort |
| 1 -- Foundations | Schema, CLI, file ingestion, FTS5 search |
| 2 -- Tree-sitter | TS/Rust/Python/Go/Java chunkers, relationships, importance |
| 3 -- Incremental | Rayon pipeline, file watcher, hash-based change detection |
| 4 -- Embeddings | Vector search, hybrid scoring, model auto-download |
| 5 -- Relationship Graph | Cross-file call graph + recursive CTE depth traversal |
| 6 -- Agent API | Daemon mode, context assembly, token budgets |

## Daemon Protocol

`sqmd serve` listens on `~/.sqmd/daemon.sock` with a JSON request/response protocol:

```json
{"method": "search", "params": {"query": "authentication", "top_k": 10}}
{"method": "context", "params": {"query": "how does auth work", "max_tokens": 8000, "include_deps": true}}
{"method": "stats", "params": {}}
{"method": "index_file", "params": {"path": "src/main.rs"}}
{"method": "embed", "params": {}}
{"method": "ls", "params": {"file": "src/auth.ts", "depth": 1}}
{"method": "cat", "params": {"id": 42}}
```

> `embed` method and hybrid search require `--features embed`. Without it, `search` uses FTS5 keyword matching.

## License

MIT
