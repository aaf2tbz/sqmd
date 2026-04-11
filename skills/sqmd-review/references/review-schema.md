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
    "justification": "string — concrete evidence, NOT boilerplate. Name specific files, lines, or missing artifacts."
  },
  "ui_screenshot_needed": false,
  "comments": [
    {
      "file": "string — file path relative to repo root",
      "line": 42,
      "body": "string — the finding description",
      "evidence_note": "string (optional) — required when re-raising a previously rebutted concern",
      "severity": "blocking | warning | nitpick"
    }
  ]
}
```

## Verdict Guidelines

| Verdict | Meaning |
|---------|---------|
| `no_issues` | Nothing worth flagging, ready for human review |
| `comment` | Found issues worth discussing but not blocking merge |
| `request_changes` | Found issues that should be fixed before pushing |

## Severity Guidelines

### `blocking`
Must be grounded in concrete evidence. Use when:
- Security vulnerability (injection, auth bypass, data leak)
- Data corruption risk (race condition, missing transaction, unsafe mutation)
- Logic error that breaks stated behavior
- Breaking API/contract change without migration
- Missing null/undefined/error handling on a critical path

Escalate to `blocking` only if the issue is directly provable from the diff or sqmd context. If confidence is `low` or `medium` due to `missing_cross_module_context`, downgrade to `warning`.

### `warning`
Use when:
- Logic concern but not provably wrong from available context
- Convention drift that affects maintainability
- Dependency impact: caller may need updating
- Error handling gap on a non-critical path
- Potential performance regression

### `nitpick`
Use when:
- Style or naming inconsistency (only if it conceals a correctness issue)
- Minor documentation gap
- Small refactor opportunity

## Confidence Levels

| Level | When to Use |
|-------|------------|
| `high` | Finding is directly provable from diff + sqmd context. Named specific files, lines, and evidence. |
| `medium` | Finding is supported by evidence but requires context not in scope (e.g., cross-module type contract). |
| `low` | Finding is a reasonable concern but cannot be verified without runtime or broader codebase context. |

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
  "ui_screenshot_needed": false,
  "comments": [
    {
      "file": "src/worker.rs",
      "line": 198,
      "body": "Two async tasks access `shared_state` concurrently without synchronization. If `task_a` writes while `task_b` reads, stale or partial data is possible. Consider a Mutex or RwLock.",
      "severity": "warning"
    },
    {
      "file": "src/pipeline.ts",
      "line": 41,
      "body": "The `process()` call is not awaited and has no `.catch()`. A rejection here will surface as an unhandled promise rejection at runtime.",
      "severity": "warning"
    }
  ]
}
```
````

## Fallback Behavior

If structured JSON cannot be produced, fall back to a plain-text summary. Never return empty output.
