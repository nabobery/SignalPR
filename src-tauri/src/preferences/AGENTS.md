# Preferences Module

**Reviewer preference scoring** — time-decay accept rates for LLM tuning.

## STRUCTURE

```
preferences/
├── scoring.rs    # PreferenceSummary computation + prompt block
├── decisions.rs  # ReviewerDecision model
└── mod.rs        # Barrel exports
```

## KEY CONCEPTS

| Type                | Purpose                                      |
| ------------------- | -------------------------------------------- |
| `ReviewerDecision`  | Accept/reject/skip record with timestamp     |
| `PreferenceSummary` | Aggregated accept_rate per (agent, category) |

## SCORING

- Decay factor: 0.95 per day
- "accept" + "edit" = accept; "reject" + "skip" = reject
- `accept_rate = Σ(weight * is_accept) / Σ(weight)`

## CONVENTIONS

- `compute_preference_summaries()` returns sorted `Vec<PreferenceSummary>`
- `generate_prompt_block()` formats summaries for LLM system prompt injection
- Timestamps parsed as RFC 3339, fallback to now on parse failure
