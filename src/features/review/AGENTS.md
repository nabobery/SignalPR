# Review Feature

**Main review workspace** — the core UI for reviewing PR findings.

## COMPONENTS

| Component           | File                  | Purpose                                           |
| ------------------- | --------------------- | ------------------------------------------------- |
| `ReviewWorkspace`   | ReviewWorkspace.tsx   | Top-level orchestrator, state, event listener     |
| `FileTree`          | FileTree.tsx          | Changed files sidebar                             |
| `SignalBoard`       | SignalBoard.tsx       | Findings list with filtering                      |
| `DiffPanel`         | DiffPanel.tsx         | Diff viewer with line highlighting                |
| `FindingCard`       | FindingCard.tsx       | Individual finding display                        |
| `ClusterCard`       | ClusterCard.tsx       | Grouped findings with expand/collapse             |
| `LaneProgress`      | LaneProgress.tsx      | Multi-lane status indicators (security/arch/perf) |
| `StreamingActivity` | StreamingActivity.tsx | Real-time streaming output per lane               |
| `ApprovalModal`     | ApprovalModal.tsx     | Interactive Codex tool approval queue             |
| `FixPreview`        | FixPreview.tsx        | Auto-fix preview modal                            |
| `FixBatchBar`       | FixBatchBar.tsx       | Batch fix actions bar                             |

## STATE FLOW

```
ReviewContext.Provider (ReviewWorkspace)
  ├── FileTree → selectedFile
  ├── SignalBoard → findings[] → activePanel="signals"
  │     └── ClusterCard / FindingCard
  ├── DiffPanel → selectedFile → activePanel="diff"
  ├── LaneProgress → lanes[] (LaneSnapshot[])
  │     └── StreamingActivity (per lane)
  ├── ApprovalModal → codex_approval_requested queue
  └── FixPreview / FixBatchBar → auto-fix workflow
```

## EVENTS

- `review_progress` — Backend pipeline progress
- `codex_lane_delta` — Real-time streaming output per lane (filtered by `laneId`)
- `codex_approval_requested` — Interactive tool approval queue (Ack/Decline/Cancel)

## STREAMING (`StreamingActivity.tsx`)

Shows last meaningful line from Codex streaming buffer:

- Listens to `codex_lane_delta` event, filtered by `laneId`
- Debounces 100ms to avoid flicker
- Truncates at 120 chars with ellipsis
- Icon: `Activity` from lucide-react

## APPROVAL (`ApprovalModal.tsx`)

Modal queue for Codex interactive approval requests:

- Queue-based: multiple approvals stack
- Shows method, command, cwd, thread/turn IDs
- Actions: Accept, Decline, Cancel turn
- Calls `resolveCodexApproval(requestId, decision)` IPC

## CONVENTIONS

- `Panel` type: `"signals" | "diff"`
- Submit button shows active finding count
- Loading/error states render full-screen
- Status indicators in header (running, failed, submitted)
- ClusterCard suppresses entire cluster by suppressing representative finding
- LaneProgress shows per-lane icons: Shield (security), Layers (arch), Gauge (perf)
- FixPreview/FixBatchBar handle auto-fix suggestions via `apply_fix` IPC
- ApprovalModal is globally rendered (fixed inset-0 z-50)
