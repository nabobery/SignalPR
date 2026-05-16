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
└── AGENTS.md          # This module guidance
```

## TABS

| Tab      | Component    | IPC Commands Used                    |
| -------- | ------------ | ------------------------------------ |
| General  | GeneralPanel | get_settings, update_setting         |
| Presets  | PresetPanel  | get_settings, update_setting         |
| Agents   | AgentPanel   | get_agents, save_agent, delete_agent |
| Channels | ChannelPanel | get_settings, get_channel_status      |

## GENERAL PANEL NOTES

- `preferred_provider` options: `auto | codex | claude | copilot | opencode | gemini | cursor | pi`
- Preferred provider setup details now come from the provider setup catalog instead of hard-coded per-provider warning blocks
- Catalog fetch should enrich the panel opportunistically; primary settings and control-plane content should remain usable from local signals even if registry fetch is slow or offline
- Curated providers can attach setup notes for guidance like Gemini API-key-only auth (`GEMINI_API_KEY` or Vertex `GOOGLE_*`) and Cursor key generation (Cloud Agents → User API Keys)

## CONVENTIONS

- Dark theme (bg-zinc-950, text-zinc-100) — consistent with app
- Active tab uses emerald accent (text-emerald-400, border-emerald-500)
- Back button navigates to `/` via react-router
- `ipc.ts` wrappers for all Tauri calls (never inline invoke)
