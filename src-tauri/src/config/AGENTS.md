# Config Module

**Configuration resolution** — three-layer merge with preset inheritance.

## STRUCTURE

```
config/
├── mod.rs      # resolve_config(), select_provider() (438 lines)
├── presets.rs  # Preset resolution + extends logic
├── merge.rs    # Config merge utilities
└── mod.rs      # Barrel exports
```

## THREE-LAYER RESOLUTION

```
defaults → user settings (DB) → repo config (.signalpr.yml)
```

## KEY FUNCTIONS

| Function                         | Returns                   |
| -------------------------------- | ------------------------- |
| `resolve_config(conn, repo, ws)` | `ResolvedConfig`          |
| `select_provider(app, pref)`     | `Arc<dyn ReviewProvider>` |
| `load_repo_config(path)`         | `Option<RepoConfig>`      |
| `resolve_extends(val, ws, ...)`  | `Option<RepoConfig>`      |

## RESOLVEDCONFIG FIELDS

```rust
pub struct ResolvedConfig {
    pub cleaner: CleanerConfig,
    pub preferred_provider: String,
    pub lane_timeout: Duration,
    pub lanes: Vec<String>,
    pub custom_agents: Vec<AgentDefinition>,
}
```

## PRESET INHERITANCE

- `extends` field in `.signalpr.yml` references preset name
- Presets cached in `.signalpr_cache/preset_cache/`
- Recursive resolution with max depth 5 to prevent cycles
- Child config overrides parent fields

## CONVENTIONS

- Unknown config fields ignored (forward compatibility)
- Invalid values fall back to defaults silently
- Custom agents loaded from `custom_agent_*` settings prefix
- Provider selection: preferred → codex → claude → copilot → opencode → mock
- Gemini and Cursor are excluded from `"auto"` fallback — only selected when `preferred_provider` is explicitly set to them
