# Notifications Module

**GitHub review request polling** — background poller for PR notifications.

## STRUCTURE

```
notifications/
├── github_poll.rs  # GitHubPoller implementation
└── mod.rs          # Barrel exports
```

## COMPONENT: GitHubPoller

Periodically polls GitHub for review request notifications using `gh api`.

### Polling Flow

```
1. Check setting: github_polling_enabled == "true"
2. If enabled: spawn GitHubPoller with 5-min interval
3. Poll loop: gh api notifications → filter review_requested → emit events
4. Dedup via in-memory HashSet<seen_ids>
```

### Key Types

```rust
pub struct GitHubPoller {
    app: AppHandle,
    interval: Duration,              // Default: 300s (5 min)
    cancel: CancellationToken,
    seen_ids: Arc<Mutex<HashSet<String>>>,
}
```

### Events Emitted

| Event                     | Payload          |
| ------------------------- | ---------------- |
| `github_review_requested` | `{ title, url }` |

## CONVENTIONS

- Opt-in: only starts if `github_polling_enabled=true` in settings
- `seen_ids` is in-memory only — restarts may re-notify (acceptable for Phase 2)
- Uses `tauri_plugin_shell` for `gh` CLI execution
- Graceful cancellation via `CancellationToken`
- Errors logged via `tracing::warn!`, never crash the poller

## FUTURE

- ETag support for GitHub API (Phase 3)
- OS keychain for token storage (Phase 3)
- Migrate to `reqwest` for direct API calls (Phase 3)
