---
name: sqmd-review
description: >
  Git-connected code review using sqmd-indexed codebase context and GitHub PR history.
  Runs locally before pushing to prevent bot review comments before they happen.
  Reads prior PR review comments, tracks dismissals, and uses sqmd dependency graphs
  for blast-radius analysis. Works offline for uncommitted changes or connected to GitHub
  for PR-aware review.
  Use when the user asks to "review my changes", "self-review", "sqmd review", "review before push",
  "check my diff", "pre-push review", "review this commit", "review this branch", or "review PR #X".
  Triggers on any request to review code changes with codebase-aware context.
---

# sqmd-review

Git-connected code review using sqmd-indexed codebase context. Prevents bot review
comments by catching issues locally before pushing. Adapted from
[pr-reviewer](https://github.com/NicholaiVogel/pr-reviewer).

## Design Philosophy

The goal is **zero-comment pushes**: every issue the remote bot would flag should be
caught here first. This means:

- **Full codebase context** via sqmd structural search (FTS + entity graph), not just the diff
- **Prior review awareness** — read dismissed/rebutted bot comments so we don't re-flag them
- **Blast-radius analysis** — dependency graphs show what breaks when signatures change
- **Hunk-level precision** — findings reference exact changed lines, not whole files
- **Anti-hallucination by construction** — every finding must quote verbatim code from
  context the agent actually read
- **Iterate until clean** — if the review finds issues, fix them, re-review, repeat until
  the verdict is `no_issues`. Only then commit and push. The goal is to reduce remote
  bot review comments to zero.

### Important: sqmd is FTS + entity graph only

sqmd-review uses sqmd's **structural search tools** (FTS, entity graph, community detection,
dependency graphs) — **not** semantic/embedding search. Do not use `sqmd_embed` or
`sqmd_context` (which depends on embeddings). The structural tools are sufficient because
they provide dependency graphs (`sqmd_deps`), entity expansion (`sqmd_search`), and
code chunk retrieval (`sqmd_get`, `sqmd_cat`, `sqmd_ls`) without needing vector embeddings.

## Workflow

```
1. Detect scope → 2. Ensure sqmd index → 3. Gather git context → 4. Assemble review context
→ 5. Run structured review → 6. Fix findings → 7. Re-review → repeat until clean → push
```

### Iteration loop

The review is not a one-shot pass. It is a loop:

```
review → findings? → fix → review again → findings? → fix → review again → ... → no_issues → push
```

After each fix, re-run the full review workflow (steps 1-5) against the updated diff.
The review must verify that the fixes didn't introduce new issues. Continue until the
verdict is `no_issues` with high confidence. Only then commit and push.

## Step 1 — Detect Scope

Determine what to review. Detect automatically from the user's request:

| Trigger | Command | Scope |
|---------|---------|-------|
| "review my changes" | `git diff` (unstaged) | Uncommitted changes |
| "review before push" / "pre-push review" | `git diff <base>...HEAD` | Branch vs base |
| "review this commit" | `git show <sha>` | Single commit |
| "review PR #X" / "review my pr" | `gh pr view <n> --json ...` | PR diff + metadata |
| "self-review" / "sqmd review" | `git diff --cached` (staged) or `git diff` | Staged/unstaged |

### Branch detection

When reviewing a branch, determine the base automatically:

```bash
# If on a feature branch, base is the merge target
gh pr view --json baseRefName 2>/dev/null | jq -r .baseRefName
# Fallback: default branch
git symbolic-ref refs/remotes/origin/HEAD 2>/dev/null | sed 's@^refs/remotes/origin/@@'
```

### PR-aware review

When a PR number is provided or detected, extract:

```bash
gh pr view <n> --json number,title,body,baseRefName,headRefName,headRefOid,files,commits
```

Store the PR body — the review must verify the implementation matches the stated goals.

## Step 2 — Ensure sqmd Index

The sqmd index must be fresh before review. Check and update:

```bash
# Check if index exists
sqmd_stats
# Re-index changed files (fast incremental)
sqmd_index_file
```

**Do not run `sqmd_embed`** — sqmd-review uses FTS + entity graph search, not embeddings.
The index provides structural context (dependency graphs, entity relationships, code chunks)
which is sufficient for review.

## Step 3 — Gather Git Context

### Prior review comments (PR-aware mode only)

If a PR is linked, fetch existing review comments to avoid re-flagging dismissed issues:

