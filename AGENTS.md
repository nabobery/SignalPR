# SignalPR Knowledge Base

**Generated:** 2026-04-12
**Branch:** main
**Commit:** 5d5d42c

## OVERVIEW

SignalPR is a **reviewer-first desktop app for AI-assisted PR review**. Built with Tauri 2 (Rust backend + React/TypeScript frontend). Fetches GitHub PR diffs, runs AI review via Codex/Claude/Copilot/OpenCode providers, presents findings in a structured workspace with multi-lane parallel analysis. Supports real-time streaming and interactive approval flows.

## STACK

Tauri 2 (Rust) + React 19 + TypeScript 5.8 + Vite 7 + Tailwind CSS 4. SQLite (rusqlite), Tokio async, reqwest HTTP, tokio-tungstenite WS. Vitest for frontend tests.

## STRUCTURE

```
signalpr/
├── src/                    # React frontend
│   ├── main.tsx            # Entry point
│   ├── App.tsx             # Router (/ and /review/:runId)
│   ├── features/           # Feature modules
│   │   ├── intake/         # PR URL input + workspace selection
│   │   ├── onboarding/     # Environment checks (gh, codex CLI)
│   │   ├── review/         # Main workspace (7 components)
│   │   ├── settings/       # Settings UI (General/Presets/Agents/Channels)
│   │   └── submission/     # Submit review dialog
│   ├── lib/                # Shared utilities
│   │   ├── ipc.ts          # Tauri invoke wrappers
│   │   ├── store.ts        # React context for review state
│   │   └── types.ts        # Shared TypeScript interfaces
│   └── test/               # Test utilities
├── src-tauri/              # Rust backend
│   ├── src/main.rs         # Windows subsystem entry
│   ├── src/lib.rs          # Tauri builder + command registration
│   ├── src/commands/       # IPC handlers (14 modules + opencode.rs, copilot.rs)
│   ├── src/config/         # Configuration resolution (438 lines, preset inheritance)
│   ├── src/providers/      # AI providers (Codex, Claude, Copilot, OpenCode, Mock)
│   │   ├── jsonrpc/        # Shared JSON-RPC transport (dual framing)
│   │   ├── copilot/        # Copilot v3 provider (manager + provider)
│   │   ├── opencode/       # OpenCode provider (HTTP REST + SSE)
│   │   ├── codex_app_server/ # Codex App Server provider
│   │   └── mock.rs          # Mock provider (test-only, #[cfg(test)])
│   ├── src/orchestration/  # Multi-lane review pipeline
│   ├── src/storage/        # SQLite layer
│   ├── src/cleaner/        # Finding dedup/rank/normalize/verify/synthesis
│   ├── src/notifications/  # GitHub poll notifications
│   ├── src/channels/       # Discord/Slack notification channels
│   ├── src/autofix/        # Auto-fix patch generation
│   ├── src/agents/         # Custom agent definitions
│   └── src/preferences/    # Reviewer preference scoring
```

## WHERE TO LOOK

| Task                   | Location                            | Notes                                |
| ---------------------- | ----------------------------------- | ------------------------------------ |
| Add Tauri command      | `src-tauri/src/commands/`           | Register in lib.rs `invoke_handler`  |
| Add AI provider        | `src-tauri/src/providers/`          | Implement `ReviewProvider` trait     |
| Change IPC contract    | `src/lib/ipc.ts` + matching command | Types in `src/lib/types.ts`          |
| Review UI              | `src/features/review/`             | ReviewWorkspace is orchestrator      |
| Data pipeline          | `src-tauri/src/cleaner/`           | dedup→normalize→rank→verify→remap    |
| Multi-lane system      | `src-tauri/src/orchestration/`     | security/arch/performance lanes      |
| Database               | `src-tauri/src/storage/`           | rusqlite, no migrations              |
| Config resolution      | `src-tauri/src/config/`            | defaults → user DB → .signalpr.yml   |
| Channels               | `src-tauri/src/channels/`          | Discord/Slack + WebSocket transport  |
| Settings UI            | `src/features/settings/`           | General/Presets/Agents/Channels tabs |

## IPC COMMANDS (Frontend → Backend)

33 commands across 14 handler files. See `src-tauri/src/commands/AGENTS.md` for full list.

Key command groups: environment, intake, review lifecycle, findings, submission, settings, diagnostics, agents, autofix, channels, codex/copilot/opencode permissions, preferences.

All commands registered in `lib.rs` via `generate_handler!` macro.

## CONFIGURATION

Three-layer resolution: `defaults → user settings (DB) → .signalpr.yml`

See `src-tauri/src/config/AGENTS.md` for details.

**Provider selection fallback**: preferred → codex → claude → copilot → opencode → mock

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

### Testing

- Vitest (jsdom) for frontend, `cargo test` for Rust
- Test files colocated: `*.test.tsx`/`*.test.ts` (frontend), `#[cfg(test)]` inline (Rust)
- Mock Tauri APIs via `src/test/mocks.ts`
- Rust tests use `init_db_in_memory()` for isolation

### Naming

- Severity: `blocker | critical | warning | info | nitpick`
- IDs: `run_id`, `pr_id` (snake_case)

## STREAMING EVENTS

See `src/AGENTS.md` for full event table. Per-provider events:
- Codex: `codex_lane_delta`, `codex_approval_requested`
- Copilot: `copilot_lane_delta`, `copilot_permission_requested`
- OpenCode: `opencode_lane_delta`, `opencode_permission_requested`

## MODULE DETAILS

Each Rust module has its own `AGENTS.md` — see subdirectories for specifics.

Key complexity hotspots (>500 lines):
- `storage/queries.rs` (1291 lines) — all SQL operations
- `orchestration/engine.rs` (1226 lines) — pipeline orchestration
- `providers/opencode/manager.rs` (666 lines) — OpenCode process lifecycle
- `providers/copilot/manager.rs` (654 lines) — Copilot process lifecycle
- `commands/submission.rs` (650 lines) — GitHub submission logic
- `commands/review.rs` (623 lines) — review pipeline commands
- `providers/codex_app_server/manager.rs` (606 lines) — Codex process lifecycle

## ANTI-PATTERNS

- **Never** call `invoke()` directly from components — use `ipc.ts` wrappers
- **Never** mutate `ReviewState` directly — use `setState` with spread
- **Never** suppress Rust errors with `unwrap()` in production paths — use `?` with proper error types
- **Never** bypass the cleaner pipeline — raw findings must go through dedup/rank/verify/remap
- **Never** use `as any` or `@ts-ignore` in TypeScript
- **Never** write tests without mocking Tauri APIs

## COMMANDS

```bash
pnpm tauri dev        # Full dev mode (Vite + Rust)
pnpm tauri build      # Production build
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
