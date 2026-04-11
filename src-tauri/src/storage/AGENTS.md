# Storage Module

**SQLite data layer** — models, schema, and query functions.

## STRUCTURE

```
storage/
├── models.rs      # Struct definitions (Workspace, PullRequest, ReviewRun, Finding)
├── queries.rs     # SQL query functions (1291 lines)
├── db.rs          # Connection management + init
├── event_log.rs   # Pipeline event logging for diagnostics
├── hashing.rs     # Content hashing (sha2) for dedup/fingerprinting
└── mod.rs         # Barrel exports
```

## MODELS

| Model         | Table         | Purpose                 |
| ------------- | ------------- | ----------------------- |
| `Workspace`   | workspaces    | Local workspace binding |
| `PullRequest` | pull_requests | PR metadata + diff      |
| `ReviewRun`   | review_runs   | Review pipeline status  |
| `Finding`     | findings      | AI review findings      |

## PATTERNS

- `AppDb` wraps `Mutex<Connection>` — lock before queries
- Use `init_db_in_memory()` for tests
- No migrations — schema changes are manual
- IDs use UUID v4

## CONVENTIONS

- Snake_case for Rust structs (matches SQL columns)
- `Option<T>` for nullable fields
- Timestamps stored as ISO 8601 strings
- `changed_files` stored as JSON string
