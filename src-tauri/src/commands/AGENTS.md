# Commands Module

**IPC command handlers** — Tauri command entry points for frontend requests.

## MODULES

| File             | Commands                                               | Purpose                           |
| ---------------- | ------------------------------------------------------ | --------------------------------- |
| `environment.rs` | `inspect_environment`                                  | Check gh/codex CLI availability   |
| `intake.rs`      | `open_from_url`, `confirm_workspace`                   | PR URL parsing, workspace binding |
| `review.rs`      | `start_review`, `cancel_review`, `get_review_snapshot` | Async review lifecycle            |
| `review.rs`      | `get_incomplete_reviews`, `resume_review`              | Incomplete review recovery        |
| `findings.rs`    | `update_finding`                                       | Edit/suppress findings            |
| `submission.rs`  | `submit_review`                                        | Submit to GitHub                  |
| `settings.rs`    | `get_settings`, `update_setting`                       | User settings CRUD                |
| `diagnostics.rs` | `export_diagnostic_bundle`, `get_event_log`            | Debug/export pipeline events      |

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
