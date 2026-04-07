# sqmd — What It Is

## The Problem

Every AI agent that reads code does it poorly. Here's why:

1. **File-level granularity.** Agents read entire files when they need one function. A 500-line file with 15 functions wastes 93% of the tokens on irrelevant code.

2. **Keyword-only search.** Grep and ripgrep are fast but blind. Searching for "auth" won't find `authenticate_credentials` unless you think to search for it.

3. **No structural awareness.** When an agent finds the auth middleware, it has no idea what imports it, what calls it, or what it depends on.

4. **Expensive extraction pipelines.** Many tools rely on cloud APIs or LLM calls for code understanding — slow, expensive, offline-incapable.

5. **Context assembly is ad hoc.** Agents manually stitch together file reads, grep results, and glob matches.

## The Solution

sqmd is a local-first code index that makes codebases fully queryable by agents in under 20ms.

### How It Works

**Ingestion** — tree-sitter parses every source file into semantic chunks: functions, classes, methods, interfaces, types, modules. Each chunk stores raw source code (`content_raw`), not pre-rendered Markdown. Also accepts external knowledge (facts, decisions, preferences) via the ingest API.

**Storage** — Every chunk goes into a single SQLite file alongside:
- An FTS5 full-text index with Porter stemming for keyword search
- Vector embeddings (via local ONNX model) for semantic search
- An import/call/contains relationship graph for dependency traversal
- An entity knowledge graph with hints for bridging the semantic gap

**Query** — A hybrid search engine combines vector similarity (70%) and keyword relevance (30%), boosted by entity graph density and importance scoring. Results are returned as JSON with both structured fields and a pre-rendered `"markdown"` field — agents grab `markdown` directly into their prompts, no extra formatting step.

**Incremental updates** — A file watcher detects changes and re-indexes only what changed, in under 200ms. Content-hash decision pipeline: SKIP (unchanged), UPDATE (modified), TOMBSTONE (deleted).

### Performance Characteristics

| Operation | Latency |
|-----------|---------|
| Full index (10k files, cold) | <60s |
| Incremental re-index (1 file) | <200ms |
| Hybrid search query | <20ms |
| Dependency traversal (depth 2) | <50ms |
| Context assembly (8k tokens) | <5ms |
| Idle daemon | ~15MB RAM, 0% CPU |

### What Makes It Different

| | grep | RAG (Pinecone/Weaviate) | LSP | **sqmd** |
|---|------|------------------------|-----|----------|
| Semantic search | No | Yes (cloud) | No | Yes (local) |
| Code structure aware | No | Partial | Yes | Yes |
| Dependency graph | No | No | Partial | Yes |
| Works offline | Yes | No | Yes | Yes |
| Zero config | Yes | No | Partial | Yes |
| Local only | Yes | No | Yes | Yes |
| Sub-20ms queries | Yes | No (~100ms) | No | Yes |
| Token-budgeted output | No | No | No | Yes |
| Markdown output | No | No | No | Yes (pre-rendered) |
| Knowledge ingest | No | No | No | Yes (facts, decisions, preferences) |

### Design Philosophy

1. **Local first.** No network, no API keys, no cloud services. A single SQLite file per project.

2. **Deterministic over probabilistic.** Use tree-sitter parsing instead of LLM extraction. Use cosine similarity instead of LLM-based deduplication.

3. **Raw code, derived Markdown.** `content_raw` stores original source. Markdown derived on demand via `Chunk::render_md()`. Every query response includes both structured fields and a `markdown` field.

4. **Single binary.** Rust + static linking. No runtime dependencies. Drop it on any machine and it works.

5. **Agent-native output.** JSON responses with a pre-rendered `markdown` field that agents can inject directly into their prompt context. No parsing, no formatting step.

## Intended Use Cases

**Primary:** Agent context retrieval — the system that answers "what code do I need to read right now?" for AI coding assistants.

**Secondary:** Codebase navigation for humans — a CLI that understands code structure better than any grep.