```bash
# PR inline review comments (bot + human)
gh api repos/<owner>/<repo>/pulls/<n>/comments --jq '.[] | {file,path,line,body,user:.user.login,created_at}'
# PR top-level review comments
gh api repos/<owner>/<repo>/pulls/<n>/reviews --jq '.[] | {body,state,user:.user.login}'
# PR issue comments (general discussion)
gh api repos/<owner>/<repo>/issues/<n>/comments --jq '.[] | {body,user:.user.login}'
```

Parse the prior comments into a structured list:
- `[dismissed by human]` — human replied rejecting the concern or marking as out of scope
- `[rejected with rationale]` — human explained why the finding is wrong
- `[likely addressed]` — the code at the referenced line has changed since the comment
- `[out of scope for this pr]` — human explicitly said it's out of scope
- `[active]` — still open, needs to be checked against current diff

For each `[active]` comment, check if the referenced file+line still exists in the current
diff. If the code at that location has changed, mark as `[likely addressed]`.

### Commit messages

For branch reviews, extract commit messages for the diff range:

```bash
git log <base>...HEAD --pretty=format:"%s%n%b---"
```

These are used to verify the implementation matches stated intent.

### Repository conventions

Read repo convention files if they exist:

```bash
# Try these in order, use first found
cat AGENTS.md CLAUDE.md CONTRIBUTING.md 2>/dev/null
```

## Step 4 — Assemble Review Context

Build a context block using git data + sqmd. This is the most important step —
the quality of the review depends on the quality of the context.

### 4a — Diff extraction

Get the diff, truncated to 500 lines. Use `--unified=5` for context:

```bash
git diff <base>...HEAD --unified=5 2>/dev/null | head -500
```

### 4b — Changed file contents

For each changed file (up to 15 files), read the **full current content** of the changed
regions. Use `sqmd_get` or direct file reads to get line-numbered content.

For each file, identify which lines changed using the diff hunk headers (`@@` lines).

### 4c — Dependency blast radius

For each changed file that imports/exports symbols (`.rs`, `.ts`, `.tsx`, `.js`, `.py`, `.go`):

```
sqmd_deps(path=<file_path>, depth=1)
```

Focus on **dependents** (files that import FROM the changed file). For each dependent,
use `sqmd_get` to read the relevant import lines. If a changed function's signature shifts,
check whether callers pass the right arguments.

### 4d — Structural search

Use sqmd's FTS and entity graph tools (NOT embedding-based search):

```
# Find relevant chunks by symbol name
sqmd_search(query="<primary exported struct/function name>", file_filter="<file_path>", top_k=5)

# List chunks in a file
sqmd_ls(file_filter="<file_path>", type_filter="function")

# Get a specific chunk by file + line
sqmd_get(file_path="<path>", line=<number>)
```

**Do not use** `sqmd_context`, `sqmd_embed`, or `sqmd_embed_start` — these depend on
vector embeddings which are not needed for structural code review.

### 4e — Context assembly

Build the final context block:

```markdown
## Review Context

### Metadata
- Files changed: <comma-separated list>
- Commit range: <base>...<head>
- PR: #<number> — <title> (if PR-aware)

### Repository Conventions
<contents of AGENTS.md/CLAUDE.md if found>

### Prior Review History
<prior bot comments with status: [dismissed], [active], [likely addressed]>
<human replies that dismissed or rejected findings>
**IMPORTANT: Findings marked [dismissed by human] or [rejected with rationale]
MUST NOT be re-flagged unless new evidence exists in the current diff.**

### Diff
```diff
<diff, truncated to 500 lines>
```

### Changed File Contents
For each changed file (up to 15 files):
#### <file_path>
```<line numbered content of changed regions only>```

### Dependency Impact
For each dependent file:
#### <dependent_file_path>
<relevant import/usage lines with line numbers>
<note if signature change may break this caller>

### sqmd Structural Context
<chunks from sqmd_search and sqmd_get>
```

## Step 5 — Run Structured Review

Use the assembled context with the review prompt below. Output MUST be structured
JSON in a fenced block tagged `pr-review-json`.

### Review Prompt

