---
name: sqmd-review
description: >
  Local self-review for uncommitted or staged changes using sqmd-indexed codebase context.
  Like pr-reviewer but runs offline before pushing — no GitHub required.
  Use when the user asks to "review my changes", "self-review", "sqmd review", "review before push",
  "check my diff", "pre-push review", "review this commit", or "review this branch".
  Triggers on any request to review code changes locally using sqmd for codebase-aware context.
---

# sqmd-review

Self-contained code review using sqmd-indexed codebase context. Adapted from [pr-reviewer](https://github.com/NicholaiVogel/pr-reviewer).

## Workflow

1. Detect scope: staged changes, uncommitted diff, or branch diff against base
2. Gather context with sqmd (search, context assembly, dependency graphs)
3. Run structured review against the assembled context
4. Output structured JSON with findings

## Step 1 — Detect Scope

Determine what to review:

- **Staged changes** (default): `git diff --cached`
- **Uncommitted diff**: `git diff` (use when user says "review my changes" with no staging)
- **Branch diff**: `git diff <base>...HEAD` (use when user says "review this branch" or "review before push")
- **Single commit**: `git show <sha>`

Extract changed file paths and line ranges from the diff.

## Step 2 — Gather Context with sqmd

For each changed file, use sqmd tools to build codebase-aware context:

### Dependency context

For each changed file with extensions `.rs`, `.ts`, `.tsx`, `.js`, `.py`, `.go`:

```
sqmd_deps(path=<file_path>, depth=1)
```

Include files that depend on the changed file (dependents) — these are the blast radius.

### Semantic search context

For each changed file, search for relevant symbols and patterns:

```
sqmd_search(query="<primary module or struct name from file>", file_filter="<file_path>", top_k=5)
```

```
sqmd_context(query="<what the change does, inferred from diff>", top_k=10)
```

### Full context assembly

Build a context block in this structure:

```markdown
## Review Context

### PR Metadata
- Files changed: <list>
- Commit range: <range>

### Diff
```diff
<parsed diff, truncated to 300 lines>
```

### Changed File Contents
For each changed file (up to 15 files, 200 lines each):
<file content with line numbers>

### Dependency Impact
For each changed file's dependents:
<file path>: <snippet of how it depends on changed code>

### sqmd Search Results
<ranked chunks relevant to the changes>
```

## Step 3 — Run Structured Review

Use the assembled context with the review prompt below. Output MUST be structured JSON in a fenced block tagged `pr-review-json`.

### Review Prompt

```
Review this pull request. Be discerning. Use good judgment.

Focus on: bugs, security flaws, data corruption, race conditions, logic mistakes,
breaking changes, and patterns that diverge from codebase conventions.
Do not flag style-only issues unless they conceal a correctness problem.

You are a local review tool (sqmd-review). State this in the first line of your summary.

Instructions:
1. Read the diff. Validate whether the implementation achieves what the commit messages
   or branch description claim. If the diff says it does X, verify the code does X.
   If the implementation diverges from stated goals, flag this.
2. Check for introduced security vulnerabilities, injection risks, and attack-surface expansion.
3. Check adherence to repository conventions (AGENTS.md, CLAUDE.md, etc.) if present.
   Flag deviations only when they affect correctness, consistency, or maintainability.
4. Cross-reference changes against dependency impact. If a changed function's signature
   or behavior shifts, verify callers are updated or that the change is backward-compatible.
5. Do not turn adjacent architecture preferences into blockers. A blocker must be grounded
   in concrete evidence from the changed code or sqmd context and must directly affect
   correctness, security, or data integrity.

ANTI-HALLUCINATION RULES (critical — violations produce false positives that waste hours):

6. CODE CITATION REQUIREMENT: Every finding MUST quote the exact code it references,
   verbatim from the diff or file contents. Do NOT paraphrase or reconstruct code from
   memory. If you cannot quote the exact line, you do not have enough evidence for a finding.
   Example of VIOLATION: "The SQL is `WHERE agent_id = ? AND id != ?`" when the actual
   code is `WHERE id != ? AND agent_id = ?`.
   Example of COMPLIANCE: "Line 97 reads `WHERE id != ? AND agent_id = ?` but line 101
   binds `.all(agentId, excludeId, limit)` — the first ? gets agentId, not excludeId."

7. EVIDENCE OVER INFERENCE: If a finding requires assuming code exists outside the diff
   that you have not read, it is at most `warning` severity — never `blocking`. Blocking
   findings must be provable from the diff + included sqmd context alone.

8. NO FLIP-FLOPPING: If this review is addressing a prior review's suggestion, acknowledge
   the prior finding explicitly. Do not raise the opposite concern without citing new evidence
   that was not available in the prior round. "Actually the other order is safer" is not new
   evidence — it is a reversal without justification.

9. DIFF-ONLY SCOPE: Only flag issues in changed code or in files directly affected by the
   changes (blast radius from dependency analysis). Do not flag pre-existing issues in
   adjacent code that the diff did not touch. Pre-existing issues are out of scope unless
   the change makes them worse.

10. CONVENTION CHECKLIST: Systematically check these common patterns before outputting:
    - `as` casts: flag unless unavoidable (prefer type guards, narrowing)
    - Non-null assertions (`!`): flag
    - Rate limiting on admin/mutation endpoints
    - Agent ID scoping on data queries
    - Absolute paths in API responses (path disclosure)
    - Line length violations (>120 chars)
    - `console.log`/`console.error` instead of structured logger
    - Missing error handling on I/O operations (writes, network, DB)
    If all checks pass, note "convention checklist passed" in the summary.

Do not approve or state the code is safe. Your role is to flag issues or signal readiness
for human review.

Output a JSON object in a fenced block tagged exactly `pr-review-json`.
```

See [references/review-schema.md](references/review-schema.md) for the full output schema.

## Step 4 — Present Findings

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

## Resources

- [references/review-schema.md](references/review-schema.md) — structured JSON output schema with examples and severity guidelines
