# sqmd

**Local-first code and knowledge intelligence for AI agents. A single Rust binary, zero network.**

sqmd indexes any codebase into a SQLite database of semantically chunked source code with tree-sitter parsing, FTS5 keyword search, vector embeddings, and an import/call relationship graph. It also accepts external knowledge — facts, decisions, preferences, transcripts — alongside code chunks. Zero external services. Works offline.

| Build | Stripped size | What's included |
|-------|--------------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, call graph, daemon, knowledge ingest |
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
| **sqmd (default)** | **904 KB** | **4.2x** | Chunks + FTS5 + relationships + contains + call graph + knowledge |
| **sqmd (with embeddings)** | **~2.5 MB** | **11.7x** | All above + 768-dim vector search + hybrid scoring |

sqmd at 4.2x raw source stores everything needed: chunked code with names, signatures, types, importance scores, FTS5 full-text index, import/call/contains relationships, entity graph, prospective hints, and a unified knowledge store. No external services.

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

## Comparison: Markdown vs SQLite vs sqmd

Most code intelligence approaches fall into three categories. Here's how they compare across the dimensions that matter for agent consumption.

### At a Glance

| | Raw Markdown | Raw SQLite | **sqmd** |
|---|---|---|---|
| **Format** | Flat files on disk | Single `.db` file | Single `.db` file |
| **Structure** | None (or manual headers) | Manual schema | Tree-sitter AST-derived |
| **Chunking** | Manual or none | Manual or none | Automatic per declaration |
| **Search** | `grep` / filesystem | Manual SQL queries | FTS5 + vector hybrid |
| **Relationships** | None | Manual foreign keys | Import/call/contains graph |
| **Incremental updates** | Rewrite entire file | Manual diffing | Content-hash decision pipeline |
| **Token efficiency** | Dump everything | Query but no relevance ranking | Relevance-ranked + budget-aware |
| **Entity graph** | None | Manual tables | Auto-built from code structure |
| **Semantic hints** | None | None | Prospective FTS5 bridge |
| **Knowledge ingest** | Manual | Manual | Built-in API (facts, decisions, preferences) |
| **Offline** | Yes | Yes | Yes |
| **Network required** | No | No | No |
| **External services** | None | None | None |
| **Binary size** | N/A | N/A | ~10MB (27MB with embeddings) |

### The Problem with Raw Markdown

Agents today often get code context as markdown files -- concatenated source, hand-formatted headers, maybe some structure. This works for small codebases but breaks down fast:

- **No query capability.** You grep or you read the whole file. No "find the auth middleware and everything it depends on."
- **No relevance ranking.** Every chunk is equal. The agent burns tokens on boilerplate, imports, and tests when it needs the core logic.
- **No relationships.** You can't ask "what calls this function?" or "what does this module import?" without the agent reading every file.
- **Stale on change.** Regenerate the whole file on every code change, or accept drift. No incremental updates.
- **Token waste.** A 50K-token codebase becomes a 50K-token markdown file. sqmd returns 10 relevant chunks in ~4K tokens (96% reduction).
- **No semantic gap bridge.** An agent asking "how does authentication work" won't find a function called `verify_jwt` through keyword search on raw markdown.

### The Problem with Raw SQLite

You could build a SQLite database yourself. But you'd need to:

1. Write a code parser or use tree-sitter bindings in your language
2. Design a schema that handles chunks, relationships, embeddings, entities, knowledge
3. Build an FTS5 index with content-sync triggers
4. Implement a vector search extension (sqlite-vec)
5. Write an embedding pipeline with ONNX Runtime
6. Build a change detection system (content hashes, decision pipeline)
7. Implement a relationship extractor (imports, calls, contains)
8. Build a token-budgeting context assembler
9. Add entity graph with aspects and attributes
10. Build knowledge ingest with source type discrimination

That's everything sqmd already does, in a single binary.

### What sqmd Adds Over Either

