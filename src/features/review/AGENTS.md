# Review Feature

**Main review workspace** — the core UI for reviewing PR findings.

## COMPONENTS

| Component         | File                | Purpose                                           |
| ----------------- | ------------------- | ------------------------------------------------- |
| `ReviewWorkspace` | ReviewWorkspace.tsx | Top-level orchestrator, state, event listener     |
| `FileTree`        | FileTree.tsx        | Changed files sidebar                             |
| `SignalBoard`     | SignalBoard.tsx     | Findings list with filtering                      |
| `DiffPanel`       | DiffPanel.tsx       | Diff viewer with line highlighting                |
| `FindingCard`     | FindingCard.tsx     | Individual finding display                        |
| `ClusterCard`     | ClusterCard.tsx     | Grouped findings with expand/collapse             |
| `LaneProgress`    | LaneProgress.tsx    | Multi-lane status indicators (security/arch/perf) |
| `FixPreview`      | FixPreview.tsx      | Auto-fix preview modal                            |
| `FixBatchBar`     | FixBatchBar.tsx     | Batch fix actions bar                             |

## STATE FLOW

```
ReviewContext.Provider (ReviewWorkspace)
  ├── FileTree → selectedFile
  ├── SignalBoard → findings[] → activePanel="signals"
  │     └── ClusterCard / FindingCard
  ├── DiffPanel → selectedFile → activePanel="diff"
  ├── LaneProgress → lanes[] (LaneSnapshot[])
  └── FixPreview / FixBatchBar → auto-fix workflow
```

## EVENTS

- Listens to `review_progress` event from backend
- Calls `refreshSnapshot()` on each event
- Polls `getReviewSnapshot()` for current state

## CONVENTIONS

- `Panel` type: `"signals" | "diff"`
- Submit button shows active finding count
- Loading/error states render full-screen
- Status indicators in header (running, failed, submitted)
- ClusterCard suppresses entire cluster by suppressing representative finding
- LaneProgress shows per-lane icons: Shield (security), Layers (arch), Gauge (perf)
- FixPreview/FixBatchBar handle auto-fix suggestions via `apply_fix` IPC
