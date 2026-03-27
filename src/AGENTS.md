# Frontend Source

**React/TypeScript frontend** — feature-based structure with IPC bridge.

## ENTRY

- `main.tsx` — React DOM render
- `App.tsx` — Router (`/` → IntakeView, `/review/:runId` → ReviewWorkspace)

## LIB MODULES

| File       | Purpose                                                        |
| ---------- | -------------------------------------------------------------- |
| `ipc.ts`   | Tauri `invoke()` wrappers (ALWAYS use these, never raw invoke) |
| `store.ts` | `ReviewContext` for workspace state                            |
| `types.ts` | Shared TypeScript interfaces                                   |

## FEATURES

| Feature       | Components                                                     | Purpose                            |
| ------------- | -------------------------------------------------------------- | ---------------------------------- |
| `intake/`     | IntakeView                                                     | PR URL input + workspace selection |
| `onboarding/` | EnvironmentCheck                                               | Verify gh/codex CLI                |
| `review/`     | ReviewWorkspace, FileTree, SignalBoard, DiffPanel, FindingCard | Main review UI                     |
| `submission/` | SubmitDialog                                                   | Submit review to GitHub            |

## CONVENTIONS

- Dark theme (bg-zinc-950, text-zinc-100) — no light mode
- Tailwind CSS 4 for styling
- `lucide-react` for icons
- Never call `invoke()` directly — use `ipc.ts`