| Capability | Markdown | SQLite | sqmd |
|---|---|---|---|
| Parse 6 languages to semantic chunks | No | DIY | Built-in |
| Content-hash incremental re-index | No | DIY | Built-in |
| Decision pipeline (skip/update/tombstone) | No | DIY | Built-in |
| FTS5 full-text search | No | DIY | Built-in |
| Vector embeddings (768d) | No | DIY | `--features embed` |
| Hybrid alpha-blended scoring | No | DIY | Built-in |
| Import/call/contains graph | No | DIY | Built-in |
| Recursive CTE graph traversal | No | DIY | Built-in |
| Entity/aspect/attribute model | No | DIY | Built-in |
| Prospective hint indexing | No | No | Built-in |
| Graph-boosted search ranking | No | DIY | Built-in |
| Soft-delete with retention decay | No | DIY | Built-in |
| Knowledge ingest (facts, decisions, etc.) | No | DIY | Built-in |
| Multi-agent scoping (agent_id) | No | DIY | Built-in |
| Token-budgeted context assembly | No | DIY | Built-in |
| On-demand Markdown rendering | N/A | N/A | Built-in |
| Unix socket daemon + JSON protocol | No | DIY | Built-in |
| File watcher with debounce | No | DIY | Built-in |

### Storage Overhead Comparison

Measured on a 243-file TypeScript codebase (~45K lines, 986 KB source):

| Method | Disk size | Index included | Queryable |
|---|---|---|---|
| Raw source files | 986 KB | No | grep only |
| Concatenated markdown | 1,024 KB | No | grep only |
| Custom SQLite (chunks + FTS5 only) | ~1.8 MB | Partial | Keyword only |
| Custom SQLite (chunks + FTS5 + relationships) | ~2.4 MB | Partial | + graph traversal |
| **sqmd (default)** | **~2.1 MB** | **Full** | **FTS5 + hints + graph + knowledge** |
| **sqmd (embed)** | **~4.8 MB** | **Full** | **+ vector hybrid** |
| Custom SQLite (all features, hand-built) | ~5 MB+ | Full | Depends on implementation |

sqmd's overhead is the cost of making code and knowledge actually queryable. At 2.1x raw source (default) or 4.9x (with embeddings), you get structured search, relationship traversal, entity graph, knowledge types, and token-efficient context assembly. Building equivalent functionality yourself costs more in storage and orders of magnitude more in development time.

## How sqmd Works

### Index

```
source files (TS, Rust, Python, Go, Java, or fallback line-based)
    |
    v tree-sitter (per-language grammar -> AST)
    |
    v AST walker extracts declarations
    |
    +--> function, method, class, struct, trait, enum,
    |    interface, type, import, module chunks
    |
    +--> each chunk stores:
    |    content_raw (original source code, NOT markdown)
    |    name, signature, chunk_type, source_type
    |    file_path, language, line_start, line_end
    |    content_hash (SHA-256), importance (0.0-1.0)
    |    agent_id, tags, decay_rate, created_by
    |
    v decision pipeline (content_hash comparison)
    +--> SKIP:     hash unchanged -> 0 mutations
    +--> UPDATE:   hash changed   -> re-chunk, update DB
    +--> TOMBSTONE: file deleted  -> soft-delete (is_deleted=1)
```

### Knowledge Ingest (external API)

```
external system (Signet, agent, user)
    |
    v KnowledgeChunk { content, chunk_type, source_type, ... }
    |
    +--> fact, summary, decision, preference,
    |    entity_description, document_section
    |
    v content-hash dedup -> skip if exists
    |
    v INSERT into chunks table
    v generate prospective hints
    v create optional relationships
```

### Store

```
chunk (code or knowledge)
    |
    +--> chunks table       (raw code + metadata + knowledge fields)
    +--> chunks_fts          (FTS5 auto-sync via triggers)
    +--> relationships       (imports, contains, calls, contradicts, ...)
    +--> entities            (files, structs, functions, etc.)
    +--> entity_aspects      (exports, implementation, constraints)
    +--> entity_attributes   (per-chunk annotations linked to entities)
    +--> entity_dependencies (entity-level dependency edges)
    +--> hints + hints_fts   (prospective natural-language queries)
    +--> structural importance (graph density: in-degree, contains count)
    +--> chunks_vec          (768-dim embeddings via sqlite-vec, optional)
```

### Query

