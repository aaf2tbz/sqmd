# sqmd

**Local-first code and knowledge intelligence for AI agents. A single Rust binary, zero network.**

sqmd indexes any codebase into a SQLite database of semantically chunked source code with tree-sitter parsing, FTS5 keyword search, vector embeddings, and an import/call relationship graph. It also accepts external knowledge — facts, decisions, preferences, transcripts — alongside code chunks. Zero external services. Works offline.

| Build | Stripped size | What's included |
|-------|--------------|-----------------|
| `cargo build --release` | ~10MB | Chunking, FTS5, relationships, call graph, daemon, knowledge ingest |
| `cargo build --release --features embed` | ~27MB | + ONNX Runtime, vector search, hybrid scoring |

## Benchmarks

All measurements on the sqmd codebase itself (27 files, 20 `.rs` source files, ~219 KB raw source).

### Storage

| Method | Size | vs raw source | What you get |
|--------|------|--------------|--------------|
| Raw source files | 214 KB | 1.0x | Files on disk, nothing indexed |
| LSP symbol JSON | 53 KB | 0.2x | Names + positions only, no content |
| Tree-sitter JSON AST | 642-1071 KB | 3-5x | Full AST, no search, no relationships |
| Flat markdown dump | 216 KB | 1.0x | All code in one file, no structure, no search |
| JSONL + vector embeddings (768d) | 1,660 KB | 7.8x | Vectors + content, needs Chroma/Pinecone |
| **sqmd (default)** | **904 KB** | **4.2x** | Chunks + FTS5 + relationships + contains + call graph + knowledge |
| **sqmd (with embeddings)** | **~2.5 MB** | **11.7x** | All above + 768-dim vector search + hybrid scoring |

On a 243-file TypeScript codebase (~45K lines, 986 KB source): sqmd default is ~2.1 MB, sqmd embed is ~4.8 MB. A 10-chunk context window (~4K tokens) typically answers most "how does X work" questions — 96% fewer tokens than reading the whole codebase.

### Query Speed

| Approach | Latency | Notes |
|----------|---------|-------|
| `grep -R` | ~10ms | Linear scan, no structure awareness |
| `ripgrep` | ~19ms | Faster grep, still no structure |
| `sqmd fts_search` | ~20ms | Structured results with chunk types, names, line ranges |
| `sqmd hybrid_search` | ~40ms | FTS5 + vector KNN combined |

## Comparison: Markdown vs SQLite vs sqmd

| | Raw Markdown | Raw SQLite | **sqmd** |
|---|---|---|---|
| **Format** | Flat files on disk | Single `.db` file | Single `.db` file |
| **Structure** | None (or manual headers) | Manual schema | Tree-sitter AST-derived |
| **Chunking** | Manual or none | Manual or none | Automatic per declaration |
| **Search** | `grep` / filesystem | Manual SQL queries | FTS5 + vector hybrid + multi-layer |
| **Scoring** | None | Manual | Three-factor: relevance × recency × importance |
| **Relationships** | None | Manual foreign keys | Import/call/contains graph + bi-temporal |
| **Incremental updates** | Rewrite entire file | Manual diffing | Content-hash decision pipeline + episodes |
| **Token efficiency** | Dump everything | Query but no relevance ranking | Relevance-ranked + budget-aware |
| **Entity graph** | None | Manual tables | Auto-built from code structure |
| **Community summaries** | None | None | Directory-based GraphRAG communities |
| **Semantic hints** | None | None | Prospective FTS5 bridge |
| **Knowledge ingest** | Manual | Manual | Built-in API (facts, decisions, preferences) |
| **Change provenance** | Git only | Manual | Automatic episode recording |
| **Offline / zero network** | Yes | Yes | Yes |
| **Binary size** | N/A | N/A | ~10MB (27MB with embeddings) |

### What sqmd Adds (over building it yourself)

You'd need to: write a code parser, design a schema, build FTS5 triggers, implement sqlite-vec, write an ONNX embedding pipeline, build a change detection system, implement a relationship extractor, build a token-budgeting context assembler, add an entity graph, and build knowledge ingest. That's everything sqmd already does, in a single binary.

