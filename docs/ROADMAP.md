# sqmd — Development Roadmap

## Overview

A single Rust binary (~5MB) that turns any codebase into a queryable SQLite database of Markdown chunks, with tree-sitter parsing, local embeddings, FTS5 + vector hybrid search, and an import/call relationship graph. Zero network, zero external services, works offline.

---

## Phase 0: Spike (Days 1-2)

Validate the two riskiest dependencies before committing to the stack.

### 0.1 sqlite-vec loadable extension via rusqlite
- Can `rusqlite` load `sqlite-vec` as a loadable extension?
- Test `CREATE VIRTUAL TABLE ... USING vec0(...)` and KNN queries
- **Fallback:** Custom brute-force cosine similarity in Rust (fast enough for <1M chunks)

### 0.2 ONNX Runtime (ort) + nomic-embed-text-v1.5
- Can `ort` load and run `nomic-embed-text-v1.5` with q8 quantization?
- Test single-text and batch embedding throughput
- Measure model load time and resident RAM
- **Fallback:** HTTP embedding API to local Ollama (adds network dep but works)

### 0.3 Spike deliverable
- Minimal Rust project that opens SQLite, loads sqlite-vec, embeds a string, runs a KNN query
- Document results in `docs/SPIKE_RESULTS.md`

---

## Phase 1: Foundations (Week 1)

**Goal:** Project scaffold, SQLite schema, CLI skeleton, basic file ingestion.

### 1.1 Cargo workspace

```
sqmd/
├── Cargo.toml              (workspace)
├── crates/
│   ├── sqmd-core/          (library — schema, chunking, search, embedding, graph)
│   └── sqmd-cli/           (binary — user-facing commands)
└── docs/
```

### 1.2 SQLite schema (`sqmd-core/src/schema.rs`)

```sql
CREATE TABLE files (
    path      TEXT PRIMARY KEY,
    language  TEXT NOT NULL,
    size      INTEGER,
    mtime     REAL,
    hash      TEXT NOT NULL,
    indexed_at TEXT DEFAULT (datetime('now'))
);

CREATE TABLE chunks (
    id           INTEGER PRIMARY KEY,
    file_path    TEXT NOT NULL REFERENCES files(path),
    language     TEXT NOT NULL,
    chunk_type   TEXT NOT NULL,
    name         TEXT,
    signature    TEXT,
    line_start   INTEGER NOT NULL,
    line_end     INTEGER NOT NULL,
    content_md   TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    metadata     TEXT,
    importance   REAL DEFAULT 0.5,
    created_at   TEXT DEFAULT (datetime('now')),
    updated_at   TEXT DEFAULT (datetime('now'))
);

CREATE TABLE relationships (
    id       INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    target_id INTEGER NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    rel_type TEXT NOT NULL,
    metadata TEXT
);

CREATE TABLE embeddings (
    chunk_id  INTEGER PRIMARY KEY REFERENCES chunks(id) ON DELETE CASCADE,
    vector    BLOB NOT NULL,
    dimensions INTEGER NOT NULL,
    model     TEXT NOT NULL,
    created_at TEXT DEFAULT (datetime('now'))
);

CREATE VIRTUAL TABLE chunks_fts USING fts5(
    content_md,
    name,
    signature,
    file_path,
    content='chunks',
    content_rowid='id'
);
```

Auto-sync triggers for FTS5. WAL mode enabled by default.

### 1.3 CLI commands (`sqmd-cli`)

```
sqmd init              # Create .sqmd/index.db, add to .gitignore
sqmd index [path]      # Full index of project (default: cwd)
sqmd watch             # File watcher mode (incremental re-index)
sqmd search <query>    # Hybrid search (FTS5 + vector once Phase 3 lands)
sqmd get <file:line>   # Retrieve chunk at file:line
sqmd deps <file:line>  # Show dependency graph for a chunk
sqmd stats             # Index statistics
sqmd reset             # Drop and re-index
```

### 1.4 File ingestion (no tree-sitter yet)

- Walk directory, skip `.git`, `node_modules`, `target`, `.sqmd`, `dist`, `build`, `.venv`
- Respect `.gitignore` (via `ignore` crate)
- Detect language from file extension
- SHA-256 content hash for change detection
- Store file metadata in `files` table

### 1.5 Dependencies

| Crate | Purpose |
|-------|---------|
| `rusqlite` | SQLite bindings |
| `clap` (derive) | CLI argument parsing |
| `serde` / `serde_json` | Serialization |
| `walkdir` | Directory traversal |
| `ignore` | .gitignore-aware walking |
| `sha2` | Content hashing |

---

## Phase 2: Tree-sitter Chunking (Week 2-3)

**Goal:** Parse source files into semantically meaningful Markdown chunks.

### 2.1 Language support (v1)

| Language | Grammar | Priority |
|----------|---------|----------|
| TypeScript | `tree-sitter-typescript` | Critical |
| TSX | `tree-sitter-typescript` (tsx variant) | Critical |
| Rust | `tree-sitter-rust` | High |
| Python | `tree-sitter-python` | High |

