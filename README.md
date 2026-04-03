# sqmd

**SQLite + Markdown — the most optimal file reading system for AI agents.**

sqmd turns any codebase into a single SQLite database of semantically chunked Markdown, queryable via FTS5 keyword search, vector similarity, and import/call graph traversal. Zero network. Zero external services. Works offline.

A single Rust binary (~5MB). Index once, query in <20ms.

## What It Is

sqmd solves the core problem every AI agent faces when reading code: **context retrieval is slow, wasteful, and structurally blind.**

Currently, agents read files one at a time, grep for keywords without understanding semantics, and burn tokens on irrelevant code. There's no way to ask "find the auth middleware and everything it depends on" and get a precise, token-efficient answer.

sqmd fixes this by:

1. **Parsing** any source file into semantically meaningful chunks (functions, classes, types, modules) using tree-sitter
2. **Storing** each chunk as Markdown in SQLite with metadata (file path, line ranges, language, signature, exports)
3. **Embedding** each chunk with a local ONNX model for vector similarity search
4. **Mapping** import/call relationships between chunks in a dependency graph
5. **Querying** with hybrid search (70% vector + 30% keyword) and graph traversal in <20ms

The result: an agent can ask a natural language question about a codebase and get exactly the relevant chunks — with their dependencies — assembled into a context-ready Markdown document within a token budget.

### What It Replaces

sqmd is designed to replace the LLM-heavy extraction pipelines used by systems like Signet, where per-session costs include 3-5 LLM calls for transcript extraction, fact extraction, decision-making, and synthesis. sqmd eliminates all of these by using deterministic parsing, embedding, and scoring — cutting LLM costs by 60-80% with better recall quality.

## Quick Start (Planned)

```bash
# Install
cargo install sqmd

# Index your project
sqmd init          # creates .sqmd/index.db
sqmd index         # tree-sitter parse + embed everything

# Watch for changes
sqmd watch         # incremental re-index on file save

# Query
sqmd search "how does authentication work"
sqmd get src/auth.ts:42           # chunk at file:line
sqmd deps src/auth.ts:42 --depth 2  # auth + its dependency graph
```

## Architecture

```
Source Files
    │
    ▼ tree-sitter (per-language AST parsing)
┌─────────┐
│ Chunks  │ ──► Markdown content + metadata
└────┬────┘
     │
     ├──► SQLite (chunks table, FTS5 index)
     ├──► sqlite-vec (vector embeddings, KNN search)
     └──► relationships table (import/call graph)
              │
              ▼
         Hybrid Search Engine
              │
              ▼
         Context Assembly (token-budgeted Markdown)
              │
              ▼
         Agent context injection
```

## Roadmap

See [docs/ROADMAP.md](docs/ROADMAP.md) for the full development plan.

## License

MIT
