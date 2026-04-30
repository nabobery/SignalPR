# Frontend Source

**React/TypeScript frontend** — feature-based structure with IPC bridge.

## ENTRY

- `main.tsx` — React DOM render
- `App.tsx` — Router (`/` → IntakeView, `/review/:runId` → ReviewWorkspace, `/settings` → SettingsView)

## LIB MODULES

| File       | Purpose                                                        |
| ---------- | -------------------------------------------------------------- |
| `ipc.ts`   | Tauri `invoke()` wrappers (ALWAYS use these, never raw invoke) |
| `store.ts` | `ReviewContext` for workspace state                            |
| `types.ts` | Shared TypeScript interfaces                                   |

Notable updated state shape:
- `ReviewState` includes `focusedFindingId` for signal card focus
- `ReviewContextType` includes `revealFinding(findingId)` for diff-to-signal navigation

### Key Types (`types.ts`)

Key recurring types: `Finding`, `LaneSnapshot`, `ReviewSnapshot`, `FindingCardType`, `FindingClusterType`, plus provider permission/delta payloads.

### Key IPC Functions (`ipc.ts`)

- Permission resolvers: `resolveCodexApproval`, `resolveCopilotPermission`, `resolveOpenCodePermission`.
- Channel controls: `startChannelListeners`, `stopChannelListeners`, `getChannelStatus`.
- All other backend interactions flow through typed IPC calls in `ipc.ts`.

## FEATURES

| Feature       | Purpose |
| ------------- | ------- |
| `intake/`     | PR input + workspace selection |
| `onboarding/` | Environment checks (`gh`, `codex`) |
| `review/`     | Review flow (findings, diffs, streaming, approvals, fixes) |
| `review/diff/`| `@pierre/diffs` parser/annotation path + fallback renderer |
| `settings/`   | User configuration + provider/channel setup |
| `submission/` | Review submission |

## EVENTS

Frontend listens to `review_progress`, provider lane deltas, and permission request events in `ReviewWorkspace`.

## CONVENTIONS

- Dark theme (bg-zinc-950, text-zinc-100) — no light mode
- Tailwind CSS 4 for styling
- `lucide-react` for icons
- Never call `invoke()` directly — use `ipc.ts`
- Event listeners use `listen<T>("event", handler)` from `@tauri-apps/api/event`
- Always cleanup: `unlisten.then((fn) => fn())` in useEffect return
