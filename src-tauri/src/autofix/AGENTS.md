# Autofix Module

**Auto-fix patch generation** — search/replace → unified diff workflow.

## STRUCTURE

```
autofix/
├── patch.rs    # FixSuggestion + unified diff conversion
├── apply.rs    # Patch application to files
└── mod.rs      # Barrel exports
```

## KEY TYPES

| Type            | Purpose                            |
| --------------- | ---------------------------------- |
| `FixSuggestion` | Search/replace pair with file path |

## WORKFLOW

```
1. LLM returns FixSuggestion (search, replace, file_path)
2. search_replace_to_unified_diff() → unified diff string
3. apply_patch() writes changes to workspace
```

## CONVENTIONS

- `search_replace_to_unified_diff()` returns `Option<String>` (None if search not found)
- Unified diff format: `--- a/{path}` / `+++ b/{path}` / `@@ -L,C +L,C @@`
- Apply uses file content from workspace path
- Tests validate diff format and missing search text
