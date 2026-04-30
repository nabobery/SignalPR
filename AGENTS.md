# SignalPR Knowledge Base

**Generated:** 2026-04-30
**Branch:** main
**Commit:** 6c5264f

## OVERVIEW

SignalPR is a reviewer-first desktop app for AI-assisted PR review. It combines a Tauri 2 backend with React/TypeScript frontend rendering for PR diff review. Findings flow through multi-lane providers and are surfaced in a unified workspace with streaming, approvals, and focused review actions.

## STACK

Tauri 2 (Rust) + React 19 + TypeScript 5.8 + Vite 7 + Tailwind CSS 4. SQLite (rusqlite), Tokio async, reqwest HTTP, tokio-tungstenite WS. Vitest for frontend tests.

## STRUCTURE

```
signalpr/
├── src/           # React frontend (`src/AGENTS.md`)
│   ├── features/  # App features (review/onboarding/...)
│   └── lib/       # IPC wrappers + shared types
├── src-tauri/     # Rust backend (`src-tauri/AGENTS.md`, module AGENTS under `src-tauri/src/*`)
└── .github/       # CI/workflow definitions
```

## WHERE TO LOOK

| Task                   | Location                            | Notes                                |
| ---------------------- | ----------------------------------- | ------------------------------------ |
| Add Tauri command      | `src-tauri/src/commands/`           | Register in lib.rs `invoke_handler`  |
| Backend entrypoints    | `src-tauri/AGENTS.md`               | Backend command/pipeline/providers map |
| Diff rendering         | `src/features/review/diff/`         | Pierre parser, line annotations, collapse heuristics |
| Add AI provider        | `src-tauri/src/providers/`          | Implement `ReviewProvider` trait     |
| Change IPC contract    | `src/lib/ipc.ts` + matching command | Types in `src/lib/types.ts`          |
| Review UI              | `src/features/review/`             | ReviewWorkspace is orchestrator      |
| Data pipeline          | `src-tauri/src/cleaner/`           | dedup→normalize→rank→verify→remap    |
| Multi-lane system      | `src-tauri/src/orchestration/`     | security/arch/performance lanes      |
| Database               | `src-tauri/src/storage/`           | rusqlite, no migrations              |
| Config resolution      | `src-tauri/src/config/`            | defaults → user DB → .signalpr.yml   |
| Channels               | `src-tauri/src/channels/`          | Discord/Slack + WebSocket transport  |
| Settings UI            | `src/features/settings/`           | General/Presets/Agents/Channels tabs |

## IPC COMMANDS

34 commands across 15 handler files. Registered in `src-tauri/src/lib.rs` via `generate_handler!`.

## CONFIGURATION

Three-layer resolution: `defaults → user settings (DB) → .signalpr.yml`

See `src-tauri/src/config/AGENTS.md` for details.

Frontend diff rendering now uses `@pierre/diffs` in `src/features/review/diff/`, with legacy fallback for robust rendering.

**Provider selection fallback**: preferred → codex → claude → copilot → opencode → mock

Gemini and Cursor are **opt-in only** — excluded from the `"auto"` chain (paid API keys should not be silently consumed).

## CONVENTIONS

### Rust

- `mod.rs` barrel exports in each module
- `async_trait` for async trait methods
- `tokio_util::sync::CancellationToken` for cancellation
- Custom errors via `thiserror` (`AppError`, `ProviderError`)
- Tests use `init_db_in_memory()` for isolation
- Multi-lane parallel reviews: `security`, `architecture`, `performance`
- `LaneStatus` enum for per-lane progress tracking
- Unit tests in `#[cfg(test)] mod tests` within each file

### Providers

See `src-tauri/src/providers/AGENTS.md` for full provider table and details.

### TypeScript

- Feature-based folder structure (`features/<name>/`)
- IPC calls wrapped in `src/lib/ipc.ts` (never inline `invoke`)
- Manual interfaces in `types.ts`, `ReviewContext` via React.createContext
- Frontend linting uses `oxlint` via `pnpm lint` and `pnpm lint:fix`.
- Frontend formatting uses `oxfmt` via `pnpm format` and `pnpm format:check` for `src/**/*.{ts,tsx,css}`.

### Testing

- Vitest (jsdom) for frontend, `cargo test` for Rust
- Test files colocated: `*.test.tsx`/`*.test.ts` (frontend), `#[cfg(test)]` inline (Rust)
- Mock Tauri APIs via `src/test/mocks.ts`
- Rust tests use `init_db_in_memory()` for isolation
- Diff subsystem tests are colocated in `src/features/review/diff/` (parser, annotation mapping, heuristics, integration fixtures)

### Naming

- Severity: `blocker | critical | warning | info | nitpick`
- IDs: `run_id`, `pr_id` (snake_case)

## STREAMING EVENTS

Frontend and backend stream event updates through `review_progress` and per-lane events (`codex|copilot|opencode|gemini|cursor|pi_lane_delta`) plus permission-request events (e.g. `*_permission_requested`) into `ReviewWorkspace` and related components.

## ANTI-PATTERNS

- **Never** call `invoke()` directly from components — use `ipc.ts` wrappers
- **Never** mutate `ReviewState` directly — use `setState` with spread
- **Never** suppress Rust errors with `unwrap()` in production paths — use `?` with proper error types
- **Never** bypass the cleaner pipeline — raw findings must go through dedup/rank/verify/remap
- **Never** use `as any` or `@ts-ignore` in TypeScript
- **Never** write tests without mocking Tauri APIs
- **Never** bypass `DiffPanel` fallback behavior when touching parser/renderer changes
- **Never** run tooling binaries with ad-hoc `npx` invocations when repo scripts can call `pnpm exec`

## COMMANDS

```bash
pnpm tauri dev        # Full dev mode (Vite + Rust)
pnpm tauri build      # Production build
pnpm lint             # Oxlint frontend JS/TS checks (`pnpm exec oxlint src/`)
pnpm lint:fix         # Auto-fix frontend lint violations
pnpm format           # Oxfmt frontend source formatting (`src/**/*.{ts,tsx,css}`)
pnpm format:check     # Check frontend formatting
pnpm check            # All checks (typecheck + lint + format + clippy + tests)
pnpm test             # Both rust + frontend tests
```

## NOTES

- SQLite stored in Tauri app data dir (`app.path().app_data_dir()`)
- Review pipeline is fully async with cancellation support at each stage
- Frontend listens to `review_progress` event for live updates
- No migrations — schema changes require manual SQL
- Event log (`EventLog`) captures pipeline events for diagnostics
- `EventLog` + `hashing.rs` in storage for event tracking and content hashing
- Config resolution cascades: defaults → user DB settings → `.signalpr.yml`
- `react-error-boundary` for graceful error handling in UI
- No CI/CD yet — builds are developer-driven via `pnpm tauri build`
- CSP set to `null` in tauri.conf.json — permissive for desktop, relies on Tauri capabilities
