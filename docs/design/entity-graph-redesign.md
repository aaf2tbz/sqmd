# sqmd Entity Graph Redesign

## Problem

sqmd's entity graph is architecturally incomplete. The indexer creates one entity per **file** and stores named chunks as attributes on that file entity. Cross-file entity dependencies (`entity_dependencies`) are never populated during indexing. The chunk-level `relationships` table and entity-level `entity_dependencies` table operate independently with no synchronization. The result: graph expansion search rarely contributes meaningful results because the entity graph is too shallow to traverse.

An AI agent asking "what implements `RedisModule_CreateCommand`?" or "show me all callers of `dictRehash`" gets keyword search at best. The relational structure that tree-sitter already parses is thrown away.

## Current Architecture (what exists)

### Two disconnected graph layers

**Chunk relationships** (`relationships` table):
- 13 `rel_type` values defined in CHECK constraint
- Only 3 auto-populated by the indexer: `contains`, `calls`, `imports`
- `calls` uses blind regex (`\w+\(`) -- no type awareness
- `imports` uses path-based resolution -- no re-export/workspace handling
- `contains` is intra-file only (class→method nesting)

**Entity graph** (4 tables: `entities`, `entity_aspects`, `entity_attributes`, `entity_dependencies`):
- Entities created only at file granularity
- Each file entity gets an `"exports"` aspect listing its named chunks as attributes
- `entity_dependencies` is never written during indexing
- Temporal columns (`valid_from`/`valid_to`) exist but have no automated population
- 3-hop CTE-based graph expansion exists in `graph_boost_scored()` but operates on mostly-empty entity_deps

### What the schema supports but nothing populates

| Feature | Schema support | Populated? |
|---------|---------------|------------|
| Per-symbol entities (fn, struct, trait) | `entities.entity_type` is free TEXT | No -- only `file` entities |
| Cross-file entity deps | `entity_dependencies` table | No |
| `extends`/`implements` relationships | `relationships.rel_type` CHECK | No -- tree-sitter nodes not extracted |
| `overrides` relationships | `relationships.rel_type` CHECK | No |
| Temporal fact versioning | `valid_from`/`valid_to` columns | No |
| Constraint-type attributes | `entity_attributes.kind = 'constraint'` | No |
| Module/crate containment | Would need cross-file `contains` | No |

## Design

### 1. Symbol-level entities

Every named chunk becomes a first-class entity. The entity graph becomes the primary abstraction; chunks remain as storage for raw content and embeddings, but relationships flow through entities.

**Entity types** (extending current free-text `entity_type`):

| entity_type | Source | Examples |
|-------------|--------|---------|
| `file` | File path (existing) | `src/server.c` |
| `function` | tree-sitter `function_definition` | `dictRehash` |
| `method` | tree-sitter `method_definition` | `RedisModule_CreateCommand` |
| `class` | tree-sitter `class_declaration` | `redisObject` |
| `struct` | tree-sitter `struct_specifier` | `client` |
| `interface` | tree-sitter `interface_declaration` | `CommandParser` |
| `trait` | tree-sitter `trait_declaration` | `Drop` |
| `enum` | tree-sitter `enum_specifier` | `REDISMODULE_EVENT` |
| `impl` | tree-sitter `impl_item` | `impl Write for BufWriter` |
| `constant` | tree-sitter `constant_declaration` | `OBJ_SHARED_INTEGERS` |
| `module` | tree-sitter `module` / file path inference | `src/networking` |
| `macro` | tree-sitter `macro_definition` | `REDISMODULE_API` |
| `type` | tree-sitter `type_alias_declaration` | `sds` |

**Canonical name format**: `<qualifier>.<name>` where qualifier is the containing scope.
- `src/server.c.dictRehash` (function `dictRehash` in `server.c`)
- `src/server.c.clientRedisModule_CreateCommand` (method on struct `client`)
- Fully qualified names enable cross-project deduplication

**Entity schema additions** (v9 migration):
```sql
ALTER TABLE entities ADD COLUMN file_path TEXT;
ALTER TABLE entities ADD COLUMN language TEXT;
ALTER TABLE entities ADD COLUMN line_start INTEGER;
ALTER TABLE entities ADD COLUMN line_end INTEGER;
ALTER TABLE entities ADD COLUMN signature TEXT;
ALTER TABLE entities ADD COLUMN chunk_id INTEGER REFERENCES chunks(id);
CREATE INDEX idx_entities_file ON entities(file_path);
CREATE INDEX idx_entities_chunk ON entities(chunk_id);
```

