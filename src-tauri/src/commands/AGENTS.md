# Commands Module

**IPC command handlers** — Tauri command entry points for frontend requests.

## MODULES

| File             | Commands                                               | Purpose                           |
| ---------------- | ------------------------------------------------------ | --------------------------------- |
| `environment.rs` | `inspect_environment`, `get_environment_summary`       | Check gh/codex/copilot/opencode/gemini CLI + env-var auth |
| `intake.rs`      | `open_from_url`, `confirm_workspace`                   | PR URL parsing, workspace binding |
| `review.rs`      | `start_review`, `cancel_review`, `get_review_snapshot` | Async review lifecycle            |
| `review.rs`      | `get_incomplete_reviews`, `resume_review`              | Incomplete review recovery        |
| `findings.rs`    | `update_finding`                                       | Edit/suppress findings            |
| `submission.rs`  | `submit_review`, `get_submission_history`              | Submit to GitHub + history        |
| `settings.rs`    | `get_settings`, `update_setting`                       | User settings CRUD                |
| `diagnostics.rs` | `export_diagnostic_bundle`, `get_event_log`            | Debug/export pipeline events      |
| `agents.rs`      | `get_agent_definitions`, `save_agent_definition`, `delete_agent_definition` | Custom agent management |
| `autofix.rs`     | `preview_fix`, `apply_fix`, `accept_fix`, `reject_fix` | Auto-fix workflow                |
| `codex.rs`       | `resolve_codex_approval`                               | Codex interactive approval        |
| `copilot.rs`     | `resolve_copilot_permission`                           | Copilot v3 permission approval    |
| `opencode.rs`    | `resolve_opencode_permission`                          | OpenCode permission reply         |
| `gemini.rs`      | `resolve_gemini_permission`                            | Gemini permission stub (no-op; future: interactive approval gate) |
| `cursor.rs`      | `resolve_cursor_permission`                            | Cursor permission stub (no-op; future: interactive approval gate) |
| `providers.rs`   | `get_provider_credential_statuses`, `store_provider_secret`, `delete_provider_secret`, `get_provider_capabilities`, `get_agent_run_metadata` | Provider credentials + capability metadata |
| `channels.rs`    | `configure_channel`, `remove_channel`, `get_channel_status`, etc. | Channel management |
| `preferences.rs` | `get_preferences`                                      | Reviewer preference summaries     |

## PATTERNS

- All commands use `#[tauri::command]` attribute
- Access shared state via `State<'_, T>` parameter
- `ActiveReviews` is `Mutex<HashMap<String, CancellationToken>>` for cancellation
- Returns `Result<T, String>` (serialized errors to frontend)
- `EventLog` state for pipeline event tracking

## CONVENTIONS

- Extract DB conn via `db.0.lock()?.deref()`
- Use `queries::*` functions for DB operations
- Emit `review_progress` event for status updates
- Settings stored as `HashMap<String, String>` key-value pairs
- Provider and channel commands expose errors through `AppError` conversion paths before serialization
- Command handlers are intentionally slim; orchestration/business logic stays in non-command modules
