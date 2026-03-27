# Providers Module

**AI provider abstraction** — trait-based system for PR review providers.

## STRUCTURE

```
providers/
├── traits.rs    # ReviewProvider trait + data types
├── codex.rs     # Codex CLI implementation
├── claude.rs    # Direct HTTP to Anthropic API
├── github.rs    # GitHub integration
├── prompts.rs   # Agent focus configs (security/arch/performance)
└── mod.rs       # Barrel exports
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

| Provider | File      | Auth                | Method                |
| -------- | --------- | ------------------- | --------------------- |
| `Codex`  | codex.rs  | CLI subprocess      | Spawns `codex review` |
| `Claude` | claude.rs | `ANTHROPIC_API_KEY` | HTTP + `tool_use`     |

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