This denormalizes key chunk fields onto the entity for fast lookup without a JOIN. The `chunk_id` FK links to the full source content.

### 2. Auto-populate entity_dependencies during indexing

After chunking each file, the indexer builds entity edges from tree-sitter AST data. This replaces the current `build_entity_graph()` which only creates file-level entities.

**Edge extraction rules** (per language):

| dep_type | Source (AST) | Example |
|----------|-------------|---------|
| `contains` | Parent-child nesting (existing) | `struct client` contains `authCallback` |
| `calls` | Function calls in body (existing, upgrade to type-aware) | `processInputBuffer` calls `readQueryFromClient` |
| `imports` | Import/use statements (existing) | `server.c` imports `dict.h` |
| `extends` | Class/struct extends clause | `class SortedSet extends BaseSet` |
| `implements` | Implements clause, trait bounds | `impl Write for BufWriter` |
| `overrides` | `override` keyword (Rust, C++, Java) | `fn drop(&mut self)` overrides `Drop::drop` |

**Language-specific extraction** (implemented in each `LanguageChunker` or a new `RelationshipExtractor` trait):

- **Rust**: `impl Trait for Struct` -> `implements` edges. `fn foo()` inside `impl Struct` -> `overrides` if trait method exists. `use crate::module::symbol` -> `imports`. `pub use` re-exports -> additional `imports` edges.
- **C/C++**: `struct foo { ... }` -> type hierarchy. Function calls resolved via forward declarations in headers. `#include` -> `imports`.
- **Python**: `class Foo(Base)` -> `extends`. `def foo(self)` -> method on class. `from x import y` -> `imports`.
- **TypeScript**: `implements Foo` -> `implements`. `extends Foo` -> `extends`. `import { x } from 'y'` -> `imports`.
- **Go**: Interface satisfaction (structural typing) -> `implements`. Import paths -> `imports`.
- **Ruby**: `class Foo < Bar` -> `extends`. `include Mod` -> `implements`. `require 'path'` -> `imports`.

**Implementation approach**: Rather than extracting relationships during chunking (which is already complex), add a separate `extract_entity_relationships()` function that runs after chunking per file. This function takes the tree-sitter AST (already parsed) and the list of created chunks/entities, and emits `(source_entity, target_entity, dep_type)` tuples.

### 3. Merge the two graph layers

Currently, `relationships` (chunk-to-chunk) and `entity_dependencies` (entity-to-entity) are independent. The redesign makes `entity_dependencies` the **single source of truth** for all structural relationships.

**Migration strategy**:
1. During indexing, all relationship data flows into `entity_dependencies`
2. The `relationships` table becomes a **materialized view** (or thin wrapper) derived from entity_deps: `SELECT source.chunk_id, target.chunk_id, dep_type FROM entity_dependencies ed JOIN entities source ON ... JOIN entities target ON ...`
3. Backward compatibility: keep `relationships` table, populate it from entity_deps via triggers or post-index pass
4. Eventually deprecate `relationships` in favor of entity_deps queries

**Relationship flow**:
```
tree-sitter AST
    |
    v
LanguageChunker extracts chunks
    |
    v
RelationshipExtractor emits (source_entity, target_entity, dep_type)
    |
    v
entity_dependencies table (single source of truth)
    |
    v
relationships table (materialized for backward compat)
```

### 4. Prospective indexing: graph-driven hints

The current `hints` system generates natural language phrases per chunk (function names, quoted strings, date patterns). Expand this to generate **relational hints** from the entity graph.

**New hint types**:

| Hint type | Source | Example |
|-----------|--------|---------|
| `symbol` | Entity name + type | "the `dictRehash` function" |
| `caller` | Reverse `calls` edges | "functions that call `processInputBuffer`" |
| `callee` | Forward `calls` edges | "`processInputBuffer` calls `readQueryFromClient`" |
| `heir` | `extends`/`implements` edges | "classes that extend `BaseClient`" |
| `member` | `contains` edges | "methods of `redisObject`" |
| `importer` | Reverse `imports` edges | "files that import `dict.h`" |
| `cluster` | Community membership | "networking module functions" |
| `chain` | Transitive call closure | "call chain from `acceptHandler` to `createClient`" |

