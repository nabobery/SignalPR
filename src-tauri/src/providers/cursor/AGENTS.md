# Cursor Provider

**Cursor CLI via ACP** — JSON-RPC 2.0 over newline-delimited stdio (`agent acp`).

## STRUCTURE

```
cursor/
├── manager.rs   # CursorManager: child process lifecycle, session management, ACP dispatch
├── provider.rs  # CursorProvider: ReviewProvider impl (build prompt, run review, parse output)
└── mod.rs       # Barrel: pub mod manager; pub mod provider
```

## PROCESS LIFECYCLE (`manager.rs`)

`CursorManager` owns a single persistent child process. All sessions share it.

- `ensure_started()` — idempotent spawn + handshake under 15s timeout
- Binary: `agent` (Cursor CLI). Override with `CURSOR_CLI_PATH`.
- Spawn args: `--mode ask acp` (locks agent to read-only exploration at policy layer). Disable via `CURSOR_ACP_DISABLE_MODE_FLAG=1` for builds that reject the flag.
- Auth: `CURSOR_API_KEY` (Cloud Agents → User API Keys at cursor.com/dashboard). Browser-based `agent login` also honored if pre-authenticated, but health check only validates env var.
- Child stderr drained into `tracing::debug` to prevent pipe backpressure.
- Session buffers capped at 1 MiB per session; oldest bytes dropped with UTF-8 boundary safety.
- `session_roots` tracks per-session cwd to sandbox `fs/read_text_file` requests.

## SESSION FLOW (`provider.rs`)

1. `ensure_started()` — idempotent
2. `create_session(lane_id, cwd)` → `CursorSessionHandle { session_id, available_modes }`
3. `set_session_mode("ask")` — belt-and-braces with `--mode ask` spawn flag; non-fatal if method not found
4. `set_session_model(model)` — non-fatal; falls back to server default on failure
5. `prompt(session_id, text)` — 300s timeout, cancellable via `CancellationToken`
6. `unregister_session()` on completion, error, or cancellation

## ACP WIRE METHODS

| Direction | Method | Notes |
|-----------|--------|-------|
| Outbound | `initialize` | First call after spawn |
| Outbound | `session/new` | Returns `available_modes` |
| Outbound | `session/set_mode` | `modeId: "ask"` — belt-and-braces; tolerate `method not found` |
| Outbound | `session/set_model` | Unstable — tolerate failure, warn + continue |
| Outbound | `session/prompt` | Sends prompt, accumulates response text |
| Outbound | `session/cancel` | Notification (no response expected) |
| Inbound | `session/update` | Streaming chunk → `cursor_lane_delta` event |
| Inbound | `session/request_permission` | Auto-denied; broadcast on `cursor_permission_requested` |
| Inbound | `fs/read_text_file` | Proxied to real disk; sandboxed to session cwd; max 200 KiB / 2000 lines |
| Inbound | `fs/write_text_file` | **Always refused** |

## OUTPUT PARSING

Same strategy as Gemini provider (no structured-output tool in ACP):
1. Strip ` ```json ` / ` ``` ` markdown fences
2. Locate outermost `{...}` to tolerate leading prose
3. Deserialize as `CodexReviewOutput`

`truncate_for_log` finds char boundaries — never slices mid-codepoint.

## MODELS

Default: `auto` (Cursor routes to best available for the user's plan). Configurable in settings.
`VALID_CURSOR_MODELS` in `provider.rs` is the authoritative list for the UI dropdown — not enforced at runtime.

Current values: `auto`, `gpt-5.2`, `sonnet-4.5-thinking`, `sonnet-4.5`, `opus-4.6`.

## EVENTS

| Event | Payload type | Trigger |
|-------|-------------|---------|
| `cursor_lane_delta` | `CursorSessionEvent` | Each `session/update` chunk |
| `cursor_permission_requested` | `CursorPermissionRequest` | Each `session/request_permission` (observational only) |

## IPC COMMAND

`resolve_cursor_permission` (`commands/cursor.rs`) — **no-op stub**. Manager auto-denies all permission requests; this IPC exists as scaffolding for future interactive approval gating.

## SECURITY POSTURE

- `--mode ask` at spawn (primary enforcement) + `session/set_mode("ask")` belt-and-braces
- `fs/write_text_file` always refused regardless of mode
- `fs/read_text_file` sandboxed to session cwd; capped at 200 KiB / 2000 lines
- `session/request_permission` auto-denied; never silently passes through
- **Opt-in only** — Cursor does not participate in the `"auto"` provider fallback chain

## ANTI-PATTERNS

- Never pass `CURSOR_API_KEY` as a JSON argument — inherit via child env only
- `set_session_mode` and `set_session_model` failures are non-fatal — warn + continue, never abort
- Do not add Cursor to the `"auto"` fallback chain without explicit user opt-in
- `VALID_CURSOR_MODELS` in `provider.rs` is the display list; do not enforce at runtime
