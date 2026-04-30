# Frontend Features

## OVERVIEW

Feature-area entrypoint for user-facing review workflows and settings.
Each feature folder is intentionally isolated by domain with dedicated components, tests, and shared utility imports from `../../lib`.

## STRUCTURE

```
features/
├── intake/       # PR URL input and workspace selection
├── onboarding/   # CLI/environment readiness checks
├── review/       # Review workspace and diff renderer
├── settings/     # General/Presets/Agents/Channels UI and persistence entry
└── submission/   # Submit dialog and confirmation flow
```

## WHERE TO LOOK

| Feature | Entry | Notes |
| ------- | ----- | ----- |
| `review` | `ReviewWorkspace.tsx` | Main workspace orchestration and state propagation |
| `settings` | `SettingsView.tsx` | IPC-driven configuration update flows |
| `submission` | `SubmitDialog.tsx` | Review submission pipeline handoff |

## CONVENTIONS

- Shared types come from `src/lib/types.ts`.
- Shared IPC wrappers come from `src/lib/ipc.ts` (no raw `invoke` in feature code).
- Feature-level components should keep event listener cleanup explicit and return `unlisten` handlers.
- Keep error and loading states intentionally explicit in full-screen or full-panel views.
- Use existing table/markdown/text conventions already established in sibling feature modules.

## ANTI-PATTERNS

- Don't mutate store-like state objects inline; always use React state setters with immutability.
- Don't introduce feature-specific fetch logic in nested components; route through IPC wrappers in `src/lib/ipc.ts`.
- Don't add review-flow dependencies into non-review features without clear boundaries.

