# Contributing to sqmd

Thanks for your interest in contributing! This is a small project with specific conventions — please read through before opening a PR.

## Development Setup

```bash
cargo build
cargo test
cargo clippy -- -D warnings
```

All three must pass before submitting a PR. CI enforces `clippy -D warnings` (zero warnings).

### With optional features

```bash
cargo build --features embed       # vector embeddings (ONNX Runtime)
cargo build --features ollama      # LLM hint generation (requires Ollama)
cargo build --features embed,ollama
```

## Code Style

- No comments unless they explain *why*, not *what*
- Follow existing patterns in the crate you're modifying
- New chunkers go in `crates/sqmd-core/src/languages/` and follow the `LanguageChunker` trait
- Prefer tree-sitter grammars compatible with `tree-sitter 0.24.x`. If no compatible grammar exists, use regex-based chunking (see `meson.rs` and `markdown.rs` for examples)
- Tests are required for new chunkers — cover at least: basic parsing, key extraction, and imports
- Feature-gated code uses `#[cfg(feature = "...")]` — keep feature dependencies minimal

## Commit Messages

Use conventional commits:

```
feat: add Lua chunker with tree-sitter
fix: pass source_types filter into SearchQuery
docs: update README language table
feat: add Ollama prospective hint generation
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
4. Register in `crates/sqmd-core/src/languages/mod.rs`
5. Wire into `crates/sqmd-core/src/index.rs` in `chunk_file_content()`
6. Add `Language::*` variant to `crates/sqmd-core/src/files.rs` with extension mapping
7. Update the language table in `README.md`
8. Write tests — see `html.rs` or `css.rs` for recent examples of complete chunkers with tests

### Chunker patterns

There are two patterns for `walk_declarations`:

1. **Recursive walk** — used by HTML, CSS, and most languages. The walker recurses into child nodes and creates chunks for each matching node kind. See `html.rs` for a clean example.

2. **Flat extraction** — used by JSON, TOML, YAML. The walker iterates immediate children and creates chunks directly. No recursion needed.

Choose based on whether the language has deeply nested structures (HTML elements inside elements) or flat ones (JSON key-value pairs).

## Running Benchmarks

```bash
cargo build -p sqmd-bench
./target/debug/sqmd-bench run /path/to/.sqmd/index.db         # existing ground truth
./target/debug/sqmd-bench generate /path/to/.sqmd/index.db    # generate eval queries
./target/debug/sqmd-bench compare /path/to/.sqmd/index.db     # compare retrieval lanes
```

The `run` subcommand uses the Redis codebase as its default ground-truth set (32 queries across 10 categories). The `generate` and `compare` subcommands support any indexed codebase.
