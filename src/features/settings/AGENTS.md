# Settings Feature

**User settings UI** — tabbed interface for configuration.

## STRUCTURE

```
settings/
├── SettingsView.tsx   # Tab container (General/Presets/Agents/Channels)
├── GeneralPanel.tsx   # General settings (lanes, provider, etc.)
├── PresetPanel.tsx    # Preset configuration
├── AgentPanel.tsx     # Custom agent management
├── AgentForm.tsx      # Agent create/edit form
├── ChannelPanel.tsx   # Discord/Slack webhook config
└── (no AGENTS.md previously)
```

## TABS

| Tab      | Component    | IPC Commands Used                    |
| -------- | ------------ | ------------------------------------ |
| General  | GeneralPanel | get_settings, update_setting         |
| Presets  | PresetPanel  | get_settings, update_setting         |
| Agents   | AgentPanel   | get_agents, save_agent, delete_agent |
| Channels | ChannelPanel | get_settings, get_channel_statuses   |

## GENERAL PANEL NOTES

- `preferred_provider` options: `auto | codex | claude | copilot | opencode | gemini | cursor`
- Selecting `gemini` renders an inline amber warning about API-key-only auth (`GEMINI_API_KEY` or Vertex `GOOGLE_*`); links to Gemini CLI ToS and auth guide
- Selecting `cursor` renders inline instructions for obtaining `CURSOR_API_KEY` (Cloud Agents → User API Keys at cursor.com/dashboard)

## CONVENTIONS

- Dark theme (bg-zinc-950, text-zinc-100) — consistent with app
- Active tab uses emerald accent (text-emerald-400, border-emerald-500)
- Back button navigates to `/` via react-router
- `ipc.ts` wrappers for all Tauri calls (never inline invoke)
