# Review Output Schema

Output MUST be a JSON object in a fenced block tagged `pr-review-json`.

## Schema

```jsonc
{
  "summary": "string — first line must identify this as an sqmd-review",
  "verdict": "no_issues | comment | request_changes",
  "confidence": {
    "level": "high | medium | low",
    "reasons": [
      // one or more:
      "sufficient_diff_evidence",
      "targeted_context_included",
      "missing_runtime_repro",
      "missing_cross_module_context",
      "ambiguous_requirements"
    ],
    "justification": "string — concrete evidence, NOT boilerplate. Name specific files, lines, or missing artifacts. Never use 'full repository context is unavailable' unless you name a specific missing artifact."
  },
  "convention_checklist": {
    "no_as_casts": true,
    "no_non_null_assertions": true,
    "rate_limiting_on_mutations": true,
    "agent_id_scoping": true,
    "no_path_disclosure": true,
    "line_length_ok": true,
    "structured_logging": true,
    "io_error_handling": true,
    "no_duplicate_imports": true,
    "no_unused_imports": true,
    "notes": "string — any checklist items that failed, with file:line"
  },
  "prior_review_status": {
    "total_prior_findings": 42,
    "re_raised": 0,
    "suppressed_as_dismissed": 12,
    "suppressed_as_addressed": 3,
    "notes": "string — summary of prior review handling"
  },
  "comments": [
    {
      "file": "string — file path relative to repo root",
      "line": 42,
      "body": "string — the finding description",
      "code_quote": "string — VERBATIM quote of the exact code this finding references (2-3 lines of context)",
      "evidence_note": "string (optional) — required when re-raising a previously rebutted concern. Must cite NEW evidence not available in prior rounds.",
      "severity": "blocking | warning | nitpick",
      "hunk": "string (optional) — the diff hunk this finding references, for precision"
    }
  ]
}
```

## Verdict Guidelines

| Verdict | Meaning |
|---------|---------|
| `no_issues` | Nothing worth flagging, ready for human review |
| `comment` | Found issues worth discussing but not blocking push |
| `request_changes` | Found issues that should be fixed before pushing |

## Severity Guidelines

### `blocking`
Must be grounded in concrete evidence. Use when:
- Security vulnerability (injection, auth bypass, data leak)
- Data corruption risk (race condition, missing transaction, unsafe mutation)
- Logic error that breaks stated behavior
- Breaking API/contract change without migration
- Missing null/undefined/error handling on a critical path

**Anti-hallucination guard**: Escalate to `blocking` ONLY if:
1. The finding is directly provable from the diff + sqmd context
2. You have quoted the exact code (verbatim) that demonstrates the issue
3. The code quote matches what appears in the diff or file contents

If confidence is `low` or `medium` due to `missing_cross_module_context`, ALWAYS downgrade to `warning`.
If you cannot quote the exact code, the finding is at most `warning`.

### `warning`
Use when:
- Logic concern but not provably wrong from available context
- Convention drift that affects maintainability
- Dependency impact: caller may need updating
- Error handling gap on a non-critical path
- Potential performance regression
- Finding requires code outside the diff that has not been read

### `nitpick`
Use when:
- Style or naming inconsistency (only if it conceals a correctness issue)
- Minor documentation gap
- Small refactor opportunity
- Comment placement or readability suggestion

## Confidence Levels

| Level | When to Use |
|-------|------------|
| `high` | Finding is directly provable from diff + sqmd context. Named specific files, lines, and evidence. Verbatim code quote included. |
| `medium` | Finding is supported by evidence but requires context not in scope (e.g., cross-module type contract). Code quote included. |
| `low` | Finding is a reasonable concern but cannot be verified without runtime or broader codebase context. |

## Prior Review Status

When prior bot review comments are available (PR-aware mode), the output MUST include
`prior_review_status` tracking how prior findings were handled:

