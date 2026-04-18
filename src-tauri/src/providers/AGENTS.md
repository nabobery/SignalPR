# Providers Module

**AI provider abstraction** — trait-based system for PR review providers.

## STRUCTURE

```
providers/
├── traits.rs          # ReviewProvider trait + data types
├── codex.rs           # Codex CLI implementation (one-shot)
├── codex_app_server/  # Long-running Codex via JSON-RPC
├── copilot/           # GitHub Copilot v3 via JSON-RPC + Content-Length framing
├── opencode/          # OpenCode via HTTP REST + SSE
├── gemini/            # Gemini CLI via ACP (JSON-RPC + NDJSON framing)
├── cursor/            # Cursor CLI via ACP (JSON-RPC + NDJSON framing)
├── pi/                # PI Agent SDK via pi --mode rpc (LF-delimited JSONL, single-session)
├── jsonrpc/           # Shared JSON-RPC 2.0 transport (Codex + Copilot + Gemini + Cursor)
├── claude.rs          # Direct HTTP to Anthropic API
├── github.rs          # GitHub integration (PR fetching only)
├── mock.rs            # Mock provider (#[cfg(test)] only)
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

| Provider   | File              | Auth                | Method                                          |
| ---------- | ----------------- | ------------------- | ----------------------------------------------- |
| `Codex`    | codex.rs          | CLI subprocess      | One-shot `codex exec`                           |
| `CodexApp` | codex_app_server/ | JSON-RPC stdio      | Persistent process, streaming                   |
| `Claude`   | claude.rs         | `ANTHROPIC_API_KEY` | HTTP + `tool_use`                               |
| `Copilot`  | copilot/          | GitHub Copilot CLI  | JSON-RPC v3, Content-Length                     |
| `OpenCode` | opencode/         | `opencode` CLI      | HTTP REST + SSE                                 |
| `Gemini`   | gemini/           | `GEMINI_API_KEY`    | ACP JSON-RPC over stdio (ndjson)                |
| `Cursor`   | cursor/           | `CURSOR_API_KEY`    | ACP JSON-RPC over stdio (ndjson)                |
| `PI`       | pi/               | `pi` CLI            | pi --mode rpc, LF-JSONL, single-session         |
| `GitHub`   | github.rs         | `gh` CLI            | PR fetching only                                |
| `Mock`     | mock.rs           | Built-in fixture    | `#[cfg(test)]` only                             |

### CodexAppServer Details

See `codex_app_server/AGENTS.md` for full architecture.

### Copilot Details

See `copilot/AGENTS.md` for v3 JSON-RPC protocol, session lifecycle, and permission flow.

### OpenCode Details

See `opencode/AGENTS.md` for HTTP REST + SSE protocol, session management, and permission flow.

### Cursor Details

See `cursor/AGENTS.md` for ACP protocol, session lifecycle, `fs/` sandboxing, and security posture.

### PI Details

See `pi/AGENTS.md` for RPC protocol, single-session lifecycle, JSON extraction strategy, and buffer limits.

### Gemini Details

See `gemini/AGENTS.md` for ACP protocol, session lifecycle, `authenticate` handshake, and security posture.

### Claude Provider Details

- Default model: `claude-sonnet-4-5-20250929`
- Uses Anthropic Messages API with forced `tool_use` for structured output
- Retry on 429 with exponential backoff (2 attempts max)
- Cancellation via `CancellationToken` in request loop

### JSON-RPC Shared Transport (`jsonrpc/`)

Shared JSON-RPC 2.0 wire protocol used by Codex App Server, Copilot, Gemini, and Cursor:

- `types.rs` — `OutboundMessage`, `InboundMessage`, `FramingMode` enum
- `transport.rs` — Bidirectional transport with two framing modes:
  - `NewlineDelimited` — `{json}\n` (Codex, Gemini ACP)
  - `ContentLength` — `Content-Length: N\r\n\r\n{json}` (Copilot, LSP-style)
- All outbound messages include `"jsonrpc":"2.0"` via `inject_jsonrpc()`

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
- Streaming providers emit `{provider}_lane_delta` events (e.g. `cursor_lane_delta`)
- Permission/approval flows emit `{provider}_permission_requested` events
- Gemini and Cursor are **observational only** — backend auto-denies; UI shows dismiss-only cards
