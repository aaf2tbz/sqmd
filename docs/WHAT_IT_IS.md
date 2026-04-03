# sqmd — What It Is

## The Problem

Every AI agent that reads code does it poorly. Here's why:

1. **File-level granularity.** Agents read entire files when they need one function. A 500-line file with 15 functions wastes 93% of the tokens on irrelevant code.

2. **Keyword-only search.** Grep and ripgrep are fast but blind. Searching for "auth" won't find `authenticate_credentials` unless you think to search for it. It can't find code that *conceptually* relates to authentication without sharing a keyword.

3. **No structural awareness.** When an agent finds the auth middleware, it has no idea what the middleware imports, what calls it, or what it depends on. It has to make separate tool calls to trace each dependency manually.

4. **LLM-burning extraction pipelines.** Systems like Signet use 3-5 LLM calls per session to extract, classify, decide on, and synthesize memories from transcripts. This is slow, expensive, and often wrong.

5. **Context assembly is ad hoc.** Agents manually stitch together file reads, grep results, and glob matches into something coherent. There's no system that assembles a token-budgeted, structured context from a codebase.

## The Solution

sqmd is a local-first code index that makes codebases fully queryable by agents in under 20ms.

### How It Works

**Ingestion** — tree-sitter parses every source file into semantic chunks. Not lines, not files — actual language constructs: functions, classes, methods, interfaces, types, modules. Each chunk is rendered as Markdown with its signature, file path, line range, and metadata.

**Storage** — Every chunk goes into a single SQLite file alongside:
- An FTS5 full-text index for keyword search
- Vector embeddings (via local ONNX model) for semantic search
- An import/call relationship graph for dependency traversal

**Query** — A hybrid search engine combines vector similarity (70%) and keyword relevance (30%), then optionally traverses the dependency graph to include related chunks. Results are assembled into a token-budgeted Markdown document ready for direct context injection.

**Incremental updates** — A file watcher detects changes and re-indexes only what changed, in under 200ms.

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

### Design Philosophy

1. **Local first.** No network, no API keys, no cloud services. A single SQLite file per project.

2. **Deterministic over probabilistic.** Use tree-sitter parsing instead of LLM extraction. Use cosine similarity instead of LLM-based deduplication. LLMs are a last resort, not a first step.

3. **Query-time extraction.** Don't pre-classify everything into rigid categories at index time. Store raw, well-structured chunks and let the query decide what's relevant.

4. **Markdown everywhere.** Every chunk is Markdown. Every search result is Markdown. Every assembled context is Markdown. Agents already speak Markdown fluently — meet them where they are.

5. **Single binary.** Rust + static linking. No runtime dependencies. Drop it on any machine and it works.

## Intended Use Cases

**Primary:** Agent context retrieval — the system that answers "what code do I need to read right now?" for AI coding assistants.

**Secondary:** Codebase navigation for humans — a CLI that understands code structure better than any grep.

**Integration target:** Memory and extraction pipelines (Signet, etc.) that currently rely on LLM calls for code understanding.
