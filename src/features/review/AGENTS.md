# Review Feature

**Main review workspace** — the core UI for reviewing PR findings.

## COMPONENTS

| Component           | File                  | Purpose                                           |
| ------------------- | --------------------- | ------------------------------------------------- |
| `ReviewWorkspace`   | ReviewWorkspace.tsx   | Top-level orchestrator, state, event listener     |
| `FileTree`          | FileTree.tsx          | Changed files sidebar                             |
| `SignalBoard`       | SignalBoard.tsx       | Findings list with filtering                      |
| `DiffPanel`         | DiffPanel.tsx         | Diff viewer + ErrorBoundary wrapper for Pierre/legacy fallback |
| `FindingCard`       | FindingCard.tsx       | Individual finding display                        |
| `ClusterCard`       | ClusterCard.tsx       | Grouped findings with expand/collapse             |
| `LaneProgress`      | LaneProgress.tsx      | Multi-lane status indicators (security/arch/perf) |
| `StreamingActivity` | StreamingActivity.tsx | Real-time streaming output per lane               |
| `ApprovalModal`     | ApprovalModal.tsx     | Interactive Codex tool approval queue             |
| `FixPreview`        | FixPreview.tsx        | Auto-fix preview modal                            |
| `FixBatchBar`       | FixBatchBar.tsx       | Batch fix actions bar                             |
| `SessionDrawer`     | SessionDrawer.tsx     | Session progress + errors + warnings                |
| `diff/*`            | `src/features/review/diff/*` | Diff parser, line annotations, heuristics, fixtures |

## STATE FLOW

```
ReviewContext.Provider (ReviewWorkspace)
  ├── FileTree → selectedFile
  ├── SignalBoard → findings[] → activePanel="signals"
  │     └── ClusterCard / FindingCard
  │           └── scrollIntoView when focusedFindingId is set
  ├── DiffPanel → selectedFile → activePanel="diff"
  │     └── PierreDiffPanel + LegacyDiffPanel fallback
  ├── LaneProgress → lanes[] (LaneSnapshot[])
  │     └── StreamingActivity (per lane)
  ├── ApprovalModal → codex_approval_requested queue
  └── FixPreview / FixBatchBar → auto-fix workflow
```

## EVENTS

Backend emits `review_progress`, lane deltas (`<provider>_lane_delta`), and permission queue events (`<provider>_permission_requested`).

## STREAMING (`StreamingActivity.tsx`)

Shows last meaningful line from streaming buffer per lane:

- Listens to `codex_lane_delta`, `copilot_lane_delta`, `opencode_lane_delta`, `gemini_lane_delta`, `cursor_lane_delta`, `pi_lane_delta` — all filtered by `laneId`
- Debounces 100ms to avoid flicker
- Truncates at 120 chars with ellipsis
- Icon: `Activity` from lucide-react

## APPROVAL (`ApprovalModal.tsx`)

Modal queue is provider-agnostic.
- Codex / Copilot / OpenCode resolve via their provider-specific IPC actions.
- Gemini / Cursor are observational-only; requests resolve to dismiss-only UI.

## TESTING

Focus coverage:
- Review event handling, lane streaming, and permission queue UI.
- Diff annotation click-through path (`focusedFindingId` + `revealFinding`) should stay aligned with panel rendering.

## CONVENTIONS

- `Panel` type: `"signals" | "diff"`
- Submit button shows active finding count
- Loading/error states render full-screen
- Status indicators in header (running, failed, submitted)
- ClusterCard suppresses entire cluster by suppressing representative finding
- LaneProgress shows per-lane icons: Shield (security), Layers (arch), Gauge (perf)
- FixPreview/FixBatchBar handle auto-fix suggestions via `apply_fix` IPC
- ApprovalModal is globally rendered (fixed inset-0 z-50)
- `revealFinding` routes diff annotation clicks to signal focus for 2 seconds
- `focusedFindingId` is part of `ReviewState` and bubbles through `ReviewContext`
- Diff rendering collapses large file sets by default and remains usable via expand behavior
