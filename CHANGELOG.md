# Changelog

All notable changes to this project will be documented in this file.

## [3.4.1] - 2026-04-12

### Fixed

- MCP server no longer resolves to home directory as project root when `.sqmd/` lives at `~/.sqmd/`. `project_root_from_index_db()` now falls back to CWD with a warning, fixing wrong `tool_stats` counts and broken relative path resolution.
- `tool_index_file` validates the resolved path exists before indexing, preventing silent tombstoning when an incorrect root causes `root.join(path)` to produce a nonexistent file.

## [3.4.0] - 2026-04-12

### Changed

- **sqmd-review** rewritten as a git-connected, iterated review workflow.
  - Adds PR-aware mode: reads prior bot review comments via `gh api`, tracks
    dismissed/rebutted/addressed findings to prevent re-flagging.
  - Adds git scope detection: staged, uncommitted, branch diff, single commit,
    or PR-linked review with automatic base branch detection.
  - Improves context assembly: hunk-level file contents, `sqmd_deps` for blast
    radius, structural search via `sqmd_search` and `sqmd_get`.
  - Switches to FTS + entity graph only (no embedding/vector search needed).
  - Adds iteration loop: fix findings → re-review → repeat until `no_issues`
    verdict before pushing. The goal is zero remote bot comments.
  - Adds prior_review_status output field tracking suppression counts.
  - Added dismissal signal detection table for human replies.
  - Added `no_duplicate_imports` and `no_unused_imports` to convention checklist.

## [3.3.2] - 2026-04-12

### Fixed

- Release workflow now triggers only after `bump-version` completes via `workflow_run`, eliminating the race condition that prevented automatic tag and GitHub release creation.
- Bump-version workflow now runs with elevated permissions to push to protected main branch.

## [3.3.1] - 2026-04-11

### Changed

- `sqmd-review` now treats the actual git diff, commit blobs, and checked-out files as authoritative, using sqmd indexed context as a cross-checked second lens.
- `sqmd-review` now requires a changed-file coverage audit before a `no_issues` verdict and falls back to direct local searches when sqmd reports existing changed files as tombstoned, missing, or dependency-limited.
- README refreshed for current MCP tools, background embedding, review skill behavior, and git worktree usage.
- MCP server version bumped to 3.3.1.

### Fixed

- `sqmd mcp` now maps `.sqmd/index.db` back to the project root before indexing files, so relative paths are interpreted correctly.
- MCP index discovery now finds a main-worktree `.sqmd/index.db` when launched from a linked git worktree.

## [3.3.0] - 2026-04-11

### Added

- `embed_start` MCP tool — start embedding in a background thread, returns immediately
- `embed_progress` MCP tool — poll for embedding status, percentage, progress bar, ETA
- `embed_stop` MCP tool — stop a running embedding job gracefully
- Index validation on MCP startup — exits with clear error if index missing or empty
- `humantime()` helper for human-readable duration formatting in embed progress

### Changed

- MCP server version bumped to 3.3.0
- `setup_opencode` now uses absolute binary path via `current_exe()` (was bare `"sqmd"` in PATH)
- All `setup_*` functions now merge config keys instead of clobbering (preserves user-added `env`, `timeout`, etc.)
- `setup_claude` and `setup_cursor` also merge keys (previously only opencode had this fix planned)

### Fixed

- sqmd mcp no longer hangs indefinitely when harness fails to send `initialize` message
- MCP startup now prints clear error to stderr when index is missing or has 0 chunks
- Zombie process accumulation reduced — MCP server exits cleanly on BrokenPipe/EOF

## [3.2.0] - 2026-04-11

### Fixed

- MCP server auto-detects transport mode (Content-Length framed vs raw JSON) for Cursor compatibility
- MCP server removes stderr logging that Cursor treated as errors
- MCP server walks parent directories to find `.sqmd/index.db`
- Clippy `type_complexity` and `collapsible_if` warnings in `mcp_server.rs`

