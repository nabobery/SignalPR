# Copilot Provider

**GitHub Copilot v3 via JSON-RPC over stdio** — Content-Length framed, interactive permissions.

## STRUCTURE

```
copilot/
├── manager.rs    # Process lifecycle, session→lane mapping, permission broadcast
├── provider.rs   # ReviewProvider impl with v3 event loop
└── mod.rs        # Barrel exports
```

## MANAGER (`manager.rs`)

Spawns `copilot --server`, manages sessions and permission routing:

- Protocol version detection via `ping` RPC
- Session lifecycle: `create_session` / `send_message` / `abort_session` / `destroy_session`
- Unwraps `session.event` notifications by `event.type` into `CopilotSessionEvent`
- Routes `permission.requested` events to permission broadcast
- Responds via `session.permissions.handlePendingPermissionRequest` RPC
- Responds to tool calls via `session.tools.handlePendingToolCall` RPC
- Child-scoped `CancellationToken` allows manager restart after shutdown

## PROVIDER (`provider.rs`)

`ReviewProvider` impl:

- Uses `submit_review` custom tool for structured output
- Handles `external_tool.requested`, `session.idle`, `session.error` events
- 300s review timeout, session cleanup on all exit paths

## EVENTS

| Event                          | Direction | Purpose                    |
| ------------------------------ | --------- | -------------------------- |
| `copilot_permission_requested` | → UI      | Permission approval prompt |
| `copilot_lane_delta`           | → UI      | Real-time streaming delta  |

## CONVENTIONS

- Uses shared `jsonrpc/` transport with `ContentLength` framing mode
- Lazy startup: process spawns on first `health_check` or `run_review`
- All child process access through `Arc<Mutex<Inner>>`
- Permission requests require UI response (don't auto-decide)