### 2.2 Chunker trait

```rust
pub struct Chunk {
    pub file_path: PathBuf,
    pub language: String,
    pub chunk_type: ChunkType,  // Function, Class, Method, Interface, Type, Module, Section
    pub name: Option<String>,
    pub signature: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub content_md: String,
    pub metadata: Value,
}

pub trait LanguageChunker: Send + Sync {
    fn language(&self) -> &str;
    fn chunk(&self, source: &str, path: &Path) -> Result<Vec<Chunk>>;
}
```

### 2.3 AST walking strategy

Walk the tree depth-first, extracting:

- **Module-level** imports → stored as relationship rows
- **Declarations** (functions, classes, interfaces, types) → individual chunks
- **Methods/properties** inside classes → chunks with "contains" relationship to parent
- **Unclaimed lines** between declarations → grouped into "section" chunks (max ~50 lines)

### 2.4 Markdown rendering

Each chunk renders as:

```markdown
### `functionName(param: Type): Return`

**File:** `src/auth/login.ts:42-67`
**Type:** function
**Exports:** yes
**Imports:** `./db.findUser`, `./types.Credentials`

​```typescript
export async function authenticateUser(
  credentials: Credentials
): Promise<AuthResult> {
  const user = await db.findUser(credentials.email);
  return createSession(user);
}
​```
```

### 2.5 Context windows

- Include 2-3 lines before each chunk (decorators, JSDoc, comments)
- Include first line of next sibling (boundary context)
- Each chunk is self-contained enough for agent comprehension

### 2.6 Dependencies

| Crate | Purpose |
|-------|---------|
| `tree-sitter` | Core parsing library |
| `tree-sitter-typescript` | TS/TSX grammar |
| `tree-sitter-rust` | Rust grammar |
| `tree-sitter-python` | Python grammar |

---

## Phase 3: Incremental Indexing (Week 3-4)

**Goal:** Fast incremental updates when files change.

### 3.1 Change detection

- Compare file `mtime` + `hash` against `files` table
- Three states: `unchanged` (skip), `modified` (re-chunk), `deleted` (remove all chunks + relationships)
- Parallel file processing via `rayon`

### 3.2 File watcher

- `notify` crate (kqueue on macOS, inotify on Linux, ReadDirectoryChanges on Windows)
- 200ms debounce window (coalesce rapid save events)
- On change: hash check → if different, re-index that single file
- On delete: cascade remove from `files`, `chunks`, `relationships`, `embeddings`, `chunks_fts`

### 3.3 Connection management

- SQLite WAL mode for concurrent reads during writes
- 1 writer connection, N reader connections
- Embeddings written asynchronously without blocking reads

### 3.4 Performance targets

| Metric | Target |
|--------|--------|
| Per-file parse + chunk | <50ms |
| Per-file with embedding | <100ms |
| Full index (10k files, cold) | <60s |
| Incremental (single file) | <200ms |

---

## Phase 4: Embeddings + Vector Search (Week 4-6)

**Goal:** Semantic search on top of keyword search.

### 4.1 Embedding pipeline (`sqmd-core/src/embed.rs`)

- `ort` crate for ONNX Runtime
- Model: `nomic-embed-text-v1.5` (768-dim, q8 quantized, ~50MB)
- First-run download from HuggingFace, cached in `~/.sqmd/models/`
- Batch embedding for throughput (process N chunks per ONNX session)

### 4.2 Vector storage

**Primary:** `sqlite-vec` loadable extension (validated in Phase 0 spike)
```sql
CREATE VIRTUAL TABLE chunks_vec USING vec0(embedding float[768]);
```

**Fallback (if spike fails):** Vectors as BLOBs in `embeddings` table, brute-force cosine similarity in Rust. Still fast for <1M chunks.

### 4.3 Hybrid search engine (`sqmd-core/src/search.rs`)

```rust
pub struct SearchQuery {
    pub text: String,
    pub top_k: usize,           // default 20
    pub alpha: f64,             // 0.7 = 70% vector, 30% keyword
    pub file_filter: Option<String>,
    pub type_filter: Option<ChunkType>,
    pub min_score: f64,         // default 0.3
}

pub struct SearchResult {
    pub chunk: Chunk,
    pub score: f64,
    pub vec_distance: Option<f64>,
    pub fts_rank: Option<f64>,
    pub context_chunks: Vec<Chunk>,
}
```

### 4.4 Hybrid scoring algorithm

1. Embed query text → query vector
2. FTS5 MATCH query → normalized scores
3. vec0 KNN search → normalized distances
4. Merge: `hybrid_score = alpha * vec_score + (1 - alpha) * fts_score`
5. Single-source penalty: if a result only appears in one index, multiply by 0.8
6. Fetch 1-2 adjacent chunks (line proximity) as context
7. Return top-K

### 4.5 Performance targets

