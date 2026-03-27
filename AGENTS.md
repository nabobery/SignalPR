# SignalPR Knowledge Base

**Generated:** 2026-03-27
**Branch:** main

## OVERVIEW

SignalPR is a **reviewer-first desktop app for AI-assisted PR review**. Built with Tauri 2 (Rust backend + React/TypeScript frontend). Fetches GitHub PR diffs, runs AI review via Codex/Claude providers, presents findings in a structured workspace with multi-lane parallel analysis.

## STACK

| Layer              | Technology        | Version      |
| ------------------ | ----------------- | ------------ |
| Desktop shell      | Tauri             | 2.x          |
| Frontend framework | React             | 19.x         |
| Language (FE)      | TypeScript        | 5.8.x        |
| Styling            | Tailwind CSS      | 4.x          |
| Routing            | react-router      | 7.x          |
| Build tool         | Vite              | 7.x          |
| Language (BE)      | Rust              | Edition 2021 |
| Database           | SQLite (rusqlite) | 0.32         |
| Async runtime      | Tokio             | 1.x          |
| HTTP client        | reqwest           | 0.12         |

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
│   │   └── submission/     # Submit review dialog
│   ├── lib/                # Shared utilities
│   │   ├── ipc.ts          # Tauri invoke wrappers
│   │   ├── store.ts        # React context for review state
│   │   └── types.ts        # Shared TypeScript interfaces
│   └── ui/                 # Reusable UI components (empty)
├── src-tauri/              # Rust backend
│   ├── src/main.rs         # Windows subsystem entry
│   ├── src/lib.rs          # Tauri builder + command registration
│   ├── src/commands/       # IPC handlers (6 files)
│   ├── src/providers/      # AI providers (Codex, Claude, GitHub)
│   ├── src/orchestration/  # Multi-lane review pipeline
│   ├── src/storage/        # SQLite layer (models, queries)
│   ├── src/cleaner/        # Finding dedup/rank/normalize/verify
│   └── src/notifications/  # GitHub poll notifications
└── docs/                   # PRD + implementation docs
```

## WHERE TO LOOK

| Task                  | Location                              | Notes                                |
| --------------------- | ------------------------------------- | ------------------------------------ |
| Add new Tauri command | `src-tauri/src/commands/`             | Register in lib.rs `invoke_handler`  |
| Modify review UI      | `src/features/review/`                | ReviewWorkspace is main orchestrator |
| Add AI provider       | `src-tauri/src/providers/`            | Implement `ReviewProvider` trait     |
| Claude provider       | `src-tauri/src/providers/claude.rs`   | Direct HTTP to Anthropic API         |
| Change IPC contract   | `src/lib/ipc.ts` + matching command   | Types in `src/lib/types.ts`          |
| Modify data pipeline  | `src-tauri/src/cleaner/`              | dedup → normalize → rank → verify    |
| Multi-lane system     | `src-tauri/src/orchestration/lane.rs` | Security/arch/performance lanes      |
| Database schema       | `src-tauri/src/storage/models.rs`     | rusqlite, no migrations yet          |
| Notification polling  | `src-tauri/src/notifications/`        | GitHub review request poller         |

## IPC COMMANDS (Frontend → Backend)

| Command               | Handler        | Purpose                         |
| --------------------- | -------------- | ------------------------------- |
| `inspect_environment` | environment.rs | Check gh/codex CLI availability |
| `open_from_url`       | intake.rs      | Parse PR URL, fetch diff        |
| `confirm_workspace`   | intake.rs      | Bind PR to local workspace path |
| `start_review`        | review.rs      | Launch async review pipeline    |
| `cancel_review`       | review.rs      | Cancel running review           |
| `get_review_snapshot` | review.rs      | Get full review state           |
| `update_finding`      | findings.rs    | Edit/suppress findings          |
| `submit_review`       | submission.rs  | Submit review to GitHub         |

## CONVENTIONS

### Rust

- `mod.rs` barrel exports in each module
- `async_trait` for async trait methods
- `tokio_util::sync::CancellationToken` for cancellation
- Custom errors via `thiserror` (`AppError`, `ProviderError`)
- Tests use `init_db_in_memory()` for isolation
- Multi-lane parallel reviews: `security`, `architecture`, `performance`
- `LaneStatus` enum for per-lane progress tracking

### Providers

| Provider | File      | Auth Method         | Notes                 |
| -------- | --------- | ------------------- | --------------------- |
| Codex    | codex.rs  | CLI subprocess      | Primary provider      |
| Claude   | claude.rs | `ANTHROPIC_API_KEY` | Direct HTTP, tool_use |
| GitHub   | github.rs | `gh` CLI            | PR fetching only      |

### TypeScript

- Feature-based folder structure (`features/<name>/`)
- IPC calls wrapped in `src/lib/ipc.ts` (never inline `invoke`)
- Zod-adjacent typing (manual interfaces in `types.ts`)
- `ReviewContext` via React.createContext for workspace state

### Naming

- `run_id` (snake_case) for review pipeline IDs
- `pr_id` for PR identifiers
- Findings use severity: `blocker | critical | warning | info | nitpick`

## ANTI-PATTERNS

- **Never** call `invoke()` directly from components — use `ipc.ts` wrappers
- **Never** mutate `ReviewState` directly — use `setState` with spread
- **Never** suppress Rust errors with `unwrap()` in production paths — use `?` with proper error types
- **Never** bypass the cleaner pipeline — raw findings must go through dedup/rank/verify

## COMMANDS

```bash
# Frontend
pnpm dev              # Vite dev server
pnpm build            # tsc + vite build
pnpm check:frontend   # typecheck + lint + format

# Rust
cargo build --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings

# Combined
pnpm check            # All checks (frontend + rust)
pnpm tauri dev        # Full Tauri dev mode
pnpm tauri build      # Production build
```

## NOTES

- SQLite stored in Tauri app data dir (`app.path().app_data_dir()`)
- Review pipeline is fully async with cancellation support at each stage
- Frontend listens to `review_progress` event for live updates
- No migrations — schema changes require `cargo sqlx` or manual SQL
