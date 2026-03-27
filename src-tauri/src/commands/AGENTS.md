# Commands Module

**IPC command handlers** — Tauri command entry points for frontend requests.

## MODULES

| File             | Commands                                               | Purpose                           |
| ---------------- | ------------------------------------------------------ | --------------------------------- |
| `environment.rs` | `inspect_environment`                                  | Check gh/codex CLI availability   |
| `intake.rs`      | `open_from_url`, `confirm_workspace`                   | PR URL parsing, workspace binding |
| `review.rs`      | `start_review`, `cancel_review`, `get_review_snapshot` | Async review lifecycle            |
| `findings.rs`    | `update_finding`                                       | Edit/suppress findings            |
| `submission.rs`  | `submit_review`                                        | Submit to GitHub                  |

## PATTERNS

- All commands use `#[tauri::command]` attribute
- Access shared state via `State<'_, T>` parameter
- `ActiveReviews` is `Mutex<HashMap<String, CancellationToken>>` for cancellation
- Returns `Result<T, String>` (serialized errors to frontend)

## CONVENTIONS

- Extract DB conn via `db.0.lock()?.deref()`
- Use `queries::*` functions for DB operations
- Emit `review_progress` event for status updates