### Added

- 17 new language detections: Shell (sh/bash/zsh/fish), SQL, Dockerfile, Makefile, Kotlin, Swift, C#, PHP, Lua, Dart, Scala, Haskell, Elixir, Zig, XML/SVG, GraphQL, Protobuf
- `.sqmdignore` support — custom exclusion file in project root, same format as `.gitignore`
- Markdown code-block sub-chunking — fenced code blocks split into separate `Code`-typed chunks with language tags
- Structural relationship extraction wired up for Go, Java, C, C++, Ruby
- `.mjs`/`.cjs` extension support for JavaScript
- Filename-based detection for `Dockerfile` and `Makefile` (no extension)

### Changed

- SCSS/SASS/LESS separated from CSS into dedicated `Scss` language variant
- Markdown prose sections now tagged as `SourceType::Document` (was `SourceType::Code`)
- `sqmd-bench generate` loads native model once and reuses across all queries (was loading per-query)
- Template eval queries improved from `"how does fn foo() work"` to `"find the method named foo in config.rs"`
- Edition bumped to 2026 for all crates

### Fixed

- Hint deduplication — added `UNIQUE(chunk_id, hint_text)` index (schema v14) + `INSERT OR IGNORE`
- `sqmd hints` is now safe to re-run without creating duplicates
- Embed progress output fixed — writes to stderr consistently, shows percentage
- Embed progress was printing to stdout but flushing stderr (mismatch)

## [3.1.0] - 2026-04-11

### Added

- Lifecycle commands: `sqmd start`, `sqmd stop`, `sqmd setup`, `sqmd doctor`, `sqmd update`, `sqmd install`
- `sqmd setup` writes MCP config into OpenCode, Codex, and Claude Code settings automatically
- `sqmd doctor` runs diagnostics on index, native embedder, model manifest, MCP binary, and daemon status
- Native llama.cpp embedding runtime — replaces Ollama HTTP calls with direct GGUF loading via `llama-cpp-2`
- `NativeGenerator` — text generation via native llama.cpp for hint production (replaces Ollama HTTP client)
- `sqmd hints` now runs entirely through native llama.cpp (phi4-mini GGUF), no Ollama service required
- Metal GPU acceleration on Apple Silicon (99 GPU layers)
- MCP server (`sqmd mcp`) — JSON-RPC 2.0 over stdio with 5 tools (search, context, deps, stats, get)
- Daemon PID tracking at `~/.sqmd/daemon.pid` with stale PID cleanup
- `native-metal` feature for Metal GPU (default on macOS), `native` for CPU-only (Linux CI)
- `libc` and `dirs` dependencies in CLI crate

### Changed

- Default feature is now `native-metal` on macOS, `native` for CPU-only builds
- Removed `embed` feature — replaced by `native`
- Removed `ollama` feature entirely — all inference now runs through native llama.cpp
- Removed `ureq` dependency (was only used for Ollama HTTP calls)
- Removed `ollama.rs` module
- `generate_ollama_hints_batch` replaced by `generate_hints_batch` using `NativeGenerator`
- README rewritten for native runtime, MCP server, and lifecycle commands
- BENCHMARKING.md updated with native feature flags and current benchmark numbers

### Removed

- `ollama` feature flag
- `ollama.rs` module (Ollama HTTP client)
- `ureq` dependency
- All Ollama HTTP API calls — replaced by native llama.cpp inference

### Benchmark

Tested against Signet codebase (505 TypeScript files, 8,886 chunks, 200 queries):

| Lane | Hit@1 | Hit@3 | Hit@5 | Hit@10 | MRR |
|------|-------|-------|-------|--------|-----|
| FTS | 86% | 97.5% | 98.5% | 99.5% | 0.915 |
| Layered (native) | 86% | 97.5% | 98.5% | 99.5% | 0.915 |

Performance: ~0.55s per query, ~19 q/sec batch throughput.

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
