# Channels Module

**Discord/Slack notification channels** — webhook integrations for PR events.

## STRUCTURE

```
channels/
├── manager.rs   # Broadcast event bus + status tracking
├── discord.rs   # Discord webhook integration
├── slack.rs     # Slack webhook integration
├── secrets.rs   # Webhook URL storage (OS keychain future)
└── mod.rs       # Barrel exports + shared types
```

## KEY TYPES

| Type             | Purpose                                     |
| ---------------- | ------------------------------------------- |
| `ChannelManager` | Singleton managing event broadcast + status |
| `ChannelEvent`   | PR event payload (source, url, requester)   |
| `ChannelStatus`  | Connection status per channel               |

## PATTERNS

- Uses `tokio::sync::broadcast` for pub/sub
- Webhook URLs stored in SQLite via `secrets.rs`
- Status updates replace existing source (dedup by `source` field)
- Tests use round-trip emit/subscribe pattern

## CONVENTIONS

- Channels are opt-in (user must configure webhook URL)
- Errors logged via `tracing::warn!`, never crash
- `ChannelManager` is Tauri managed state (`State<'_, ChannelManager>`)
