# Providers Module

**AI provider abstraction** — trait-based system for PR review providers.

## STRUCTURE

```
providers/
├── traits.rs    # ReviewProvider trait + data types
├── codex.rs     # Codex CLI implementation
├── github.rs    # GitHub integration
└── mod.rs       # Barrel exports
```

## TRAIT: ReviewProvider

```rust
#[async_trait]
pub trait ReviewProvider: Send + Sync {
    async fn health_check(&self) -> ProviderHealth;
    async fn run_review(
        &self,
        diff: &str,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError>;
}
```

## DATA TYPES

| Type                | Purpose                                  |
| ------------------- | ---------------------------------------- |
| `ProviderHealth`    | Health check result (available, version) |
| `CodexReviewOutput` | Review results + findings                |
| `RawFinding`        | Individual finding before cleaning       |

## CONVENTIONS

- Implement `async_trait` for async methods
- Use `CancellationToken` for cancellable operations
- Return `ProviderError` (not `AppError`) from providers
- Health checks must not fail — return degraded status