| Capability | Built-in? |
|---|---|
| Parse 6 languages to semantic chunks | Yes |
| Content-hash incremental re-index (skip/update/tombstone) | Yes |
| FTS5 full-text search with Porter stemming | Yes |
| Vector embeddings (768d) + hybrid alpha-blended scoring | `--features embed` |
| Import/call/contains graph + recursive CTE traversal | Yes |
| Entity/aspect/attribute model | Yes |
| Prospective hint indexing (semantic gap bridge) | Yes |
| Graph-boosted + importance-aware search ranking | Yes |
| Three-factor retrieval scoring (relevance × recency × importance) | Yes |
| Multi-layer retrieval with short-circuit (FTS → graph → community) | Yes |
| Diversity dampening (same-file penalty) | Yes |
| Soft-delete with retention decay | Yes |
| Knowledge ingest (facts, decisions, preferences) | Yes |
| Multi-agent scoping (agent_id) | Yes |
| Token-budgeted context assembly with dependency expansion | Yes |
| Hierarchical community summaries (directory-based GraphRAG) | Yes |
| Bi-temporal fact tracking (valid_from/valid_to on entity deps) | Yes |
| Episodic ingestion pipeline (change provenance) | Yes |
| Unix socket daemon + JSON protocol | Yes |
| File watcher with debounce | Yes |

## How sqmd Works

### Index

```
source files (TS, Rust, Python, Go, Java, or fallback line-based)
    |
    v tree-sitter (per-language grammar -> AST)   [parse ONCE]
    |
    v AST walker extracts declarations
    |                                        [reuse Tree for imports]
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
    +--> chunks_fts          (FTS5 auto-sync via triggers, porter-stemmed)
    +--> relationships       (imports, contains, calls, contradicts, ...)
    +--> entities            (files, structs, functions, etc.)
    +--> entity_aspects      (exports, implementation, constraints)
    +--> entity_attributes   (per-chunk annotations linked to entities)
    +--> entity_dependencies (entity-level dependency edges, bi-temporal)
    +--> hints + hints_fts   (prospective natural-language queries)
    +--> communities          (directory-based code communities + summaries)
    +--> episodes             (change provenance: added/modified/deleted)
    +--> structural importance (graph density: in-degree, contains count)
    +--> chunks_vec          (768-dim embeddings via sqlite-vec, optional)
```

### Search Pipeline

```
agent query: "how does authentication work"
    |
    +--> query cache (LRU, 10s TTL)
    |       +--> HIT: return cached results (structured + markdown)
    |       +--> MISS: continue
    |
    v layered_search (multi-layer with short-circuit)
    |
    +--> Layer 1: FTS5 MATCH
    |       +--> hints_fts MATCH (prospective hint bridging)
    |       +--> graph boost (entity LIKE + recursive CTE expansion)
    |       +--> SHORT-CIRCUIT: if 3+ hits with score > 0.7 and top > 0.85 -> return
    |
    +--> Layer 2: Graph expansion
    |       +--> entity dependency graph -> related chunks
    |       +--> three-factor scoring: relevance × recency × importance
    |
    +--> Layer 3: Community summaries
    |       +--> directory-based community lookup
    |       +--> member chunk expansion
    |
    v normalize + rank by three-factor score
    v importance_boost()     (0.7x-1.0x based on chunk importance field)
    v dampen()               (same-file penalty: 3rd from same file -> 85%, 4th -> 72%)
    v top-K results
    |
    v render_search_markdown()
    |   SELECT content_raw, language, source_type, importance, tags
    |   FROM chunks WHERE id IN (...)  -- single batch query
    |   for each result: Chunk::render_md() -> rendered markdown string
    |
    v build JSON response array:
    |   [
    |     {
    |       "chunk_id": 42,
    |       "file_path": "src/auth.rs",
    |       "name": "authenticate",
    |       "score": 0.92,
    |       "layers_hit": ["fts", "graph"],
    |       "markdown": "<document>\n<source>src/auth.rs</source>\n..."
    |     },
    |     ...
    |   ]
    |
    v store results in query cache
    |
    v response over unix socket -> caller
```

Three-factor scoring replaces simple cosine/FTS ranking:
- **relevance** = FTS rank or vector cosine similarity
- **recency** = time decay with 90-day half-life based on `updated_at`
- **importance** = stored importance weight (0.0-1.0)

Multi-layer retrieval short-circuits when Layer 1 (FTS) produces 3+ high-confidence hits (score > 0.7, top score > 0.85), skipping graph and community layers entirely.

### Embedding Details

Embeddings use nomic-embed-text-v1.5 with asymmetric retrieval (`search_query:` / `search_document:` prefixes). Documents are embedded with `search_document:` and queries with `search_query:`, which significantly improves recall for Q&A workloads.

