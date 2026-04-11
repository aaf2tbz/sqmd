# Changelog

All notable changes to this project will be documented in this file.

## [3.0.0] - 2026-04-11

### Added

- Unified layered search pipeline — single 5-layer retrieval (FTS, graph expansion, community summaries, vector KNN, hint vector) replacing hybrid search
- mxbai-embed-large as default embedding model (1024-dim, 670MB, MTEB SOTA for code retrieval)
- Schema v13 migration — recreates vector tables at 1024 dimensions
- Hit@10 metric in `sqmd-bench compare`
- Single-item embed error logging (previously silent zero-vector fallback)

### Changed

- `sqmd search` now always uses layered search. `--keyword` flag for FTS-only
- Embed batch size reduced from 64 to 8 for mxbai-embed-large 512-token context
- Embed truncation reduced from 6000 to 1500 chars
- Removed nomic-embed-text `search_query:` / `search_document:` prefix system

### Removed

- `hybrid_search()`, `merge_results()`, `merge_hint_vec_results()` functions
- `--alpha` CLI flag
- ONNX runtime dependencies (`ort`, `ndarray`, `tokenizers`)
- nomic-embed-text as default embedding model

### Benchmark

Tested against Signet codebase (505 TypeScript files, 8,886 chunks, 200 queries):

| Lane | Hit@1 | Hit@3 | Hit@5 | Hit@10 | MRR |
|------|-------|-------|-------|--------|-----|
| FTS | 86% | 97.5% | 98.5% | 99.5% | 0.915 |
| Layered | 85% | 97% | 98.5% | 99.5% | 0.907 |

## [2.2.0] - 2026-04-10

### Added

- `sqmd hints` CLI command — decoupled LLM hint generation from indexing pipeline
- `SQMD_EMBED_MODEL` and `SQMD_HINT_MODEL` environment variables
- Ollama embedding backend via `/api/embed` (replaces ONNX runtime)

### Changed

- Switched embeddings from ONNX runtime to Ollama nomic-embed-text
- Binary size reduced from ~27MB to ~10MB (removed ort/ndarray/tokenizers deps)
- Shortened hint prompt to `"3 search queries for this code, one per line, no explanation:\n{content}"` with 1500 char truncation
- Increased embed batch size to 64

### Fixed

- Hybrid search scoring — raw unnormalized FTS rank and vector distance in alpha blend let vector dominate. Fixed with normalization to [0,1]
- Hint vector scoring — `score.max(hint_relevance)` overrode everything. Changed to additive boost
- Ollama hint generation blocking indexing pipeline — decoupled into separate `sqmd hints` command

## [2.1.0] - 2026-04-09

### Added

- Typed graph communities (Phase 5) — module communities (files connected by imports) and type-hierarchy communities (entities connected by extends/implements)
- `search_communities()`, `get_community_chunks()` functions

## [2.0.0] - 2026-04-08

### Added

- Relational hints from entity graph (Phase 4) — prospective search hints like "items that implement Trait" or "functions in this module"
- Entity dependencies graph layer (Phase 3) — structural relationships (extends, implements, contains) merged with import dependency graph
- Structural relationship extraction (Phase 2) — extends, implements, contains detection from AST
- Named chunks promoted to first-class symbol entities (Phase 1) — entities with kind, signature, parent scope metadata
- Ruby chunker (`tree-sitter-ruby`)
- YAML chunker (`tree-sitter-yaml`)
- JSON chunker (`tree-sitter-json`)
- TOML chunker (`tree-sitter-toml-ng`)
- Markdown chunker (regex-based, heading splits)
- Code retrieval benchmark harness (`sqmd-bench`)
- `CONTRIBUTING.md` with branch policy and code style

### Changed

- Complete README rewrite focusing on code intelligence for agents
- Source type filter fix in ContextAssembler

## [1.0.0] - 2026-04-07

### Added

- C, C++, CMake, QML, and Meson language support
- FTS5 special character stripping
- Initial release with TypeScript, Python, Go, Java language support
- FTS5 keyword search, import/call relationship graphs, entity graph
- Unix socket daemon mode
- Tree-sitter chunking with incremental re-indexing
- SQLite storage with content-hash deduplication