- `suppressed_as_dismissed`: prior findings that a human dismissed or rejected — these
  must NOT appear in the `comments` array unless new evidence exists
- `suppressed_as_addressed`: prior findings where the code at the referenced location
  has changed and the issue appears resolved
- `re_raised`: prior findings that are still active (code unchanged and issue persists)
  — these should appear in `comments` only if they remain valid against the current diff

## Code Quote Rules

Every finding MUST include a `code_quote` field with the verbatim code being referenced.
This is the single most important anti-hallucination measure.

- Quote from the diff hunk or file contents — not from memory
- Include enough context (2-3 lines) to make the issue visible in the quote
- If the quote doesn't match the actual file content, the finding is invalid
- If you can't produce a quote (e.g., the code is "somewhere else"), the finding is at most `warning`

## Flip-Flop Prevention

When re-reviewing code that was changed to address a prior review finding:
- Acknowledge the prior finding in the `body` or `evidence_note`
- Explain specifically what NEW evidence justifies the new concern
- "The opposite approach is actually safer" is NOT new evidence
- If you're reversing yourself, your severity must be `warning` or lower, never `blocking`
- Prior findings marked `[dismissed by human]` are permanently closed for this PR

## Dismissal Signals

The following patterns in human replies indicate a finding should be suppressed:

| Signal | Meaning | Action |
|--------|---------|--------|
| "intentional" / "by design" | Human confirms the code is correct as-is | Suppress permanently |
| "out of scope" / "separate PR" | Not relevant to this change | Suppress permanently |
| "already handled" / "covered by" | Addressed elsewhere | Suppress permanently |
| "will fix" / "on it" | Acknowledged but not yet fixed | Keep as `warning`, not `blocking` |
| No reply (finding is active) | Unaddressed | Re-check against current diff |

## Example

````markdown
```pr-review-json
{
  "summary": "[sqmd-review] Found one potential race condition and one missing error handler.",
  "verdict": "comment",
  "confidence": {
    "level": "medium",
    "reasons": ["sufficient_diff_evidence", "missing_cross_module_context"],
    "justification": "The race condition in worker.rs:198 is directly visible in the diff — two async tasks read/write shared state without a lock. The missing error handler in pipeline.ts:41 is provable from the diff. Confidence is medium because WorkerStats type definition (not in diff) is needed to confirm whether the shared state access is safe via TypeScript structural typing."
  },
  "convention_checklist": {
    "no_as_casts": false,
    "no_non_null_assertions": true,
    "rate_limiting_on_mutations": true,
    "agent_id_scoping": true,
    "no_path_disclosure": true,
    "line_length_ok": true,
    "structured_logging": true,
    "io_error_handling": false,
    "no_duplicate_imports": true,
    "no_unused_imports": true,
    "notes": "as cast at pipeline.ts:89, missing try-catch on writeFileSync at pipeline.ts:41"
  },
  "prior_review_status": {
    "total_prior_findings": 8,
    "re_raised": 0,
    "suppressed_as_dismissed": 5,
    "suppressed_as_addressed": 3,
    "notes": "5 prior findings dismissed by human in review rounds 1-3, 3 addressed in subsequent commits"
  },
  "comments": [
    {
      "file": "src/worker.rs",
      "line": 198,
      "body": "Two async tasks access `shared_state` concurrently without synchronization.",
      "code_quote": "let state = shared_state.clone();\ntokio::spawn(async move {\n    state.update(data); // write\n});",
      "severity": "warning"
    },
    {
      "file": "src/pipeline.ts",
      "line": 41,
      "body": "The `process()` call is not awaited and has no `.catch()`. A rejection will surface as unhandled.",
      "code_quote": "process(result); // no await, no .catch()",
      "severity": "warning"
    }
  ]
}
```
````

## Fallback Behavior

If structured JSON cannot be produced, fall back to a plain-text summary with findings
grouped by severity. Never return empty output.
