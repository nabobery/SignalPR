# Cleaner Module

**Finding post-processing pipeline** — dedup → normalize → rank → verify → remap.

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

| Stage       | File         | Purpose                               |
| ----------- | ------------ | ------------------------------------- |
| `dedup`     | dedup.rs     | Remove duplicate findings             |
| `normalize` | normalize.rs | Standardize severity, format          |
| `rank`      | rank.rs      | Score and filter by confidence        |
| `verify`    | verify.rs    | Validate findings against diff        |
| `remap`     | remap.rs     | Adjust line anchors when diff changes |

## REMAP MODULE

Remaps finding anchors when PR diff changes between review start and submission:

| Scenario                   | Action                                |
| -------------------------- | ------------------------------------- |
| File removed from new diff | Orphan the finding                    |
| Hunk shifted               | Adjust `line_start/line_end` by delta |
| Hunk gone                  | Demote to file-level (clear anchors)  |
| Unanchored finding         | Pass through unchanged                |

## CONFIG

```rust
pub struct CleanerConfig {
    pub min_confidence: f64,
    pub max_surface_findings: usize,  // renamed from max_findings
    pub similarity_threshold: f64,    // renamed from dedup_threshold
    pub drop_nitpicks: bool,
}
```

## OUTPUT

```rust
pub struct CleanResult {
    pub surfaced: Vec<Finding>,      // Final output
    pub suppressed: Vec<RawFinding>, // Filtered out
}
```
