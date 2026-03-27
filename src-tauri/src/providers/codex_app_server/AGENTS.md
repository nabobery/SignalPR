# Codex App Server Provider

**Long-running Codex provider via JSON-RPC over stdio** — enables streaming, multi-turn, and interactive approval flows.

## STRUCTURE

```
codex_app_server/
├── manager.rs    # Process lifecycle + broadcast channels
├── provider.rs   # ReviewProvider implementation
├── transport.rs  # JSON-RPC wire protocol
└── mod.rs        # Barrel exports
```

## ARCHITECTURE

```
CodexAppServerProvider
  └── CodexAppServerManager
        ├── JsonRpcTransport (stdin/stdout)
        ├── broadcast::Sender<ApprovalRequest>   → ApprovalModal
        ├── broadcast::Sender<ServerNotification> → StreamingActivity
        └── lane_by_thread: HashMap<String, String>
```

## KEY TYPES

| Type                     | Purpose                                       |
| ------------------------ | --------------------------------------------- |
| `CodexAppServerManager`  | Process lifecycle, thread↔lane mapping        |
| `CodexAppServerProvider` | `ReviewProvider` impl with streaming buffer   |
| `JsonRpcTransport`       | Wire protocol (request/response/notification) |
| `ApprovalRequest`        | Server-initiated approval with IDs            |
| `ServerNotification`     | Streaming delta event                         |

## TRANSPORT (`transport.rs`)

JSON-RPC 2.0 over child process stdio:

**OutboundMessage:**

- `Request { id, method, params }` — Client request
- `Response { id, result }` — Client response
- `Notification { method, params }` — Fire-and-forget

**InboundMessage:**

- `Response { id, result }` — Request completed
- `ErrorResponse { id, error }` — Request failed
- `ServerRequest { id, method, params }` — Server asking for approval
- `Notification { method, params }` — Server broadcast

**Request/response correlation:**

- `oneshot::Sender` stored in `pending: HashMap<Value, oneshot::Sender<...>>`
- `AtomicU64` counter for unique request IDs

## MANAGER (`manager.rs`)

Process lifecycle management:

```rust
impl CodexAppServerManager {
    pub fn new() -> Self;                                    // Create (no process yet)
    pub async fn start(&self, cwd: &Path) -> Result<...>;    // Spawn child + connect transport
    pub async fn stop(&self);                                // Graceful shutdown
    pub async fn request_review(&self, thread_id, input) -> Result<Value>;
    pub async fn resolve_approval(&self, request_id, decision);
}
```

**Broadcast channels:**

- `approval_tx`: `broadcast::Sender<ApprovalRequest>` — 128 capacity
- `notification_tx`: `broadcast::Sender<ServerNotification>` — 1024 capacity

**Thread↔lane mapping:**

- `register_thread_lane(thread_id, lane_id)` — Map codex thread to SignalPR lane
- `unregister_thread(thread_id)` — Clean up on completion
- `lane_for_thread(thread_id) -> Option<String>` — Lookup for events

## PROVIDER (`provider.rs`)

Implements `ReviewProvider` trait:

```rust
impl ReviewProvider for CodexAppServerProvider {
    fn provider_name(&self) -> &str { "codex-app" }
    async fn health_check(&self) -> ProviderHealth;
    async fn run_review(&self, input, cwd, cancel) -> Result<CodexReviewOutput>;
}
```

**Streaming buffer:**

- `MAX_STREAM_BUFFER`: 16 KB
- `push_capped()` — Append delta, drain from front if over limit
- Emits `CodexLaneDelta` events via notification broadcast

**Turn timeout:** 300 seconds (`DEFAULT_TURN_TIMEOUT`)

## CONVENTIONS

- Lazy startup: process spawns on first `health_check` or `run_review`
- All child process access through `Arc<Mutex<Inner>>`
- Cancellation via `CancellationToken` propagated to transport
- Use `tracing::info/debug/warn` for logging
- Approval requests require UI response (don't auto-decide)