| Metric | Target |
|--------|--------|
| Single chunk embed | <10ms |
| Batch embed (64 chunks) | <200ms |
| Vector search (100k chunks) | <5ms |
| FTS5 search | <3ms |
| Full hybrid query | <20ms |
| Model load (cold) | <2s |
| Model RAM (resident) | ~300MB |

---

## Phase 5: Relationship Graph (Week 6-8)

**Goal:** Import/call dependency graph for traversal queries.

### 5.1 Import extraction (per language)

- TypeScript: `import { X } from './path'` → relationship to exported chunk in target
- Rust: `use crate::module::Item` → relationship
- Python: `from module import X` → relationship

### 5.2 Call graph extraction (best-effort, static)

- Within function bodies, find identifiers matching known function/method names
- Cross-file resolution via import relationships
- Inherently approximate for dynamic languages — treated as hints, not proofs

### 5.3 Graph queries (`sqmd-core/src/graph.rs`)

```rust
pub fn get_dependencies(db: &Connection, chunk_id: i64, depth: usize) -> Vec<Chunk>
pub fn get_dependents(db: &Connection, chunk_id: i64, depth: usize) -> Vec<Chunk>
pub fn get_path(db: &Connection, from: i64, to: i64) -> Vec<Chunk>
```

Recursive CTE traversal in SQL.

### 5.4 Graph-augmented search

When a chunk matches a search query, automatically include its direct dependencies (configurable depth). "I searched for auth middleware and got the whole auth flow."

---

## Phase 6: Agent API + Context Assembly (Week 8-10)

**Goal:** Turn sqmd into something agents can query programmatically.

### 6.1 Daemon mode (`sqmd serve`)

- Unix socket (`~/.sqmd/daemon.sock`)
- JSON request/response protocol
- Auto-start on first query, stay resident
- Background file watcher + incremental re-index

### 6.2 Query protocol

```json
{
    "method": "search",
    "params": {
        "query": "how does authentication work",
        "top_k": 10,
        "include_deps": true,
        "dep_depth": 1
    }
}
```

### 6.3 Context assembly (`sqmd-core/src/context.rs`)

Given a query or working files:

1. Search for relevant chunks
2. Fetch surrounding context chunks (±1 sibling)
3. If `include_deps`, fetch dependency graph chunks
4. Token-count via `tiktoken-rs` (cl100k base)
5. Trim to budget (default: 8000 tokens)
6. Render as a single Markdown document for context injection

### 6.4 Token counting

- `tiktoken-rs` for cl100k base encoding
- Accurate budget enforcement — no more guessing about context size

---

## Phase 7: Signet Integration (Week 10-12)

**Goal:** Replace Signet's LLM-heavy extraction pipeline with sqmd.

### 7.1 Transcript chunking

- Parse Signet JSONL transcripts into sqmd chunks (no LLM)
- Store in `transcripts` table alongside code chunks

### 7.2 Replace extraction worker

- Current: transcript → LLM extract → LLM decide → write (3 calls)
- New: transcript → sqmd chunk → embed → deterministic dedup → write (0 calls)

### 7.3 Replace decision worker

- Content hash dedup (exact match)
- Cosine similarity dedup (threshold 0.95)
- Importance scoring: recency + turn density + error count (from transcript structure)

### 7.4 Replace synthesis worker

- Query sqmd for top-scored recent chunks
- Template-based MEMORY.md assembly (no LLM render)
- Optional: single lightweight LLM pass for prose smoothing

### 7.5 Migration path

- Add `chunks` table to existing `memories.db`
- Parallel indexing (sqmd + legacy) during transition
- Switch read path to sqmd chunks
- Deprecate legacy extraction pipeline

---

## Dependency Risk Matrix

| Dependency | Risk | Mitigation |
|-----------|------|------------|
| `tree-sitter` + language grammars | Low | Battle-tested (Neovim, Helix, Zed) |
| `rusqlite` | Low | Most popular Rust SQLite lib |
| `sqlite-vec` loadable extension | **Medium** | Spike first; fallback to custom KNN in Rust |
| `ort` (ONNX Runtime) | **Medium** | Spike first; fallback to Ollama HTTP API |
| `notify` (file watcher) | Low | Mature cross-platform |
| `tiktoken-rs` | Low | Pure Rust, well-maintained |
| `clap` | Low | De facto Rust CLI framework |
| `rayon` | Low | Standard parallelism |

---

## Timeline Summary

| Week | Phase | Milestone |
|------|-------|-----------|
| 0 | Spike | Validate sqlite-vec + ort |
| 1 | 1 | Schema, CLI, file ingestion |
| 2-3 | 2 | Tree-sitter chunking (TS, Rust, Python) |
| 3-4 | 3 | Incremental indexing, file watcher |
| 4-6 | 4 | Embeddings + hybrid search |
| 6-8 | 5 | Relationship graph + traversal |
| 8-10 | 6 | Agent API + context assembly |
| 10-12 | 7 | Signet integration |

**MVP (usable by agents) after Phase 4 — Week 6.**