`embed_batch` performs actual batched ONNX inference — inputs are tokenized once (not twice), padded to the nearest multiple of 64 (not power-of-two), stacked into `[N, seq_len]` tensors, and run in a single forward pass.

Embeddings use ONNX Runtime (ort) with a quantized model cached at `~/.sqmd/models/`. Auto-downloads on first `sqmd embed` if missing.

### Key Design Decisions

1. **Raw code, not Markdown.** `content_raw` stores the original source. Markdown is derived on demand via `Chunk::render_md()`.
2. **Content-hash decision pipeline.** Every chunk gets a SHA-256 hash. Zero-mutation runs produce zero writes.
3. **Soft-delete with retention.** Deleted files are tombstoned, not hard-deleted. `sqmd prune --days N` purges old tombstones.
4. **Single-pass parsing.** Tree-sitter parses each file once; the resulting AST is reused for both chunking and import extraction.
5. **Memory-mapped I/O.** SQLite reads go through a 256MB mmap region. No checkpoint stalls (`wal_autocheckpoint=1000`).
6. **Shared daemon state.** ONNX embedder loaded once (`Arc<Mutex>`), shared across all connections. 256-entry LRU query cache deduplicates repeated agent searches.
7. **Read-consistent snapshots.** FTS search runs inside a `BEGIN`/`COMMIT` transaction so all phases see the same data state.
8. **Bi-temporal facts.** Entity dependencies track `valid_from`/`valid_to` timestamps. Superseded facts remain in history for point-in-time queries. Graph traversal filters to current facts by default.
9. **Episodic provenance.** Every index change (add/modify/delete) records an episode with file path, change type, chunk count, and optional commit hash/author.
10. **Directory-based communities.** File path prefixes form natural code communities. Community summaries provide graph-level context without external clustering dependencies.

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

## Architecture

```
sqmd/
+-- crates/
|   +-- sqmd-core/          # library
|   |   +-- src/
|   |   |   +-- schema.rs        # SQLite DDL + migrations (v8)
|   |   |   +-- chunk.rs         # Chunk struct + ChunkType + SourceType + render_md()
|   |   |   +-- chunker.rs       # LanguageChunker trait + FileChunker fallback
|   |   |   +-- index.rs         # Transactional indexer + decision pipeline + KnowledgeIngestor
|   |   |   +-- embed.rs         # ONNX embedding (ort) + BPE tokenizer + auto-download
|   |   |   +-- search.rs        # FTS5 + vector hybrid + multi-layer retrieval + three-factor scoring
|   |   |   +-- relationships.rs  # Import resolution + call graph + CTE depth traversal
|   |   |   +-- entities.rs      # Entity/aspect/attribute + hints + graph boost + bi-temporal tracking
|   |   |   +-- communities.rs   # Directory-based community detection + summaries
|   |   |   +-- episodes.rs      # Episodic ingestion pipeline + change provenance
|   |   |   +-- context.rs       # Context assembly + token budgeting
|   |   |   +-- dampening.rs     # Diversity dampening + importance boost + gravity scoring
|   |   |   +-- query_cache.rs    # LRU query cache for daemon search dedup
|   |   |   +-- vfs.rs           # Virtual file system: list, get, diff, tree rendering
|   |   |   +-- daemon.rs        # Unix socket daemon + JSON protocol + all handlers
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

## Daemon Protocol

`sqmd serve` listens on `~/.sqmd/daemon.sock` with a JSON request/response protocol:

```json
{"method": "search", "params": {"query": "authentication", "top_k": 10}}
{"method": "search", "params": {"query": "decision", "source_types": ["memory"]}}
{"method": "layered_search", "params": {"query": "how does auth work", "top_k": 10}}
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
{"method": "communities", "params": {"limit": 20}}
{"method": "community_summary", "params": {"id": 1}}
{"method": "project_summary", "params": {}}
{"method": "supersede_fact", "params": {"source_entity": 1, "target_entity": 2, "dep_type": "imports"}}
{"method": "facts_at", "params": {"entity_id": 1, "as_of": "2025-06-01"}}
{"method": "fact_history", "params": {"source_entity": 1, "target_entity": 2, "dep_type": "imports"}}
{"method": "episodes", "params": {"file_path": "src/auth.rs", "limit": 10}}
{"method": "episode_stats", "params": {}}
```

> `embed`, `embed_text`, `embed_batch` methods and hybrid search require `--features embed`. Without it, `search` uses FTS5 keyword matching.

## License

MIT
