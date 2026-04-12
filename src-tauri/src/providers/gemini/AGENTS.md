# Gemini Provider

**Gemini CLI via ACP** — JSON-RPC 2.0 over newline-delimited stdio (`gemini --acp`).

## STRUCTURE

```
gemini/
├── manager.rs   # GeminiManager: child process lifecycle, session management, ACP dispatch
├── provider.rs  # GeminiProvider: ReviewProvider impl (build prompt, run review, parse output)
└── mod.rs       # Barrel: pub mod manager; pub mod provider
```

## PROCESS LIFECYCLE (`manager.rs`)

`GeminiManager` owns a single persistent child process. All sessions share it.

- `ensure_started()` — idempotent spawn + handshake under 15s timeout
- ACP flag: `--acp` (default since PR #21171, merged 2026-03-05). Override: `GEMINI_ACP_FLAG=--experimental-acp` for pinned older builds (deprecated alias still accepted upstream).
- Handshake: `initialize` → `authenticate` (picks `authMethods[].id` containing "api-key" or "gemini").
- Auth: env-var only — `GEMINI_API_KEY`, `GOOGLE_API_KEY`, or `GOOGLE_APPLICATION_CREDENTIALS`. **OAuth not supported** (Google ToS reserves those paths for first-party clients).
- Child stderr drained into `tracing::debug` to prevent pipe backpressure (upstream stdout-pollution bug #22647).
- Session buffers capped at 1 MiB per session; oldest bytes dropped with UTF-8 boundary safety.

## SESSION FLOW (`provider.rs`)

1. `ensure_started()` — idempotent
2. `create_session(lane_id, cwd)` → `GeminiSessionHandle { session_id, available_modes }`
3. `set_session_mode("plan")` if server advertises it — prevents file writes + shell exec at policy level
4. `set_session_model(model)` — non-fatal; some CLI builds reject this unstable call
5. `prompt(session_id, text)` — 300s timeout, cancellable via `CancellationToken`
6. `unregister_session()` on completion, error, or cancellation

## ACP WIRE METHODS

| Direction | Method | Notes |
|-----------|--------|-------|
| Outbound | `initialize` | First call after spawn |
| Outbound | `authenticate` | Selects API-key auth method from `authMethods` |
| Outbound | `session/new` | Returns `available_modes` |
| Outbound | `session/set_mode` | `modeId: "plan"` when available |
| Outbound | `session/set_model` | Unstable — tolerate `method not found` |
| Outbound | `session/prompt` | Sends prompt, accumulates response text |
| Outbound | `session/cancel` | Notification (no response expected) |
| Inbound | `session/update` | Streaming chunk → `gemini_lane_delta` event |
| Inbound | `session/request_permission` | Auto-denied `{outcome: "cancelled"}`; broadcast on `gemini_permission_requested` |
| Inbound | `fs/read_text_file` | Proxied to real disk |
| Inbound | `fs/write_text_file` | **Always refused** |

## OUTPUT PARSING

ACP has no structured-output tool. Provider instructs the model to emit a raw JSON object and parses:
1. Strip ` ```json ` / ` ``` ` markdown fences
2. Locate outermost `{...}` to tolerate leading prose
3. Deserialize as `CodexReviewOutput`

Tolerates CJK/emoji in log truncation (`truncate_for_log` finds char boundaries — never slices mid-codepoint).

## MODELS

Default: `gemini-2.5-pro` (stable tier; no preview gating on health checks). Configurable in settings.
`VALID_GEMINI_MODELS` in `provider.rs` is the authoritative list for the UI dropdown — not enforced at runtime.

## EVENTS

| Event | Payload type | Trigger |
|-------|-------------|---------|
| `gemini_lane_delta` | `GeminiSessionEvent` | Each `session/update` chunk |
| `gemini_permission_requested` | `GeminiPermissionRequest` | Each `session/request_permission` (observational only) |

## IPC COMMAND

`resolve_gemini_permission` (`commands/gemini.rs`) — **currently a no-op stub**. Manager auto-denies all permission requests; this IPC exists as scaffolding for future interactive approval gating.

## SECURITY POSTURE

- Plan mode requested after every `session/new` (belt-and-braces with deny-by-default permissions)
- `fs/write_text_file` always refused regardless of mode
- `session/request_permission` auto-denied; never silently passes through
- **Opt-in only** — Gemini does not participate in the `"auto"` provider fallback chain (API key should not be silently consumed)

## ANTI-PATTERNS

- Never pass `GEMINI_API_KEY` as a JSON argument — inherit via child env only
- `set_session_model` failures are non-fatal — warn + continue, never abort the review
- Do not add Gemini to the `"auto"` fallback chain without explicit user opt-in
