# SignalPR Knowledge Base

**Generated:** 2026-03-28
**Branch:** main
**Commit:** b070974

## OVERVIEW

SignalPR is a **reviewer-first desktop app for AI-assisted PR review**. Built with Tauri 2 (Rust backend + React/TypeScript frontend). Fetches GitHub PR diffs, runs AI review via Codex/Claude/Copilot providers, presents findings in a structured workspace with multi-lane parallel analysis. Supports real-time streaming and interactive approval flows.

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
| WebSocket          | tokio-tungstenite | 0.26         |
| Frontend testing   | Vitest            | 4.x          |
| Repo config        | YAML (serde_yml)  | 0.0.12       |

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
│   ├── src/commands/       # IPC handlers (15 files, incl. copilot.rs)
│   ├── src/config/         # Configuration resolution (438 lines, preset inheritance)
│   ├── src/providers/      # AI providers (Codex, Claude, Copilot, Mock)
│   │   ├── jsonrpc/        # Shared JSON-RPC transport (dual framing)
│   │   ├── copilot/        # Copilot v3 provider (manager + provider)
│   │   └── codex_app_server/ # Codex App Server provider
│   ├── src/orchestration/  # Multi-lane review pipeline
│   ├── src/storage/        # SQLite layer
│   ├── src/cleaner/        # Finding dedup/rank/normalize/verify/synthesis
│   ├── src/notifications/  # GitHub poll notifications
│   ├── src/channels/       # Discord/Slack notification channels
│   ├── src/autofix/        # Auto-fix patch generation
│   ├── src/agents/         # Custom agent definitions
│   └── src/preferences/    # Reviewer preference scoring
└── docs/                   # PRD + implementation docs
```

## WHERE TO LOOK

| Task                     | Location                              | Notes                                     |
| ------------------------ | ------------------------------------- | ----------------------------------------- |
| Add new Tauri command    | `src-tauri/src/commands/`             | Register in lib.rs `invoke_handler`       |
| Modify review UI         | `src/features/review/`                | ReviewWorkspace is main orchestrator      |
| Add AI provider          | `src-tauri/src/providers/`            | Implement `ReviewProvider` trait          |
| Claude provider          | `src-tauri/src/providers/claude.rs`   | Direct HTTP to Anthropic API              |
| Copilot provider         | `src-tauri/src/providers/copilot/`    | JSON-RPC to Copilot CLI server            |
| Change IPC contract      | `src/lib/ipc.ts` + matching command   | Types in `src/lib/types.ts`               |
| Modify data pipeline     | `src-tauri/src/cleaner/`              | dedup → normalize → rank → verify → remap |
| Multi-lane system        | `src-tauri/src/orchestration/lane.rs` | Security/arch/performance lanes           |
| Database schema          | `src-tauri/src/storage/models.rs`     | rusqlite, no migrations yet               |
| Notification polling     | `src-tauri/src/notifications/`        | GitHub review request poller              |
| Discord/Slack channels   | `src-tauri/src/channels/`             | ChannelManager + Discord/Slack impls      |
| Auto-fix patches         | `src-tauri/src/autofix/`              | Search/replace → unified diff             |
| Custom agents            | `src-tauri/src/agents/`               | AgentDefinition + registry                |
| Reviewer preferences     | `src-tauri/src/preferences/`          | Scoring with time-decay                   |
| Configuration resolution | `src-tauri/src/config/mod.rs`         | Three-layer: defaults → user → repo       |
| Repo config              | `.signalpr.yml` at workspace root     | YAML, all fields optional                 |
| Settings UI              | `src/features/settings/`              | General/Presets/Agents/Channels tabs      |
| Frontend tests           | `src/**/*.test.tsx`                   | Vitest + Testing Library                  |

## IPC COMMANDS (Frontend → Backend)

| Command                    | Handler        | Purpose                             |
| -------------------------- | -------------- | ----------------------------------- |
| `inspect_environment`      | environment.rs | Check gh/codex CLI availability     |
| `get_environment_summary`  | environment.rs | Full environment status             |
| `open_from_url`            | intake.rs      | Parse PR URL, fetch diff            |
| `confirm_workspace`        | intake.rs      | Bind PR to local workspace path     |
| `start_review`             | review.rs      | Launch async review pipeline        |
| `cancel_review`            | review.rs      | Cancel running review               |
| `get_review_snapshot`      | review.rs      | Get full review state               |
| `get_incomplete_reviews`   | review.rs      | List reviews that need completion   |
| `resume_review`            | review.rs      | Resume an incomplete review         |
| `update_finding`           | findings.rs    | Edit/suppress findings              |
| `submit_review`            | submission.rs  | Submit review to GitHub             |
| `get_submission_history`   | submission.rs  | Get past submissions for a run      |
| `get_settings`             | settings.rs    | Get all user settings               |
| `update_setting`           | settings.rs    | Update a user setting               |
| `export_diagnostic_bundle` | diagnostics.rs | Export full diagnostics for a run   |
| `get_event_log`            | diagnostics.rs | Get pipeline event log for a run    |
| `get_agents`               | agents.rs      | List custom agent definitions       |
| `save_agent`               | agents.rs      | Create/update custom agent          |
| `delete_agent`             | agents.rs      | Remove custom agent                 |
| `apply_fix`                | autofix.rs     | Apply auto-fix patch                |
| `get_channel_statuses`     | channels.rs    | Get Discord/Slack connection status |
| `configure_channel`        | channels.rs    | Set webhook URL for channel         |
| `remove_channel`           | channels.rs    | Remove channel configuration        |
| `get_channel_status`       | channels.rs    | Get connection status array         |
| `has_channel_token`        | channels.rs    | Check if channel has stored token   |
| `start_channel_listeners`  | channels.rs    | Start background channel polling    |
| `stop_channel_listeners`   | channels.rs    | Stop background channel polling     |
| `resolve_codex_approval`      | codex.rs       | Approve/decline codex tool request    |
| `resolve_copilot_permission`  | copilot.rs     | Approve/deny copilot permission (v3)  |
| `get_preferences`             | preferences.rs | Get reviewer preference summaries     |

## CONFIGURATION

### Three-Layer Config Resolution

```
defaults → user settings (DB) → repo config (.signalpr.yml)
```

**Config options** (all optional in repo config):

| Key                    | Type   | Default         | Description                 |
| ---------------------- | ------ | --------------- | --------------------------- |
| `lanes`                | array  | [sec,arch,perf] | Review lanes to run         |
| `max_findings`         | usize  | 8               | Max surfaced findings       |
| `similarity_threshold` | f64    | 0.70            | Dedup similarity threshold  |
| `drop_nitpicks`        | bool   | true            | Filter out nitpick findings |
| `min_confidence`       | f64    | 0.0             | Min confidence to surface   |
| `lane_timeout_secs`    | u64    | 120             | Per-lane timeout            |
| `preferred_provider`   | string | "auto"          | Provider preference         |

**Provider selection fallback**: preferred → codex → claude → copilot → mock

### Repo Config File

Place `.signalpr.yml` at workspace root. Unknown fields ignored for forward compatibility.

```yaml
lanes:
  - security
  - performance
