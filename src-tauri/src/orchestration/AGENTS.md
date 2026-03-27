# Orchestration Module

**Review pipeline engine** — coordinates provider execution and cleaner pipeline.

## ENGINE: run_review_pipeline

```
Stage 1: Running agents → provider.run_review()
Stage 2: Cleaner pipeline → cleaner::clean()
Stage 3: Persist findings → queries::insert_finding()
```

## CANCELLATION

Pipeline checks `cancel.is_cancelled()` at each stage boundary. On cancel:

- Updates run status to "failed"
- Emits `ReviewEvent::ReviewFailed`
- Returns `Ok(())` (not error)

## EVENTS

```rust
pub enum ReviewEvent {
    StatusChanged { status: String },
    ReviewReady { run_id: String },
    ReviewFailed { run_id: String, error: String },
}
```

## TESTING

- Use `init_db_in_memory()` for isolated tests
- Mock providers via `SlowProvider` pattern
- Test cancellation at each stage boundary
