**Generated:** 2026-04-30 20:51 IST
**Branch:** main
**Commit:** 6c5264f

## OVERVIEW

Rust + Tauri backend host for review orchestration, provider execution, persistence, and desktop integrations.

## STRUCTURE

```
src-tauri/
├── src-tauri/
│   ├── main.rs               # Tauri binary entrypoint
│   ├── lib.rs                # App bootstrap, command registration, event wiring
│   ├── commands/             # #[tauri::command] handlers
│   ├── orchestration/        # Multi-lane review pipeline + state machine
│   ├── providers/            # Provider trait + concrete provider backends
│   ├── cleaner/              # Dedup/normalize/rank/verify/remap/synthesis
│   ├── storage/              # SQLite schema, models, queries, event log
│   ├── config/               # Config merge + provider resolution
│   ├── channels/             # Slack/Discord webhook + websocket
│   ├── notifications/        # GitHub review-request poller
│   ├── preferences/          # Reviewer preference scoring
│   ├── agents/               # Custom agent definition registry
│   ├── autofix/              # Fix suggestion diff conversion/apply
│   └── secrets/              # Provider secret persistence helpers
└── src-tauri/bridges/       # Claude CLI compatibility bridge assets
```

## WHERE TO LOOK

| Task | Location | Notes |
| ---- | -------- | ----- |
| Backend bootstrap | `src-tauri/src/lib.rs`, `src-tauri/src/main.rs` | App state, command handlers, event fanout, tray bootstrap |
| Start/cancel review pipeline | `src-tauri/src/commands/review.rs`, `src-tauri/src/orchestration/engine.rs` | Parallel lanes, cleaner, DB persistence |
| Provider registry | `src-tauri/src/providers/mod.rs`, `src-tauri/src/providers/AGENTS.md` | Provider trait + implementations + capabilities |
| Credentials + capabilities | `src-tauri/src/providers/capabilities.rs`, `src-tauri/src/commands/providers.rs` | Env/keychain status and fallback policy |
| Persistence | `src-tauri/src/storage/{db,queries,models}.rs` | SQLite lifecycle and review state |
| Event-driven UI updates | `src-tauri/src/commands/review.rs`, `src-tauri/src/{commands,lib}.rs` | `review_progress` and lane delta events |
| Settings and channels | `src-tauri/src/commands/settings.rs`, `src-tauri/src/notifications/*`, `src-tauri/src/channels/*` | Pollers + webhook listeners |

## CONVENTIONS

- Backend modules use typed errors (`AppError`) and serialize failures as command return errors.
- Managers and long-lived providers are lazily started and held as shared `Arc` state.
- Provider output and stream callbacks must be cancellation-aware (`CancellationToken`).
- Keep command handlers thin; orchestration/business logic belongs in domain modules.
- SQLite bootstrap and writes are centralized in `storage::db` and `storage::queries`.
- Keep per-lane streaming buffers bounded (truncated safely at UTF-8 boundaries).
- Backend events are routed through the UI-safe event bus (`tauri::Emitter`) only.

## ANTI-PATTERNS

- Don’t add a provider without updating `providers::mod`, capability registration, and command exposure.
- Don’t execute provider work on the frontend command thread; keep async work cancelable.
- Don’t mutate lane state directly in UI-facing modules; emit events through orchestration domain.
- Never crash process startup/pipeline with recoverable provider errors (degrade status instead).
- Avoid direct SQL/schema edits in command code; use storage query helpers.