These hints enable FTS to find relevant chunks even when the query doesn't match chunk content directly. An agent asking "what are the entry points into the command pipeline?" would match hints like "functions that call `processInputBuffer`" without needing to know the specific function name.

**Implementation**: After indexing, run a post-pass that traverses the entity graph and generates hints for each entity's 1-2 hop neighborhood. Store in the existing `hints` table with a `hint_type` column (v9 migration).

### 5. Community graph upgrade

Current communities are path-based directory groupings with template summaries. Upgrade to entity-graph-based communities:

**Community types**:
- **Directory communities** (existing): group by file path prefix
- **Module communities** (new): group by import connectivity -- files that import each other form a cluster
- **Type hierarchy communities** (new): group by `extends`/`implements` -- all implementors of a trait form a cluster

**Community summaries**: Currently template-based. Future: use the entity graph to generate structural summaries (e.g., "this module exports 5 functions used by 3 other modules, contains 2 structs with 12 methods").

### 6. Context assembly improvements

The `ContextAssembler` (`context.rs`) currently expands context by following `relationships` edges. After the redesign:

1. Follow entity_deps instead of relationships
2. Prioritize by `strength` (existing column, currently unused)
3. Use entity type to weight expansion: `function` > `method` > `constant` > `type`
4. Limit expansion depth by token budget rather than fixed hop count
5. Cross-reference community membership to avoid over-expanding a single module

## Schema migration plan (v9)

```sql
-- v9: symbol-level entities + relational hints

-- Entity denormalization
ALTER TABLE entities ADD COLUMN file_path TEXT;
ALTER TABLE entities ADD COLUMN language TEXT;
ALTER TABLE entities ADD COLUMN line_start INTEGER;
ALTER TABLE entities ADD COLUMN line_end INTEGER;
ALTER TABLE entities ADD COLUMN signature TEXT;
ALTER TABLE entities ADD COLUMN chunk_id INTEGER REFERENCES chunks(id);
CREATE INDEX IF NOT EXISTS idx_entities_file ON entities(file_path);
CREATE INDEX IF NOT EXISTS idx_entities_chunk ON entities(chunk_id);
CREATE INDEX IF NOT EXISTS idx_entities_type_file ON entities(entity_type, file_path);

-- Relational hints
ALTER TABLE hints ADD COLUMN hint_type TEXT NOT NULL DEFAULT 'symbol';
CREATE INDEX IF NOT EXISTS idx_hints_type ON hints(hint_type);

-- Entity dependency strength gets used
-- (column already exists, just needs population logic)

-- Relationships become materialized view of entity_deps
-- (no schema change, just indexing behavior change)
```

## Implementation phases

### Phase 1: Symbol-level entities
- Modify `build_entity_graph()` to create per-symbol entities instead of just file entities
- Populate new columns (`file_path`, `language`, `line_start`, `line_end`, `signature`, `chunk_id`)
- Keep file entities as container entities, add `contains` edges from file to symbols
- Estimated: ~200 LOC changes across `index.rs`, `entities.rs`

### Phase 2: Relationship extraction
- Add `RelationshipExtractor` trait alongside `LanguageChunker`
- Implement for Rust, C, C++, Python, TypeScript, Go, Ruby
- Populate `entity_dependencies` during indexing
- Estimated: ~500 LOC across new `src/relationships_extract.rs` + per-language files

### Phase 3: Merge graph layers
- Change indexing to write entity_deps as primary, derive relationships
- Update `graph_boost_scored()` to operate on populated entity_deps
- Update `context.rs` dependency expansion to use entity_deps
- Estimated: ~300 LOC changes across `index.rs`, `search.rs`, `context.rs`

### Phase 4: Relational hints
- Add post-index pass to generate graph-driven hints
- Add `hint_type` column
- Expand FTS search to weight relational hints higher
- Estimated: ~200 LOC across `entities.rs`, `search.rs`

### Phase 5: Community upgrade
- Add module and type-hierarchy community detection
- Upgrade community summaries to be graph-aware
- Estimated: ~300 LOC across `communities.rs`

## Success metrics

- Entity graph density: from ~1 entity per file to ~5-20 entities per file (depending on language)
- Entity dependencies: from 0 to proportional to codebase size (roughly 2-5 deps per entity)
- Graph expansion search: should contribute results for 30%+ of queries (currently near 0%)
- Call chain queries: "show me all callers of X" should return results via graph, not just FTS
- Context assembly: dependency expansion should pull in structurally related code, not just keyword matches