```
Review this pull request. Be discerning. Use good judgment.

Focus on: bugs, security flaws, data corruption, race conditions, logic mistakes,
breaking changes, and patterns that diverge from codebase conventions.
Do not flag style-only issues unless they conceal a correctness problem.

You are a local review tool (sqmd-review). State this in the first line of your summary.

Instructions:
1. Read the PR description and commit messages. Validate whether the implementation
   achieves what the description claims. If the PR says it does X, verify the code does X.
   If the implementation diverges from stated goals, flag this.
2. Check for introduced security vulnerabilities, injection risks, and attack-surface expansion.
3. Check adherence to repository conventions (AGENTS.md, CLAUDE.md, etc.) if present.
   Flag deviations only when they affect correctness, consistency, or maintainability.
4. Cross-reference changes against dependency impact. If a changed function's signature
   or behavior shifts, verify callers are updated or that the change is backward-compatible.
5. Do not turn adjacent architecture preferences into blockers. A blocker must be grounded
   in concrete evidence from the changed code or sqmd context and must directly affect
   correctness, security, or data integrity.

PRIOR REVIEW AWARENESS:
6. Prior review comments are provided in the context. Items marked [dismissed by human],
   [rejected with rationale], [out of scope for this pr], or [likely addressed] MUST NOT be
   re-flagged unless the new diff adds materially new evidence.
7. If you re-raise a prior concern, include a short evidence_note naming the new lines
   or changed path that justify reopening it.
8. If a human said a concern is intentional, acceptable, or out of scope, respect that and
   do not press the same angle again.

ANTI-HALLUCINATION RULES (critical — violations produce false positives that waste hours):

9. CODE CITATION REQUIREMENT: Every finding MUST quote the exact code it references,
   verbatim from the diff or file contents. Do NOT paraphrase or reconstruct code from
   memory. If you cannot quote the exact line, you do not have enough evidence for a finding.
   Example of VIOLATION: "The SQL is `WHERE agent_id = ? AND id != ?`" when the actual
   code is `WHERE id != ? AND agent_id = ?`.
   Example of COMPLIANCE: "Line 97 reads `WHERE id != ? AND agent_id = ?` but line 101
   binds `.all(agentId, excludeId, limit)` — the first ? gets agentId, not excludeId."

10. EVIDENCE OVER INFERENCE: If a finding requires assuming code exists outside the diff
    that you have not read, it is at most `warning` severity — never `blocking`. Blocking
    findings must be provable from the diff + included sqmd context alone.

11. NO FLIP-FLOPPING: If this review is addressing a prior review's suggestion, acknowledge
    the prior finding explicitly. Do not raise the opposite concern without citing new evidence
    that was not available in the prior round. "Actually the other order is safer" is not new
    evidence — it is a reversal without justification.

12. DIFF-ONLY SCOPE: Only flag issues in changed code or in files directly affected by the
    changes (blast radius from dependency analysis). Do not flag pre-existing issues in
    adjacent code that the diff did not touch. Pre-existing issues are out of scope unless
    the change makes them worse.

13. CONVENTION CHECKLIST: Systematically check these common patterns before outputting:
    - `as` casts: flag unless unavoidable (prefer type guards, narrowing)
    - Non-null assertions (`!`): flag
    - Rate limiting on admin/mutation endpoints
    - Agent ID scoping on data queries
    - Absolute paths in API responses (path disclosure)
    - Line length violations (>120 chars)
    - `console.log`/`console.error` instead of structured logger
    - Missing error handling on I/O operations (writes, network, DB)
    - Duplicate imports (same symbol imported twice)
    - Unused imports after refactoring
    If all checks pass, note "convention checklist passed" in the summary.

Do not approve or state the code is safe. Your role is to flag issues or signal readiness
for human review.

Output a JSON object in a fenced block tagged exactly `pr-review-json`.
```

See [references/review-schema.md](references/review-schema.md) for the full output schema.

## Step 6 — Fix Findings and Re-review

This is the critical loop that separates sqmd-review from a one-shot linter:

1. Present findings grouped by severity
2. Fix all blocking and warning findings
3. Re-run the full review workflow (steps 1-5) against the updated diff
4. If new findings appear, fix them and repeat
5. Continue until verdict is `no_issues` with high confidence
6. Only then commit and push

**Rules for the iteration loop:**
- Each iteration must re-read the full diff, not just the new changes
- Each iteration must re-check dependency blast radius for any files that changed
- Each iteration must verify prior findings are still addressed (not re-introduced)
- Each iteration must run the convention checklist from scratch
- Log the iteration number so the user can see progress

## Step 7 — Push

After the verdict is `no_issues`:
1. Run tests
2. Run lint
3. Update PR body with review round summary
4. Commit and push

## Resources

- [references/review-schema.md](references/review-schema.md) — structured JSON output schema with examples and severity guidelines
