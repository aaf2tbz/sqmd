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
cargo build --features native            # local llama.cpp embeddings + hints (CPU)
cargo build --features native-metal      # + Metal GPU acceleration (macOS)
```

The default build uses `native-metal` on macOS, which includes llama.cpp with Metal support.

## Code Style

- No comments unless they explain *why*, not *what*
- Follow existing patterns in the crate you're modifying
- New chunkers go in `crates/sqmd-core/src/languages/` and follow the `LanguageChunker` trait
- Prefer tree-sitter grammars compatible with `tree-sitter 0.24.x` (ABI 14). If no compatible grammar exists, use regex-based chunking (see `meson.rs` and `markdown.rs` for examples)
- Tests are required for new chunkers — cover at least: basic parsing, key extraction, and imports
- Feature-gated code uses `#[cfg(feature = "...")]` — keep feature dependencies minimal
- The `native` feature gates all llama.cpp inference (embeddings and hints). Don't add new runtime dependencies outside of this feature.

## Commit Messages

Use conventional commits:

```
feat: add Lua chunker with tree-sitter
fix: pass source_types filter into SearchQuery
docs: update README language table
fix(mcp): project root resolves to home dir when .sqmd is at ~/.sqmd
chore: bump version to 3.4.1
```

Scope prefix is encouraged for non-trivial changes: `fix(mcp):`, `feat(search):`, etc.

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
5. Add `Language::*` variant to `crates/sqmd-core/src/files.rs` with extension mapping
6. Update the language table in `README.md`
7. Write tests — see `html.rs` or `css.rs` for recent examples of complete chunkers with tests

### Chunker patterns

There are two patterns for `walk_declarations`:

1. **Recursive walk** — used by HTML, CSS, and most languages. The walker recurses into child nodes and creates chunks for each matching node kind. See `html.rs` for a clean example.

2. **Flat extraction** — used by JSON, TOML, YAML. The walker iterates immediate children and creates chunks directly. No recursion needed.

Choose based on whether the language has deeply nested structures (HTML elements inside elements) or flat ones (JSON key-value pairs).

## Adding a New MCP Tool

1. Add tool metadata to the `tools()` function in `crates/sqmd-core/src/mcp_server.rs` (name, description, inputSchema)
2. Add a dispatch arm in `call_tool()` matching the tool name
3. Implement the handler function — must return `Result<Vec<Value>, Box<dyn std::error::Error>>`
4. Return results as `[{"type": "text", "text": "..."}]` format
5. Add tests in `#[cfg(test)] mod tests` if the tool has non-trivial logic
6. Update `README.md` MCP tools table

## Running Benchmarks

```bash
cargo build -p sqmd-bench --features native --release
./target/release/sqmd-bench run /path/to/.sqmd/index.db         # existing ground truth
./target/release/sqmd-bench generate /path/to/.sqmd/index.db    # generate eval queries
./target/release/sqmd-bench compare /path/to/.sqmd/index.db     # compare retrieval lanes
```

The `run` subcommand uses the Redis codebase as its default ground-truth set (32 queries across 10 categories). The `generate` and `compare` subcommands support any indexed codebase.

## CI

CI runs on every push to `main` and on PRs:

1. **rust.yml** — `cargo build`, `cargo test`, `cargo clippy -D warnings` (with `--no-default-features --features native`)
2. **bump-version.yml** — Auto-extracts version from CHANGELOG header, bumps crate versions, pushes
3. **release.yml** — Runs after bump-version: creates git tag and GitHub Release

## Project Structure

```
sqmd-core/    — Library: schema, chunking, search, embeddings, MCP server, daemon
sqmd-cli/     — Binary: 24 CLI subcommands, harness setup, doctor
sqmd-bench/   — Benchmark harness: run, generate, compare
```

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the full component diagram and data flow.
