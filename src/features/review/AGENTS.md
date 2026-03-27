# Review Feature

**Main review workspace** — the core UI for reviewing PR findings.

## COMPONENTS

| Component         | File                | Purpose                                       |
| ----------------- | ------------------- | --------------------------------------------- |
| `ReviewWorkspace` | ReviewWorkspace.tsx | Top-level orchestrator, state, event listener |
| `FileTree`        | FileTree.tsx        | Changed files sidebar                         |
| `SignalBoard`     | SignalBoard.tsx     | Findings list with filtering                  |
| `DiffPanel`       | DiffPanel.tsx       | Diff viewer with line highlighting            |
| `FindingCard`     | FindingCard.tsx     | Individual finding display                    |

## STATE FLOW

```
ReviewContext.Provider (ReviewWorkspace)
  ├── FileTree → selectedFile
  ├── SignalBoard → findings[] → activePanel="signals"
  └── DiffPanel → selectedFile → activePanel="diff"
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
