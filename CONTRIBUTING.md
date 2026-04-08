# Contributing to sqmd

Thanks for your interest in contributing! This is a small project with specific conventions — please read through before opening a PR.

## Development Setup

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

All three must pass before submitting a PR. CI enforces `clippy -D warnings` (zero warnings).

## Code Style

- No comments unless they explain *why*, not *what*
- Follow existing patterns in the crate you're modifying
- New chunkers go in `crates/sqmd-core/src/languages/` and follow the `LanguageChunker` trait
- Prefer tree-sitter grammars compatible with `tree-sitter 0.24.x`. If no compatible grammar exists, use regex-based chunking (see `meson.rs` and `markdown.rs` for examples)
- Tests are required for new chunkers — cover at least: basic parsing, key extraction, and imports

## Commit Messages

Use conventional commits:

```
feat: add Lua chunker with tree-sitter
fix: pass source_types filter into SearchQuery
docs: update README language table
```

## Branch Policy

- All changes go through pull requests
- **Force pushes and branch deletions are disabled** on protected branches
- Linear commit history is required (rebase, don't merge)
- Keep PRs focused — one concern per PR
- All PRs require conversation resolution (all review comments must be resolved)

## Adding a New Language

1. Add the tree-sitter grammar to `crates/sqmd-core/Cargo.toml` (must be compatible with `tree-sitter 0.24.x`, ABI 14)
2. Create `crates/sqmd-core/src/languages/{lang}.rs` implementing `LanguageChunker`
3. Register in `crates/sqmd-core/src/languages/mod.rs`
4. Wire into `crates/sqmd-core/src/index.rs` in `chunk_file_content()`
5. Add `Language::*` variant to `crates/sqmd-core/src/files.rs` if needed (detection already covers common extensions)
6. Update the language table in `README.md`
7. Write tests

## Running Benchmarks

```bash
cargo build -p sqmd-bench
./target/debug/sqmd-bench /path/to/.sqmd/index.db
```

The benchmark binary uses the Redis codebase as its default ground-truth set (25 queries across 6 categories).
