# Orchestration Module

**Multi-lane review pipeline engine** — coordinates parallel provider execution.

## STRUCTURE

```
orchestration/
├── engine.rs   # Main pipeline orchestration
├── lane.rs     # Lane config, status, results
├── state.rs    # ReviewRunState machine
└── mod.rs      # Barrel exports
```

## ENGINE: run_review_pipeline

```
Stage 1: Running agents → provider.run_review() (parallel lanes)
Stage 2: Cleaner pipeline → cleaner::clean()
Stage 3: Persist findings → queries::insert_finding()
```

## MULTI-LANE SYSTEM

Three parallel review lanes with distinct focuses:

| Lane           | Icon   | Focus                            |
| -------------- | ------ | -------------------------------- |
| `security`     | Shield | Vulnerabilities, auth, injection |
| `architecture` | Layers | Design patterns, modularity      |
| `performance`  | Gauge  | Bottlenecks, efficiency          |

### LaneStatus enum

```rust
pub enum LaneStatus {
    Pending,
    Running,
    Completed { finding_count: usize },
    Failed { error: String },
    TimedOut,
    Cancelled,
}
```

### LaneSnapshot (sent to frontend)

```rust
pub struct LaneSnapshot {
    pub lane_id: String,
    pub status: String,
    pub finding_count: usize,
    pub provider_name: String,
    pub error_message: Option<String>,
}
```

## STATE MACHINE: ReviewRunState

```
Created → RunningAgents → Cleaning → ReadyForReview → Submitting → Submitted
                ↓              ↓            ↓              ↓
              Failed ←────────┴────────────┴──────────────┘
```

## CANCELLATION

Pipeline checks `cancel.is_cancelled()` at each stage boundary. On cancel:

- Updates run status to "failed"
- Emits `ReviewEvent::ReviewFailed`
- Returns `Ok(())` (not error)

## TESTING

- Use `init_db_in_memory()` for isolated tests
- Mock providers via `SlowProvider` pattern
- Test cancellation at each stage boundary
- Lane status transitions: `test_valid_transitions`, `test_invalid_transitions`
