# Channels Module

**Discord/Slack notification channels** — webhook integrations for PR events + WebSocket transport.

## STRUCTURE

```
channels/
├── manager.rs     # Broadcast event bus + status tracking
├── discord.rs     # Discord webhook integration
├── slack.rs       # Slack webhook integration
├── ws_manager.rs  # Generic WebSocket loop with reconnection
├── secrets.rs     # Webhook URL storage (OS keychain future)
└── mod.rs         # Barrel exports + shared types
```

## KEY TYPES

| Type             | Purpose                                     |
| ---------------- | ------------------------------------------- |
| `ChannelManager` | Singleton managing event broadcast + status |
| `ChannelEvent`   | PR event payload (source, url, requester)   |
| `ChannelStatus`  | Connection status per channel               |
| `WsConfig`       | WebSocket backoff configuration             |

## PATTERNS

- Uses `tokio::sync::broadcast` for pub/sub
- Webhook URLs stored in SQLite via `secrets.rs`
- Status updates replace existing source (dedup by `source` field)
- Tests use round-trip emit/subscribe pattern

## WEBSOCKET MANAGER (`ws_manager.rs`)

Generic WebSocket loop with automatic reconnection:

```rust
pub async fn run_ws_loop(
    url_provider: UrlProvider,      // Async closure returning WS URL
    config: WsConfig,               // Backoff config (1s→60s, 2x multiplier)
    incoming_tx: mpsc::Sender<String>,  // Forward incoming messages
    outgoing_rx: mpsc::Receiver<String>, // Receive outgoing messages
    status_callback: Box<dyn Fn(bool)>,  // Connected/disconnected callback
    cancel: CancellationToken,       // Clean shutdown
)
```

**Key features:**

- Exponential backoff with 0-25% jitter
- Ping/pong handling
- Text message forwarding only (ignores binary)
- `CancellationToken` for clean shutdown
- Status callback fires on connect/disconnect

**WsConfig defaults:**

- `initial_backoff`: 1s
- `max_backoff`: 60s
- `backoff_multiplier`: 2.0

## CONVENTIONS

- Channels are opt-in (user must configure webhook URL)
- Errors logged via `tracing::warn!`, never crash
- `ChannelManager` is Tauri managed state (`State<'_, ChannelManager>`)
- WebSocket errors trigger reconnection, never panic