```
agent query: "how does authentication work"
    |
    +--> FTS5 MATCH          (keyword search on chunks_fts)
    +--> hints_fts MATCH     (prospective hint bridging)
    +--> graph boost         (entity graph density ranking)
    +--> sqlite-vec KNN      (cosine similarity, optional)
    |
    v normalize + alpha-blend (default 70% vector / 30% keyword)
    |
    v filter by source_type, agent_id, tags
    |
    v top-K results
    |
    v optional: dependency expansion (recursive CTE, depth N)
    |
    v token budget -> trim to limit
    |
    v Chunk::render_md() -> on-demand Markdown for each chunk
    |
    v assembled context -> agent
```

### Key Design Decisions

1. **Raw code, not Markdown.** `content_raw` stores the original source. Markdown is derived on demand via `Chunk::render_md()`. Source of truth stays in the code.

2. **Content-hash decision pipeline.** Every chunk gets a SHA-256 hash. On re-index, only changed chunks are updated. Zero-mutation runs produce zero writes. Code chunks and knowledge chunks both use SHA-256 — no algorithm mismatch.

3. **Soft-delete with retention.** Deleted files are tombstoned, not hard-deleted. `sqmd prune --days N` purges old tombstones. Prevents data loss during re-indexes.

4. **Prospective hints.** Natural-language queries like "how does authenticate work" are generated per chunk and indexed in FTS5. Bridges the gap between code names and agent language.

5. **Graph density as relevance signal.** Highly-depended-upon code (high in-degree, many contains edges) gets boosted in search. A utility function called by 50 modules ranks higher than a private helper.

6. **Unified code + knowledge store.** Code chunks and knowledge chunks (facts, decisions, preferences) live in the same table with source type discrimination. One query searches both.

7. **Single binary, zero network.** Everything runs locally. SQLite does the heavy lifting (FTS5, WAL mode, recursive CTEs). No server, no API keys, no external services.

8. **Multi-threaded daemon.** Each client connection is handled in its own thread with its own SQLite connection (WAL mode supports concurrent readers + single writer). Pipeline, dreaming, and embedding health checks no longer block each other.

9. **Asymmetric retrieval.** Nomic query/document prefixes (`search_query:`, `search_document:`) improve recall for Q&A workloads by encoding queries and documents differently.

10. **Temporal decay.** Knowledge chunks can decay over time via an exponential decay function on `decay_rate` × days since `last_accessed`, preventing stale knowledge from dominating search results.

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
sqmd search "decision" --source-type memory # search only knowledge chunks
sqmd search "auth" --agent-id agent-1        # scope to an agent
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

### Knowledge Ingest

```bash
sqmd ingest --content "User prefers dark mode" --type preference --tags "ui,display"
sqmd ingest --content "Auth uses JWT tokens" --type fact --source-type memory --importance 0.8
sqmd forget 42                        # soft-delete a knowledge chunk
sqmd modify 42 --importance 0.9 --tags "security,auth"
```

### Maintenance

