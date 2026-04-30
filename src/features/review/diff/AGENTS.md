# Review Diff Subsystem

## OVERVIEW

**Single-purpose diff renderer domain** for the review workspace.
Parses PR unified diff text, maps SignalPR findings into `@pierre/diffs` annotations, and keeps a safe legacy rendering fallback path.

## STRUCTURE

```
src/features/review/diff/
├── parsePierrePatch.ts           # parse `diff_text` into `FileDiffMetadata`
├── normalizeFilePath.ts          # strip `a/` and `b/` only when safe
├── mapFindingsToLineAnnotations.ts  # Finding -> `DiffLineAnnotation` adapter
├── FindingAnnotation.tsx         # Clickable line badge component
├── perfHeuristics.ts            # Diff statistics + collapse threshold logic
├── PierreDiffPanel.tsx           # Primary renderer with annotations + heuristics
├── LegacyDiffPanel.tsx           # Plain-text renderer used as fallback
├── DiffPanel.tsx                 # Runtime wrapper with `ErrorBoundary`
└── __fixtures__/                 # deterministic parser + integration fixtures
```

## WHERE TO LOOK

| Item | File | Purpose |
| ---- | ---- | ------- |
| Parser | `parsePierrePatch.ts` | Converts `diff_text` into flat `FileDiffMetadata[]` |
| Annotation map | `mapFindingsToLineAnnotations.ts` | Cluster-aware, severity-aware finding aggregation |
| Renderer | `PierreDiffPanel.tsx` | Builds file-level + line-level annotations and renders `FileDiff` |
| Safety | `LegacyDiffPanel.tsx` / `DiffPanel.tsx` | Fallback path on parse/render failure |
| Heuristics | `perfHeuristics.ts` | `shouldCollapseByDefault` (large diff guardrails) |
| Fixtures | `__fixtures__/sample-diff.txt`, `__fixtures__/large-diff.txt` | Regression and perf edge-case inputs |
| End-to-end checks | `integration.test.ts`, `parsePierrePatch.test.ts`, `mapFindingsToLineAnnotations.test.ts`, `perfHeuristics.test.ts` | Behavioral contract validation |

## CONVENTIONS

- Keep parsing adapters pure and side-effect free.
- Do not emit annotations for suppressed findings or for cluster non-representatives.
- Prefer `diff_new_line` → `line_start` for line anchoring.
- Use `normalizeFilePath` when matching `a/`/`b/` file paths from patches.
- File-level findings (no anchor) are surfaced in header metadata, not line annotations.
- Keep `renderAnnotation` side effects minimal; it should only surface `FindingAnnotation`.
- Collapse non-selected files only when large thresholds trigger (`FILE_COUNT_THRESHOLD` / `LINE_COUNT_THRESHOLD`).

## ANTI-PATTERNS

- Don't mix `prevName` and `name` matching without deduplicating/combining annotation inputs.
- Don't rely on raw `parse` exceptions for control flow in UI render paths.
- Don't show unbounded diffs without collapse heuristics for large multi-file patches.
- Don't bypass `DiffPanel` boundaries when introducing new diff renderer variants.
