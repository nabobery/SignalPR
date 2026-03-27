# Cleaner Module

**Finding post-processing pipeline** — dedup → normalize → rank → verify.

## PIPELINE

```rust
pub fn clean(
    raw_findings: Vec<RawFinding>,
    diff: &str,
    run_id: &str,
    config: &CleanerConfig,
) -> CleanResult
```

## STAGES

| Stage       | File         | Purpose                        |
| ----------- | ------------ | ------------------------------ |
| `dedup`     | dedup.rs     | Remove duplicate findings      |
| `normalize` | normalize.rs | Standardize severity, format   |
| `rank`      | rank.rs      | Score and filter by confidence |
| `verify`    | verify.rs    | Validate findings against diff |

## CONFIG

```rust
pub struct CleanerConfig {
    pub min_confidence: f64,
    pub max_findings: usize,
    pub dedup_threshold: f64,
}
```

## OUTPUT

```rust
pub struct CleanResult {
    pub surfaced: Vec<Finding>,      // Final output
    pub suppressed: Vec<RawFinding>, // Filtered out
}
```
