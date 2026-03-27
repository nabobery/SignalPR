# Providers Module

**AI provider abstraction** — trait-based system for PR review providers.

## STRUCTURE

```
providers/
├── traits.rs          # ReviewProvider trait + data types
├── codex.rs           # Codex CLI implementation (one-shot)
├── codex_app_server/  # Long-running Codex via JSON-RPC
│   ├── manager.rs     # Process lifecycle, broadcast channels
│   ├── provider.rs    # ReviewProvider implementation
│   ├── transport.rs   # JSON-RPC wire protocol
│   └── mod.rs         # Barrel exports
├── claude.rs          # Direct HTTP to Anthropic API
├── github.rs          # GitHub integration
├── prompts.rs         # Agent focus configs (security/arch/performance)
└── mod.rs             # Barrel exports
```

## TRAIT: ReviewProvider

```rust
#[async_trait]
pub trait ReviewProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn health_check(&self) -> ProviderHealth;
    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError>;
}
```

## PROVIDERS

| Provider   | File              | Auth                | Method                        |
| ---------- | ----------------- | ------------------- | ----------------------------- |
| `Codex`    | codex.rs          | CLI subprocess      | One-shot `codex exec`         |
| `CodexApp` | codex_app_server/ | JSON-RPC stdio      | Persistent process, streaming |
| `Claude`   | claude.rs         | `ANTHROPIC_API_KEY` | HTTP + `tool_use`             |
| `GitHub`   | github.rs         | `gh` CLI            | PR fetching only              |
| `Mock`     | codex.rs          | Built-in fixture    | Fallback for testing          |

### CodexAppServer Provider Details

Long-running provider with interactive capabilities:

- **Transport**: JSON-RPC over child process stdio
- **Streaming**: Real-time text deltas via `CodexLaneDelta` events
- **Approvals**: Server-initiated approval requests (`codex_approval_requested` event)
- **Multi-turn**: Thread/turn tracking via `lane_by_thread` mapping
- **Default model**: `gpt-5.2-codex`

**Key types:**

- `CodexAppServerManager` — Process lifecycle + broadcast channels
- `JsonRpcTransport` — Wire protocol (request/response/notification/server-request)
- `ApprovalRequest` — Forwarded approval with thread/turn/item IDs

### Claude Provider Details

- Default model: `claude-sonnet-4-5-20250929`
- Uses Anthropic Messages API with forced `tool_use` for structured output
- Retry on 429 with exponential backoff (2 attempts max)
- Cancellation via `CancellationToken` in request loop

## DATA TYPES

| Type                | Purpose                                  |
| ------------------- | ---------------------------------------- |
| `ProviderHealth`    | Health check result (available, version) |
| `ReviewInput`       | System prompt + diff + output schema     |
| `CodexReviewOutput` | Review results + findings                |
| `RawFinding`        | Individual finding before cleaning       |

## CONVENTIONS

- Implement `async_trait` for async methods
- Use `CancellationToken` for cancellable operations
- Return `ProviderError` (not `AppError`) from providers
- Health checks must not fail — return degraded status
- Streaming providers emit `codex_lane_delta` events
- Approval flows use `codex_approval_requested` event