max_findings: 5
drop_nitpicks: false
similarity_threshold: 0.80
```

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

| Provider | File              | Auth Method         | Notes                                             |
| -------- | ----------------- | ------------------- | ------------------------------------------------- |
| Codex    | codex.rs          | CLI subprocess      | One-shot `codex exec`                             |
| CodexApp | codex_app_server/ | JSON-RPC stdio      | Persistent process, newline-delimited, streaming  |
| Claude   | claude.rs         | `ANTHROPIC_API_KEY` | Direct HTTP, tool_use                             |
| Copilot  | copilot/          | GitHub Copilot CLI  | JSON-RPC v3, Content-Length framed, streaming      |
| GitHub   | github.rs         | `gh` CLI            | PR fetching only                                  |
| Mock     | codex.rs          | Built-in fixture    | Fallback for testing                              |

### TypeScript

- Feature-based folder structure (`features/<name>/`)
- IPC calls wrapped in `src/lib/ipc.ts` (never inline `invoke`)
- Zod-adjacent typing (manual interfaces in `types.ts`)
- `ReviewContext` via React.createContext for workspace state

### Testing (Frontend)

- Vitest with jsdom environment
- Test files: `*.test.tsx` or `*.test.ts` colocated with source
- Mock Tauri APIs via `src/test/mocks.ts`
- Use `@testing-library/react` for component tests
- `@testing-library/jest-dom` matchers auto-loaded via setup

### Naming

- `run_id` (snake_case) for review pipeline IDs
- `pr_id` for PR identifiers
- Findings use severity: `blocker | critical | warning | info | nitpick`

## STREAMING EVENTS (Frontend)

| Event                          | Payload                    | Purpose                         |
| ------------------------------ | -------------------------- | ------------------------------- |
| `codex_approval_requested`     | `CodexApprovalRequest`     | Interactive Codex tool approval |
| `codex_lane_delta`             | `CodexLaneDelta`           | Real-time Codex streaming       |
| `copilot_permission_requested` | `CopilotPermissionRequest` | Copilot v3 permission approval  |
| `copilot_lane_delta`           | `CopilotLaneDelta`         | Real-time Copilot streaming     |

## NEW MODULES

### `src-tauri/src/config/mod.rs`

Configuration resolution with three layers. Key functions:

- `resolve_config(conn, repo_config)` → `ResolvedConfig`
- `select_provider(app, preference)` → `Arc<dyn ReviewProvider>`
- `load_repo_config(path)` → `Option<RepoConfig>`

### `src-tauri/src/cleaner/remap.rs`

Remaps finding anchors when PR diff changes between review start and submission:

- `remap_findings(findings, old_diff, new_diff)` → `RemapResult`
- Files removed → orphaned findings
- Hunks shifted → adjust line numbers
- Hunks gone → demote to file-level (clear line anchors)

### `src-tauri/src/channels/`

Discord/Slack notification channels + WebSocket transport:

- `ChannelManager` with broadcast events
- `DiscordWebhook` / `SlackWebhook` implementations
- `ws_manager.rs` — generic WebSocket loop with reconnection + exponential backoff
- `secrets.rs` for webhook URL storage

### `src-tauri/src/providers/codex_app_server/`

Long-running Codex provider via JSON-RPC over stdio:

- `manager.rs` — Process lifecycle, thread↔lane mapping, approval/notification broadcast
- `provider.rs` — `ReviewProvider` impl with streaming buffer, multi-turn support
- `transport.rs` — JSON-RPC wire protocol (requests, responses, notifications, server requests)
- Enables interactive approval flows (`codex_approval_requested` event)

### `src-tauri/src/providers/copilot/`

GitHub Copilot provider via v3 JSON-RPC over stdio with Content-Length framing:

- `manager.rs` — Process lifecycle (`copilot --server`), protocol version detection via `ping`, session→lane mapping
  - Unwraps `session.event` notifications by `event.type` into `CopilotSessionEvent`
  - Routes `permission.requested` events to permission broadcast
  - Responds to permissions via `session.permissions.handlePendingPermissionRequest` RPC
  - Responds to tool calls via `session.tools.handlePendingToolCall` RPC
  - Child-scoped `CancellationToken` allows manager restart after shutdown
  - Session lifecycle: `create_session` / `send_message` / `abort_session` / `destroy_session`
- `provider.rs` — `ReviewProvider` impl with v3 event loop
  - Uses `submit_review` custom tool for structured output
  - Handles `external_tool.requested`, `session.idle`, `session.error` events
  - 300s review timeout, session cleanup on all exit paths

### `src-tauri/src/providers/jsonrpc/`

Shared JSON-RPC 2.0 transport used by both Codex App Server and Copilot:

- `types.rs` — Wire types (`OutboundMessage`, `InboundMessage`), `FramingMode` enum, `inject_jsonrpc()` helper, `parse_inbound()` discriminator
- `transport.rs` — Bidirectional transport with two framing modes:
  - `NewlineDelimited` — `{json}\n` (Codex)
  - `ContentLength` — `Content-Length: N\r\n\r\n{json}` (Copilot, LSP-style)
  - All outbound messages include `"jsonrpc":"2.0"` via `inject_jsonrpc()`

### `src-tauri/src/autofix/`

Auto-fix patch generation:

- `FixSuggestion` (search/replace)
- `search_replace_to_unified_diff()` for patch format
- `apply.rs` for patch application

### `src-tauri/src/agents/`

Custom agent definitions:

- `AgentDefinition` struct (name, system_prompt, agent_type)
- `AgentRegistry` for storing/retrieving agents
- Loaded from user settings via `custom_agent_` prefix

### `src-tauri/src/preferences/`

Reviewer preference scoring with time-decay:

- `compute_preference_summaries()` with 0.95 decay factor
- `generate_prompt_block()` for LLM system prompt injection
- `ReviewerDecision` model for accept/reject tracking

## ANTI-PATTERNS

- **Never** call `invoke()` directly from components — use `ipc.ts` wrappers
- **Never** mutate `ReviewState` directly — use `setState` with spread
- **Never** suppress Rust errors with `unwrap()` in production paths — use `?` with proper error types
- **Never** bypass the cleaner pipeline — raw findings must go through dedup/rank/verify/remap
- **Never** use `as any` or `@ts-ignore` in TypeScript
- **Never** write tests without mocking Tauri APIs

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

# Testing
pnpm test:rust        # Rust unit tests
pnpm test:frontend    # Vitest (frontend)
pnpm test             # Both rust + frontend

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
- Event log (`EventLog`) captures pipeline events for diagnostics
- Config resolution cascades: defaults → user DB settings → `.signalpr.yml`
- `react-error-boundary` added for graceful error handling in UI