```bash
sqmd entities                         # list all entities (files, structs, functions)
sqmd entities --type struct            # filter by entity type
sqmd entity-deps "AuthModule"          # entity dependency graph
sqmd entity-deps "AuthModule" --depth 2
sqmd prune --days 30                   # purge soft-deleted chunks older than 30 days
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

### Code Chunk Types

| Chunk Type | Examples | Importance |
|-----------|----------|------------|
| Function | `fn main()`, `def process()`, `const authenticate = ()`, `func Handle()` | 0.9 |
| Method | `impl Block for Transaction { fn execute() }`, `func (s *Server) Start()` | 0.9 |
| Class/Struct/Enum | `struct User`, `class Database`, `enum Result`, `type Config struct` | 0.85 |
| Interface/Trait/Type | `trait Read`, `interface Handler`, `type Config` | 0.8 |
| Impl block | `impl User { ... }` | 0.8 |
| Import | `import { X }`, `use crate::module`, `from module import X` | 0.3 |
| Module/Section | Top-level unclaimed code, file-level constants | 0.2-0.5 |

### Knowledge Chunk Types

| Chunk Type | Source | Importance |
|-----------|--------|------------|
| Decision | memory | 0.8 |
| Preference | memory | 0.75 |
| Fact | memory / transcript | 0.7 |
| Entity description | entity | 0.65 |
| Summary | document | 0.6 |
| Document section | document | 0.5 |

### Source Types

| Source Type | Origin | Description |
|------------|--------|-------------|
| `code` | tree-sitter indexer | Parsed source code chunks |
| `memory` | external ingest | Facts, decisions, preferences |
| `transcript` | external ingest | Conversation summaries |
| `document` | external ingest | Document sections, summaries |
| `entity` | external ingest | Entity descriptions |

### Knowledge Columns

Each chunk stores: raw content, source type, optional agent ID (multi-agent scoping), JSON tags, decay rate (for retention scoring), last accessed timestamp, and created-by pipeline stage.

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

sqmd extracts relationships automatically from code and supports manual creation for knowledge:

### Code Relationships

- **`imports`** -- cross-file: `import { X } from './path'`, `use crate::module::Item`, `from module import X`, `"fmt"`, `import java.net.http`
- **`contains`** -- intra-file: class->method, impl->method, module->function, trait->method, struct->fields
- **`calls`** -- cross-file: regex-based call graph extraction resolved against imported symbols

### Knowledge Relationships

- **`contradicts`** -- knowledge chunks that conflict
- **`supersedes`** -- newer chunk replaces older
- **`elaborates`** -- chunk expands on another
- **`derived_from`** -- chunk was derived from source material
- **`mentioned_in`** -- chunk is referenced by another
- **`relates_to`** -- generic association

Query with `sqmd deps <file> --depth N` to traverse the graph bidirectionally.

## Hybrid Search

sqmd blends two search modes with configurable alpha weighting:

- **FTS5** (keyword): fast exact/near-match on code text, function names, signatures
- **Vector KNN** (semantic): cosine similarity via sqlite-vec on 768-dim embeddings (nomic-embed-text-v1.5)
- **Hint bridging**: prospective hints bridge the semantic gap between agent language and code names
- **Graph boost**: entity graph density (in-degree, contains count) boosts structural importance
- **Decay scoring**: knowledge chunks with a `decay_rate` lose relevance over time via exponential decay based on days since `last_accessed`

Default: `alpha=0.7` (70% vector, 30% keyword). Single-source penalty (0.8) downranks chunks that appear in only one ranking.

### Asymmetric Retrieval

Embeddings use nomic-embed-text-v1.5's `search_query:` / `search_document:` prefixes for asymmetric retrieval. Documents are embedded with `search_document:` and queries with `search_query:`, which significantly improves recall quality for question-answering workloads.

### Real Batch Embedding

`embed_batch` performs actual batched ONNX inference — inputs are stacked into `[N, seq_len]` tensors and run in a single forward pass, rather than looping through `embed_one` N times. This provides meaningful throughput gains for bulk embedding operations.

### Filter Parity

Both FTS and vector search respect the same filters: `file`, `type`, `source_types`, and `agent_id`. Previously, vector search only accepted file and type filters, meaning `source_type` and `agent_id` scoping could leak non-matching code chunks into results.

Embeddings use ONNX Runtime (ort) with a quantized model cached at `~/.sqmd/models/`. Auto-downloads on first `sqmd embed` if missing.

## Architecture

```
sqmd/
+-- crates/
|   +-- sqmd-core/          # library
|   |   +-- src/
|   |   |   +-- schema.rs        # SQLite DDL + migrations (v4)
|   |   |   +-- chunk.rs         # Chunk struct + ChunkType + SourceType + render_md()
|   |   |   +-- chunker.rs       # LanguageChunker trait + FileChunker fallback
|   |   |   +-- index.rs         # Transactional indexer + decision pipeline + KnowledgeIngestor
|   |   |   +-- embed.rs         # ONNX embedding (ort) + BPE tokenizer + auto-download
|   |   |   +-- search.rs        # FTS5 + vector hybrid search engine
|   |   |   +-- relationships.rs  # Import resolution + call graph + CTE depth traversal
|   |   |   +-- entities.rs      # Entity/aspect/attribute model + hints + graph boost
|   |   |   +-- context.rs       # Context assembly + token budgeting
|   |   |   +-- vfs.rs           # Virtual file system: list, get, diff, tree rendering
|   |   |   +-- daemon.rs        # Unix socket daemon + JSON protocol + knowledge handlers
|   |   |   +-- watcher.rs       # notify file watcher + 200ms debounce
|   |   |   +-- files.rs         # Language detection + file walking + hashing
|   |   |   +-- languages/
|   |   |       +-- typescript.rs  # TS/TSX chunker + import extraction + JSX
|   |   |       +-- rust.rs        # Rust chunker + use/impl extraction
|   |   |       +-- python.rs      # Python chunker + import extraction
|   |   |       +-- go.rs          # Go chunker + func/type/import extraction
|   |   |       +-- java.rs        # Java chunker + class/interface/enum + imports
|   |   +-- tests/
|   |   |   +-- knowledge_integration.rs  # Knowledge ingest + search E2E test
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

