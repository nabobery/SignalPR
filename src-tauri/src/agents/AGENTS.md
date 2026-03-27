# Agents Module

**Custom agent definitions** — user-defined review agents stored in settings.

## STRUCTURE

```
agents/
├── definition.rs   # AgentDefinition struct
├── registry.rs     # AgentRegistry for CRUD
└── mod.rs          # Barrel exports
```

## KEY TYPES

| Type              | Fields                                                      |
| ----------------- | ----------------------------------------------------------- |
| `AgentDefinition` | name, system_prompt, agent_type, severity_rules?, provider? |
| `SeverityRules`   | max_severity?, default_confidence?                          |

## STORAGE

- Stored in settings table with `custom_agent_` prefix
- JSON-serialized `AgentDefinition` as value
- Loaded via `queries::get_settings_by_prefix(conn, "custom_agent_")`

## CONVENTIONS

- `AgentRegistry` wraps settings CRUD for type safety
- Empty name or system_prompt → skip with warning
- Empty agent_type → skip with warning
- Sorted by name for deterministic output
