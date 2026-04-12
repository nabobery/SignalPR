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
├── jsonrpc/           # Shared JSON-RPC 2.0 transport (Codex + Copilot + Gemini)
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

| Provider   | File              | Auth                | Method                           |
| ---------- | ----------------- | ------------------- | -------------------------------- |
| `Codex`    | codex.rs          | CLI subprocess      | One-shot `codex exec`            |
| `CodexApp` | codex_app_server/ | JSON-RPC stdio      | Persistent process, streaming    |
| `Claude`   | claude.rs         | `ANTHROPIC_API_KEY` | HTTP + `tool_use`                |
| `Copilot`  | copilot/          | GitHub Copilot CLI  | JSON-RPC v3, Content-Length      |
| `OpenCode` | opencode/         | `opencode` CLI      | HTTP REST + SSE                  |
| `Gemini`   | gemini/           | `GEMINI_API_KEY`    | ACP JSON-RPC over stdio (ndjson) |
| `GitHub`   | github.rs         | `gh` CLI            | PR fetching only                 |
| `Mock`     | mock.rs           | Built-in fixture    | `#[cfg(test)]` only              |

### CodexAppServer Details

See `codex_app_server/AGENTS.md` for full architecture.

### Copilot Details

See `copilot/AGENTS.md` for v3 JSON-RPC protocol, session lifecycle, and permission flow.

### OpenCode Details

See `opencode/AGENTS.md` for HTTP REST + SSE protocol, session management, and permission flow.

### Gemini Details

- Default model: `gemini-2.5-pro` (stable tier). Selectable in settings; valid IDs include `gemini-2.5-pro`, `gemini-2.5-flash`, `gemini-2.5-flash-lite`, `gemini-3-pro-preview`, `gemini-3-flash-preview`, plus aliases `auto`, `pro`, `flash`, `flash-lite` (source: upstream `VALID_GEMINI_MODELS` in `packages/core/src/config/models.ts`, verified 2026-04-12).
- Spawns `gemini --acp` as a managed child process. Override the flag via `GEMINI_ACP_FLAG=--experimental-acp` for users pinned to pre-PR-#21171 builds (merged 2026-03-05); the old flag is still accepted upstream as a deprecated alias.
- Speaks Agent Client Protocol (ACP) — JSON-RPC 2.0 over stdio with newline-delimited framing. Wire method names are `initialize`, `authenticate`, `session/new`, `session/set_mode`, `session/set_model` (unstable), `session/prompt`, `session/cancel` (notification), `session/update` (inbound), `session/request_permission` (inbound), `fs/read_text_file` / `fs/write_text_file` (inbound).
- **Startup handshake** is bounded by a 15-second timeout and wraps `initialize` + `authenticate`. Child stderr is drained into `tracing::debug` so the pipe never backs up even under the known stdout-pollution issue (google-gemini/gemini-cli#22647).
- **Explicit `authenticate` call** after `initialize` using whichever `authMethods[].id` mentions "api-key" or "gemini" (maps to `AuthType.USE_GEMINI` upstream). The API key itself is inherited via the child's env; we never pass it as a JSON arg.
- Auth: `GEMINI_API_KEY` (AI Studio) or Vertex `GOOGLE_*` env vars. **OAuth / Code-Assist is intentionally not supported** — Gemini CLI's ToS notice reserves those paths for first-party clients, so third-party harnesses must use API keys.
- **Security posture**: after `session/new`, the provider inspects `modes` from the response and — if upstream has plan mode enabled — calls `session/set_mode` with `modeId: "plan"` (read-only). Permission requests (`session/request_permission`) are **denied by default** with a spec-compliant `{outcome: "cancelled"}` response and broadcast to the UI on `gemini_permission_requested` as observational cards. `fs/read_text_file` is proxied to the real disk; `fs/write_text_file` is refused. A follow-up PR will gate permission responses on an interactive user decision via a pending-permission oneshot and the scaffolded `resolve_gemini_permission` IPC command.
- **Session buffers** are capped at 1 MiB per session (drops oldest bytes with UTF-8 boundary safety). After each `session/prompt` call the manager emits a synthetic `session.prompt_complete` event so `lib.rs` can clear per-lane delta buffers without having to track ACP lifecycle variants.
- Structured output: system prompt instructs the agent to emit a single JSON object matching the output schema; provider parses the accumulated `agent_message_chunk` text with markdown-fence and leading-prose tolerance.
- **Opt-in only**: Gemini is reachable only when the user explicitly selects it as their preferred provider — it does not participate in the `"auto"` fallback chain, since a paid API key should not be silently selected.

### Claude Provider Details

- Default model: `claude-sonnet-4-5-20250929`
- Uses Anthropic Messages API with forced `tool_use` for structured output
- Retry on 429 with exponential backoff (2 attempts max)
- Cancellation via `CancellationToken` in request loop

### JSON-RPC Shared Transport (`jsonrpc/`)

Shared JSON-RPC 2.0 wire protocol used by Codex App Server, Copilot, and Gemini:

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
- Streaming providers emit `codex_lane_delta` events
- Approval flows use `codex_approval_requested` event
