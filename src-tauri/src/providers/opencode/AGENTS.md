# OpenCode Provider

**OpenCode via HTTP REST + SSE** — not JSON-RPC. Spawns `opencode serve` and communicates via `reqwest` HTTP + Server-Sent Events.

## STRUCTURE

```
opencode/
├── sse.rs        # Incremental SSE parser, bus envelope unwrap, reconnect with backoff
├── manager.rs    # Process lifecycle, session→lane mapping, permission routing
├── provider.rs   # ReviewProvider impl using synchronous message endpoint
└── mod.rs        # Barrel exports
```

## SSE PARSER (`sse.rs`)

Custom incremental SSE parser:

- Uses `bytes_stream()` from reqwest response
- Unwraps bus envelope `{type, properties}` format
- Reconnects with exponential backoff on disconnect

## MANAGER (`manager.rs`)

Spawns `opencode serve --port 0 --hostname 127.0.0.1`:

- Sessions: `POST /session` with `{title}`
- Messages: `POST /session/{id}/message` with `{parts, system, model, format}`
- Uses `format: json_schema` for structured output (reads `info.structured` from sync response)
- Permission replies: `POST /permission/{requestID}/reply` with `{reply: "once"|"always"|"reject"}`
- Auth: HTTP Basic with `OPENCODE_SERVER_USERNAME` (default `"opencode"`) + `OPENCODE_SERVER_PASSWORD`
- Routes SSE events (`message.part.updated`, `session.status`, `permission.asked`) to broadcast
- Child-scoped `CancellationToken` allows manager restart after shutdown

## PROVIDER (`provider.rs`)

`ReviewProvider` impl:

- Sends diff with `json_schema` format, parses `info.structured` as `CodexReviewOutput`
- SSE used for streaming deltas + permission forwarding (not result retrieval)
- 300s review timeout, session cleanup on all exit paths

## EVENTS

| Event                          | Direction | Purpose                    |
| ------------------------------ | --------- | -------------------------- |
| `opencode_permission_requested`| → UI      | Permission approval prompt |
| `opencode_lane_delta`          | → UI      | Real-time streaming delta  |

## CONVENTIONS

- Does NOT use shared `jsonrpc/` transport (HTTP REST instead)
- Lazy startup: process spawns on first `health_check` or `run_review`
- All child process access through `Arc<Mutex<Inner>>`
- Permission replies are `once`, `always`, or `reject` (not approve/decline)
