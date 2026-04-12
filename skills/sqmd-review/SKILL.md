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

The goal is **zero-surprise pushes**: every issue the remote bot would flag should be
caught here first. This means:

- **Full codebase context** via sqmd, not just the diff
- **Prior review awareness** — read dismissed/rebutted bot comments so we don't re-flag them
- **Blast-radius analysis** — dependency graphs show what breaks when signatures change
- **Hunk-level precision** — findings reference exact changed lines, not whole files
- **Anti-hallucination by construction** — every finding must quote verbatim code from
  context the agent actually read

## Workflow

```
1. Detect scope → 2. Ensure sqmd index → 3. Gather git context → 4. Assemble review context
→ 5. Run structured review → 6. Present findings
```

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
# Check if index exists and is recent
sqmd_stats
# If empty or stale (>1 hour since last index), re-index changed files
sqmd_index_file  # index changed files only (fast incremental)
```

If the index is completely empty, run `sqmd embed` first. The review CANNOT proceed
without an indexed codebase — sqmd context is the whole point.

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

### 4d — Semantic search

For each changed file, search sqmd for relevant context:

```
# Symbol-level context for the file
sqmd_search(query="<primary exported struct/function name>", file_filter="<file_path>", top_k=5)

# Broader context for what the change does
sqmd_context(query="<description of the change, inferred from diff + commit message>", top_k=10)
```

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

### sqmd Context
<ranked chunks from sqmd_search and sqmd_context>
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

## Step 6 — Present Findings

Parse the structured JSON output and present:

1. **Summary** — one paragraph overview
2. **Verdict** — `no_issues`, `comment`, or `request_changes`
3. **Confidence** — level with justification
4. **Findings** — grouped by severity:
   - **Blocking**: must fix before push (bugs, security, data integrity)
   - **Warning**: should discuss (logic concerns, convention drift)
   - **Nitpick**: minor observations

For each finding, show file, line number, and the issue. Every finding MUST include
a verbatim quote of the code it references. If sqmd context was used to identify the
issue, note what search/context supported the finding.

### Post-review action

If the verdict is `request_changes`, list the specific fixes needed before pushing.
If the verdict is `no_issues`, confirm the code is ready for push but note that human
review is still recommended.

## Resources

- [references/review-schema.md](references/review-schema.md) — structured JSON output schema with examples and severity guidelines
