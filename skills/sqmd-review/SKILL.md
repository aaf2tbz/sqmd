---
name: sqmd-review
description: >
  Local self-review for commits, staged changes, or uncommitted changes against the actual
  checkout, cross-checked with sqmd-indexed codebase context.
  Like pr-reviewer but runs offline before pushing — no GitHub required.
  Use when the user asks to "review my changes", "self-review", "sqmd review", "review before push",
  "check my diff", "pre-push review", "review this commit", or "review this branch".
  Triggers on any request to review code changes locally using sqmd for codebase-aware context.
---

# sqmd-review

Self-contained code review of the real local checkout, cross-checked with sqmd-indexed codebase context. Adapted from [pr-reviewer](https://github.com/NicholaiVogel/pr-reviewer).

Core rule: the actual git diff and local files are authoritative. sqmd is a second lens for indexed dependency and semantic context; it must never replace direct review of the checked-out code, the selected commit/range, or local caller searches.

## Workflow

1. Detect scope: staged changes, uncommitted diff, a single commit, or branch diff against a verified base
2. Gather authoritative local context from git and the checked-out codebase
3. Run a changed-file coverage audit before considering sqmd output
4. Verify sqmd is pointed at the same repository/worktree before using mutating index operations
5. Gather sqmd context and explicitly cross-check it against local code
6. Run structured review against the combined local + sqmd context
7. Output structured JSON with findings and any tool-health observations

## Step 1 — Detect Scope

Determine what to review. Scope errors are the fastest way to make this skill noisy, so validate the base before reviewing.

- **Staged changes** (default): `git diff --cached`
- **Uncommitted diff**: `git diff` (use when user says "review my changes" with no staging)
- **Branch diff**: `git diff <base>...HEAD` (use when user says "review this branch" or "review before push")
- **Single commit**: `git show --stat --patch <sha>` and `git diff <sha>^!` (use when user says "review this commit" or names a commit)

### Base selection for branch/PR reviews

Prefer bases in this order:

1. **Explicit user base or PR metadata base SHA** if available (for example from a GitHub PR URL, `gh pr view --json baseRefOid`, or a connector response). Use `git diff <base_sha>...HEAD`.
2. **Upstream merge-base**: `git merge-base @{upstream} HEAD`, when the current branch tracks the review target.
3. **Remote default branch merge-base**: `git merge-base origin/main HEAD` or `origin/master` only after confirming that remote branch exists and is fresh enough for this checkout.

Do not blindly use local `main...HEAD`. Local `main` can be stale or unrelated in worktrees.

After selecting a base, run:

```
git diff --name-only <base>...HEAD
git diff --stat <base>...HEAD
```

Sanity-check the result:

- If the file list is unexpectedly huge, contains unrelated packages/docs, or contradicts PR metadata, stop and choose a better base.
- If a PR changed-file list is available, compare it with `git diff --name-only <base>...HEAD`. A mismatch should be reported as a tool/scope observation, not silently ignored.
- Record the base used in the review context.

Extract changed file paths and line ranges from the diff. Record whether reviewed content came from the worktree, the index, or a commit object.

## Step 2 — Gather Authoritative Local Codebase Context

Before sqmd context, review the actual code that will be committed or has been committed. This step is mandatory even when sqmd is healthy.

### Required local evidence

Collect:

```
git diff --name-only <scope>
git diff --stat <scope>
git diff --find-renames --find-copies <scope>
```

For single-commit reviews, use:

```
git show --name-only --stat <sha>
git show --find-renames --find-copies --patch <sha>
```

For every changed source file, read the real local file or commit blob:

- **Uncommitted/staged review**: read from the working tree with `sed -n`, `nl -ba`, or equivalent read-only commands.
- **Single commit review**: read post-change contents with `git show <sha>:<path>` and pre-change contents with `git show <sha>^:<path>` when needed.
- **Branch/range review**: read checked-out post-change files and use `git show <base>:<path>` for old behavior when relevant.

Include nearby surrounding code, not only the changed lines. For each changed function, type, exported symbol, route, config key, database query, or public API, also inspect direct callers/dependents with local search:

```
rg -n "<symbol-or-function-name>" .
rg -n "<config-key-or-sql-table-or-route>" .
rg -n "import .*<module>|from ['\"]<module>" <relevant roots>
```

Use `rg --files` to confirm file existence and package boundaries before assuming a path or module layout.

### Local review obligations

The structured review must be able to answer from local code:

- What exact behavior changed in the real diff?
- What callers, config loaders, tests, migrations, schemas, routes, or worker entry points touch the changed behavior?
- Does the checked-out code compile/type-check at the changed call sites, or is there an obvious signature mismatch?
- Do tests exercise the new behavior or only adjacent behavior?
- Are there local conventions in nearby files that the change violates in a correctness-relevant way?

If sqmd later disagrees with local files, prefer local files and report the sqmd disagreement as a tool observation.

## Step 3 — Changed-File Coverage Audit

Before using sqmd output as review evidence, prove that the local review covers the entire diff. This is the guardrail that keeps the local reviewer aligned with repository-hosted PR reviewers when sqmd indexing is stale or incomplete.

Build a changed-file table from the selected diff scope:

```markdown
| file | status | local content read | changed symbols/queries/configs | local caller search | sqmd status |
```

For each changed source or test file:

- Mark `local content read` only after reading the current file or commit blob from the actual checkout.
- Identify changed functions, exported types, config keys, SQL queries, worker entry points, or test cases from the diff.
- Run at least one direct local caller/import/search check for each changed symbol or module boundary. Use `rg`, `git grep`, `cargo check`, `tsc`, or nearby file reads as appropriate for the repo.
- If the file exists in the checkout but sqmd reports it as deleted, tombstoned, absent from `sqmd_ls`, or missing from dependency output, mark sqmd status as `stale` and keep reviewing with local evidence.
- Do not let `sqmd_context_cross_checked` appear in confidence reasons unless every sqmd result used as evidence was compared with the current local file or commit blob.

No-issue reviews require this audit. If any changed source/test file was not read locally, the review is incomplete and must not return `no_issues`.

When a changed test file is new or recently renamed, expect sqmd to be wrong more often. New test files must still be reviewed from the checkout and cross-checked against the production code they exercise.

## Step 4 — Verify sqmd Root and Index Safety

Before calling any sqmd mutating operation such as `sqmd_index_file`, verify that sqmd is serving the same repository/worktree being reviewed. This verification supplements the local review; it does not replace it.

1. Run `git rev-parse --show-toplevel` locally.
2. Pick one changed file that exists in the local worktree and is likely to have existed before this change.
3. Query sqmd for that path with a read-only operation first, such as `sqmd_ls(file_filter=<path>, limit=3)`, `sqmd_get(file_path=<path>, line=<known line>)`, or a highly specific `sqmd_search`.
4. Confirm returned file paths and snippets correspond to the local file content.

Only call `sqmd_index_file(path=<file>)` after this root check passes.

If sqmd appears rooted elsewhere, returns stale chunks that contradict local file content, returns unrelated paths despite a `file_filter`, omits a changed file that exists in the checkout, or `sqmd_index_file` reports an existing local file as deleted/tombstoned:

- Stop using mutating sqmd index operations for this review.
- Continue the code review using the authoritative local context from Step 2.
- Include a `tool_observations` entry in the final JSON explaining the sqmd root/index mismatch.
- Lower confidence if missing sqmd context materially limits cross-module validation.

## Step 5 — Gather and Cross-Check sqmd Context

For each changed file, use sqmd tools to build indexed codebase context after the local context is assembled:

### Dependency context

For each changed file with extensions `.rs`, `.ts`, `.tsx`, `.js`, `.py`, `.go`:

```
sqmd_deps(path=<file_path>, depth=1)
```

Include files that depend on the changed file (dependents) — these are the blast radius.

If `sqmd_deps` returns no dependencies for a file that clearly imports or is imported by other code, record that as a tool observation and compensate with the direct import/caller searches from Step 2.

When `sqmd_deps` is empty, incomplete, or stale for a changed file, run fallback local dependency checks before reviewing:

```
rg -n "from ['\"]<module>|import .*<module>|require\\(['\"]<module>" .
rg -n "<exported-symbol>|<function-name>|<type-name>|<config-key>|<table-name>" .
git grep -n "<symbol-or-module>" <base> -- .
```

For Rust, also inspect `mod` declarations and direct crate references:

```
rg -n "mod <module>|use .*<symbol>|<symbol>\\(" crates .
```

For TypeScript/JavaScript, include type-only imports and barrel exports:

```
rg -n "export .*from ['\"]<module>|import type .*<symbol>|Pick<.*<symbol>" .
```

Any final review that relied on these fallbacks should include `local_callers_checked` and `sqmd_context_limited` in the confidence reasons.

### Semantic search context

For each changed file, search for relevant symbols and patterns:

```
sqmd_search(query="<primary module or struct name from file>", file_filter="<file_path>", top_k=5)
```

```
sqmd_context(query="<what the change does, inferred from diff>", top_k=10)
```

Validate search results before using them as evidence:

- Treat results outside `file_filter` as low-confidence or irrelevant unless they clearly explain shared behavior.
- Prefer exact symbol queries over broad natural-language queries when initial results are noisy.
- Do not cite sqmd results as supporting evidence unless the returned path, symbol, and snippet are actually relevant.
- If sqmd search quality is poor, say so in `tool_observations` and rely on direct file context instead.

### Cross-check rules

For every meaningful sqmd result:

- Verify the file path exists in the current checkout with `rg --files` or an equivalent read-only file listing.
- Compare the sqmd snippet or symbol against the current local file if it is used as evidence.
- Label sqmd results as `verified`, `stale`, `irrelevant`, or `unavailable` in your review notes.
- Do not raise a code finding from sqmd alone. A finding must be confirmed against the actual diff, current local file contents, or a commit blob.
- Use sqmd to discover context you might otherwise miss, then validate that context locally before relying on it.

### Full context assembly

Build a context block in this structure:

```markdown
## Review Context

### PR Metadata
- Files changed: <list>
- Commit range: <range>
- Scope source: <staged | uncommitted | explicit base | PR base SHA | upstream merge-base>
- sqmd root/index status: <verified | stale | mismatched | unavailable>

### Diff
```diff
<parsed diff, truncated to 300 lines>
```

### Authoritative Local Code
For each changed file (up to 15 files, 200 lines each), include current local or commit-blob content with line numbers:
<file content with line numbers>

### Local Caller and Contract Checks
For each changed symbol/API/config/query:
<rg results, caller snippets, old-vs-new behavior notes>

### Dependency Impact
For each changed file's dependents:
<file path>: <snippet of how it depends on changed code>

### sqmd Search Results
<ranked chunks relevant to the changes>

### Tool Observations
<scope/index/search/dependency limitations, if any>
```

## Step 6 — Run Structured Review

Use the assembled context with the review prompt below. Output MUST be structured JSON in a fenced block tagged `pr-review-json`.

### Review Prompt

```
Review this local commit or change set. Be discerning. Use good judgment.

Focus on: bugs, security flaws, data corruption, race conditions, logic mistakes,
breaking changes, and patterns that diverge from codebase conventions.
Do not flag style-only issues unless they conceal a correctness problem.

You are a local review tool (sqmd-review). State this in the first line of your summary.

Instructions:
1. Read the actual local git diff or commit patch first. Validate whether the implementation achieves what the commit messages
   or branch description claim. If the diff says it does X, verify the code does X.
   If the implementation diverges from stated goals, flag this.
2. Check for introduced security vulnerabilities, injection risks, and attack-surface expansion.
3. Check adherence to repository conventions (AGENTS.md, CLAUDE.md, etc.) if present.
   Flag deviations only when they affect correctness, consistency, or maintainability.
4. Cross-reference changes against local caller searches and sqmd dependency impact. If a changed function's signature
   or behavior shifts, verify callers are updated or that the change is backward-compatible using the actual checkout.
   Do not accept an empty or stale sqmd dependency graph as proof of no callers.
5. Do not turn adjacent architecture preferences into blockers. A blocker must be grounded
   in concrete evidence from the changed code, local codebase context, or locally verified sqmd context and must directly affect
   correctness, security, or data integrity.
6. Keep tool failures separate from code findings. A stale sqmd index, weak search result,
   or wrong local base is a tool observation unless it directly proves a code defect.
7. Never raise a finding from sqmd-only evidence. Confirm it against the actual diff,
   checked-out file contents, commit blobs, or direct local searches before including it.
8. Before returning `no_issues`, verify the changed-file coverage audit is complete, every changed source/test file was read
   from the actual checkout or commit blob, and any stale sqmd dependency/search result was compensated by direct local search.

Do not approve or state the code is safe. Your role is to flag issues or signal readiness
for human review.

Output a JSON object in a fenced block tagged exactly `pr-review-json`.
```

See [references/review-schema.md](references/review-schema.md) for the full output schema.

## Step 7 — Present Findings

Parse the structured JSON output and present:

1. **Summary** — one paragraph overview
2. **Verdict** — `no_issues`, `comment`, or `request_changes`
3. **Confidence** — level with justification
4. **Findings** — grouped by severity:
   - **Blocking**: must fix before push (bugs, security, data integrity)
   - **Warning**: should discuss (logic concerns, convention drift)
   - **Nitpick**: minor observations
5. **Tool observations** — only when scope selection, sqmd indexing, dependency graph, or search quality affected the review

For each finding, show file, line number, and the issue. If sqmd context was used to
identify the issue, note what search/context supported the finding.

## Resources

- [references/review-schema.md](references/review-schema.md) — structured JSON output schema with examples and severity guidelines
