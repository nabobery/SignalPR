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

### Key Types (`types.ts`)

| Type                          | Purpose                                                  |
| ----------------------------- | -------------------------------------------------------- |
| `CodexApprovalRequest`        | Codex approval queue item (request_id, method, params)   |
| `CodexLaneDelta`              | Codex streaming delta (lane_id, delta, buffer)           |
| `CopilotPermissionRequest`    | Copilot v3 permission (session_id, event_id, kind, event)|
| `CopilotLaneDelta`            | Copilot streaming delta (lane_id, delta, buffer)         |
| `OpenCodePermissionRequest`   | OpenCode permission (request_id, message)                |
| `OpenCodeLaneDelta`           | OpenCode streaming delta (lane_id, delta, buffer)        |
| `GeminiPermissionRequest`     | Gemini ACP permission (session_id, request_id, tool_call, options) — observational |
| `GeminiLaneDelta`             | Gemini streaming delta (lane_id, delta, buffer)          |
| `CursorPermissionRequest`     | Cursor ACP permission (session_id, request_id, tool_call, options) — observational |
| `CursorLaneDelta`             | Cursor streaming delta (lane_id, delta, buffer)          |
| `Finding`                     | Review finding with severity, location, fix              |
| `LaneSnapshot`                | Per-lane status (security/arch/perf)                     |
| `ReviewSnapshot`              | Full review state with findings + clusters               |
| `ChannelStatus`               | Discord/Slack connection status                          |

### Key IPC Functions (`ipc.ts`)

| Function                         | Purpose                                        |
| -------------------------------- | ---------------------------------------------- |
| `resolveCodexApproval()`         | Approve/decline Codex tool request             |
| `resolveCopilotPermission()`     | Approve/deny Copilot v3 permission request     |
| `resolveOpenCodePermission()`    | Reply to OpenCode permission (once/always/reject) |
| `startChannelListeners()`        | Start background channel polling               |
| `stopChannelListeners()`         | Stop background channel polling                |
| `getChannelStatus()`             | Get Discord/Slack connection status            |

## FEATURES

| Feature       | Components                                                                                                                                           | Purpose                                |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------- |
| `intake/`     | IntakeView                                                                                                                                           | PR URL input + workspace selection     |
| `onboarding/` | EnvironmentCheck                                                                                                                                     | Verify gh/codex CLI                    |
| `review/`     | ReviewWorkspace, FileTree, SignalBoard, DiffPanel, FindingCard, ClusterCard, LaneProgress, StreamingActivity, ApprovalModal, FixPreview, FixBatchBar | Main review UI + streaming + approvals |
| `settings/`   | SettingsView, GeneralPanel, PresetPanel, AgentPanel, ChannelPanel, AgentForm                                                                         | User configuration                     |
| `submission/` | SubmitDialog                                                                                                                                         | Submit review to GitHub                |

## EVENTS

Frontend listens to Tauri events:

| Event                          | Payload                     | Consumer                     |
| ------------------------------ | --------------------------- | ---------------------------- |
| `review_progress`              | Pipeline status             | ReviewWorkspace              |
| `codex_lane_delta`             | `CodexLaneDelta`            | StreamingActivity (per lane) |
| `codex_approval_requested`     | `CodexApprovalRequest`      | ApprovalModal                |
| `copilot_lane_delta`           | `CopilotLaneDelta`          | StreamingActivity (per lane) |
| `copilot_permission_requested` | `CopilotPermissionRequest`  | ApprovalModal                |
| `opencode_lane_delta`          | `OpenCodeLaneDelta`         | StreamingActivity (per lane) |
| `opencode_permission_requested`| `OpenCodePermissionRequest` | ApprovalModal                |
| `gemini_lane_delta`            | `GeminiLaneDelta`           | StreamingActivity (per lane) |
| `gemini_permission_requested`  | `GeminiPermissionRequest`   | ApprovalModal (dismiss-only; backend auto-denied) |
| `cursor_lane_delta`            | `CursorLaneDelta`           | StreamingActivity (per lane) |
| `cursor_permission_requested`  | `CursorPermissionRequest`   | ApprovalModal (dismiss-only; backend auto-denied) |

## CONVENTIONS

- Dark theme (bg-zinc-950, text-zinc-100) — no light mode
- Tailwind CSS 4 for styling
- `lucide-react` for icons
- Never call `invoke()` directly — use `ipc.ts`
- Event listeners use `listen<T>("event", handler)` from `@tauri-apps/api/event`
- Always cleanup: `unlisten.then((fn) => fn())` in useEffect return