v1.1.0. All 7 phases complete + production hardening. 61 tests (default), 70 tests (embed), 0 clippy warnings, CI passing.

| Phase | What it adds |
|-------|-------------|
| 0 -- Spike | Validated sqlite-vec + ort |
| 1 -- Foundations | Schema, CLI, file ingestion, FTS5 search |
| 2 -- Tree-sitter | TS/Rust/Python/Go/Java chunkers, relationships, importance |
| 3 -- Incremental | Rayon pipeline, file watcher, hash-based change detection |
| 4 -- Embeddings | Vector search, hybrid scoring, model auto-download |
| 5 -- Relationship Graph | Cross-file call graph + recursive CTE depth traversal |
| 6 -- Agent API | Daemon mode, context assembly, token budgets |
| 7 -- Knowledge Store | Schema v4, knowledge types, ingest/forget/modify, unified search |

### v1.1.0 Changes (production hardening)

| Change | What it fixes |
|--------|--------------|
| **Nomic query/document prefixes** | `embed_query()` / `embed_document()` / `embed_batch_documents()` / `embed_batch_queries()` use `search_query:` / `search_document:` prefixes for asymmetric retrieval |
| **Real batch ONNX embedding** | `embed_batch` stacks inputs into `[N, seq_len]` tensors for single forward pass instead of looping `embed_one` N times |
| **vec_search filter parity** | `source_type_filter` and `agent_id_filter` now applied to vector search, not just FTS |
| **Unified SHA-256 hashing** | `Chunk::knowledge()` now uses SHA-256 (64-hex) matching code indexing path, fixing dedup when same content enters via both paths |
| **Temporal decay scoring** | Search scores multiplied by exponential decay factor based on `decay_rate` × days since `last_accessed` |
| **Multi-threaded daemon** | Each connection handled in its own thread with its own SQLite connection (WAL mode) |

## Daemon Protocol

`sqmd serve` listens on `~/.sqmd/daemon.sock` with a JSON request/response protocol:

```json
{"method": "search", "params": {"query": "authentication", "top_k": 10}}
{"method": "search", "params": {"query": "decision", "source_types": ["memory"]}}
{"method": "context", "params": {"query": "how does auth work", "max_tokens": 8000, "include_deps": true}}
{"method": "stats", "params": {}}
{"method": "index_file", "params": {"path": "src/main.rs"}}
{"method": "embed", "params": {}}
{"method": "embed_text", "params": {"text": "hello world"}}
{"method": "embed_batch", "params": {"texts": ["hello", "world"]}}
{"method": "ingest", "params": {"content": "User prefers dark mode", "chunk_type": "preference", "tags": ["ui"]}}
{"method": "ingest_batch", "params": {"chunks": [...]}}
{"method": "forget", "params": {"id": 42}}
{"method": "modify", "params": {"id": 42, "importance": 0.9, "tags": ["security"]}}
{"method": "ls", "params": {"file": "src/auth.ts", "depth": 1}}
{"method": "cat", "params": {"id": 42}}
```

> `embed`, `embed_text`, `embed_batch` methods and hybrid search require `--features embed`. Without it, `search` uses FTS5 keyword matching.

## License

MIT
