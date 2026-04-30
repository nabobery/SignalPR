# Claude Code Provider

Sidecar-backed provider for persistent Claude Code review execution (`claude-code-bridge`).

## STRUCTURE

```
claude_code/
├── mod.rs            # Re-exports provider + manager
├── manager.rs        # Sidecar process lifecycle, event parsing, lane registry, cancellation
└── provider.rs       # `ReviewProvider` adapter with parsing + health check
```

## FLOW

1. `provider.rs` builds a lane-specific review request (`review.start`) and streams it to sidecar stdin.
2. `manager.rs` parses JSON-RPC style lines from stdout/stderr and emits:
   - `ClaudeCodeEvent`: `review.delta`, `review.completed`, `review.error`
   - `ClaudeCodePermissionRequest` (`review.permission_requested`) when sidecar asks for tool approval
3. Completion/error result is converted to `CodexReviewOutput`; lane process registrations are cleaned up on exit/cancel.
4. `run_review` enforces cancellation via `tokio::sync::CancellationToken`.

## WHERE TO LOOK

| File | Purpose |
| ---- | ------- |
| `manager.rs` | Sidecar spawn/IO, process registry, events, `check_health` |
| `provider.rs` | `ReviewProvider` implementation + schema parsing + cancel wiring |
| `src-tauri/src/commands/claude_code.rs` | Command wrappers for health checks, start/retrieve status |
| `src-tauri/src/providers/AGENTS.md` | Provider matrix and governance policy entry points |

## CONVENTIONS

- Sidecar path and temp/config directories are validated/created by `manager.rs`.
- Health check must include bridge status, bridge version, mode, and SDK version.
- Permission support is deliberately deferred (`TODO` placeholder) and guarded by governance layer.
- `validate_sidecar_binary` rejects missing/empty/invalid sidecar files early (fail fast, clear errors).
- Keep per-lane process maps one-entry-per-lane; cancelation must not affect unrelated lanes.

## ANTI-PATTERNS

- Don’t bypass permission gating layer; route any interactive action decision through governance first.
- Don’t write unbounded stderr output into memory.
- Don’t keep sidecar children alive beyond lane lifecycle or cancellation event.
