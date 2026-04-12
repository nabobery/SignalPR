# SignalPR

**A reviewer-first desktop application that transforms noisy AI pull request analysis into compact, human-approved review drafts.**

---

## The Problem

AI-powered code review tools create a new problem instead of solving one: they generate too much noise.

- They repeat the same observation across every file it appears in
- They surface weak, speculative, or irrelevant comments at the same priority as real issues
- They post directly to GitHub without human oversight, polluting PR timelines
- They lack architectural context, so findings are generic and shallow
- They create a second filtering job on top of the review job

The result: "AI-assisted review" ends up slower than doing it yourself.

## The Solution

SignalPR sits between your AI tools and GitHub. It runs specialized analysis agents in parallel, passes raw findings through a multi-stage cleaner pipeline that deduplicates, ranks, and verifies them, and then opens an editable workspace so you can approve what gets posted.

**Nothing is sent to GitHub without your sign-off.**

The core insight is that the problem isn't AI quality — it's the missing layer between raw AI output and the final review. SignalPR is that layer.

### How it works

```
Paste PR URL
     │
     ▼
Fetch diff + metadata (via gh CLI)
     │
     ▼
┌────────────────────────────────┐
│  Parallel agent lanes          │
│  ├── Security                  │
│  ├── Architecture              │
│  └── Performance               │
└────────────────────────────────┘
     │
     ▼
┌────────────────────────────────┐
│  Cleaner pipeline              │
│  1. Normalize                  │
│  2. Deduplicate                │
│  3. Rank by confidence         │
│  4. Verify line anchors        │
│  5. Remap to current diff      │
│  6. Cluster + synthesize       │
└────────────────────────────────┘
     │
     ▼
Review workspace (edit, suppress, rewrite)
     │
     ▼
Single batched review posted to GitHub
```

---

## Key Features

**Multi-lane orchestration** — Security, architecture, and performance agents run in parallel with specialized prompts rather than a single generic "review this" instruction.

**Cleaner pipeline** — The core differentiator. Raw findings go through six stages: normalization, deduplication, confidence ranking, diff anchor verification, anchor remapping, and semantic clustering. Fifty raw comments collapse to eight high-signal findings.

**Human-in-the-loop workspace** — A full review workspace with a file tree, diff view, and Signal Board. Edit finding text, change severity, suppress noise, or rewrite comments before anything reaches GitHub.

**Multi-provider support** — Works with Codex (CLI and App Server), GitHub Copilot, OpenCode, Claude, and Gemini. Providers stream findings to the UI in real time.

**Local-first and private** — No SignalPR cloud backend. State is stored in local SQLite. Credentials stay in your OS keychain. The app only routes code to the providers you explicitly choose.

**Repo-level config** — Drop a `.signalpr.yml` in any repo to configure which agents run, the deduplication threshold, the maximum surfaced findings, and more.

---

## Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri v2 |
| Frontend | React 19 + TypeScript + Vite |
| Routing | React Router 7 |
| Styling | Tailwind CSS v4 + Lucide React |
| Backend | Rust (async with Tokio) |
| IPC | Tauri commands + events |
| Persistence | SQLite (rusqlite) |
| Credentials | OS keychain (keyring) |
| AI providers | Codex, GitHub Copilot, OpenCode, Claude, Gemini |
| PR fetching | GitHub CLI (`gh`) |
| Streaming | WebSocket (tokio-tungstenite) + SSE |
| HTTP client | reqwest |
| Testing | Vitest (frontend) + cargo test (backend) |

---

## Project Structure

```
signalpr/
├── src/                          # React frontend
│   ├── App.tsx                   # Route definitions
│   ├── features/
│   │   ├── intake/               # PR URL input and session creation
│   │   ├── onboarding/           # Tool detection and setup guidance
│   │   ├── review/               # Main review workspace
│   │   ├── settings/             # User preferences and agent config
│   │   └── submission/           # Review submission UI
│   ├── lib/
│   │   ├── ipc.ts                # Typed Tauri invoke() wrappers
│   │   ├── types.ts              # Shared TypeScript interfaces
│   │   └── store.ts              # Review workspace state (React context)
│   └── ui/                       # Shared UI components
│
└── src-tauri/                    # Rust backend
    └── src/
        ├── commands/             # Tauri IPC command handlers
        ├── orchestration/        # Multi-lane review engine and state machine
        │   ├── engine.rs         # Coordinates parallel agent execution
        │   ├── lane.rs           # Per-lane status tracking
        │   └── state.rs          # ReviewRun state machine
        ├── cleaner/              # Six-stage finding post-processing
        │   ├── dedup.rs
        │   ├── normalize.rs
        │   ├── rank.rs
        │   ├── verify.rs
        │   ├── remap.rs
        │   └── synthesis.rs
        ├── providers/            # AI provider adapters
        │   ├── traits.rs         # ReviewProvider trait
        │   ├── codex.rs          # Codex one-shot CLI
        │   ├── codex_app_server/ # Persistent Codex via JSON-RPC
        │   ├── claude.rs         # Anthropic API
        │   ├── copilot/          # GitHub Copilot v3 JSON-RPC
        │   ├── opencode/         # OpenCode HTTP + SSE
        │   ├── gemini/           # Gemini CLI via ACP (JSON-RPC over stdio)
        │   └── github.rs         # PR metadata fetching
        ├── storage/              # SQLite data layer
        ├── config/               # Configuration loading (.signalpr.yml)
        ├── channels/             # Slack/Discord notification integrations
        └── agents/               # Custom agent registry
```

---

## Prerequisites

- **[Rust](https://rustup.rs/)** — stable toolchain
- **[Node.js](https://nodejs.org/)** (18+) and **[pnpm](https://pnpm.io/)**
- **[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)** for your OS (WebView2 on Windows, Xcode tools on macOS, webkit2gtk on Linux)
- **[GitHub CLI](https://cli.github.com/)** (`gh`) — authenticated with `gh auth login`
- At least one review provider:
  - [Codex CLI](https://github.com/openai/codex) or Codex App Server
  - [OpenCode](https://opencode.ai/)
  - `ANTHROPIC_API_KEY` in your environment (for Claude)
  - GitHub Copilot subscription (for Copilot provider)
  - [Gemini CLI](https://github.com/google-gemini/gemini-cli) (`npm i -g @google/gemini-cli`) with `GEMINI_API_KEY` set (for Gemini; API key required — OAuth is not supported)

---

## Setup and Development

```bash
# Clone the repo
git clone git@github.com:nabobery/SignalPR.git
cd SignalPR

# Install frontend dependencies
pnpm install

# Start the development build (Vite + Tauri hot reload)
pnpm tauri dev
```

This starts the Vite dev server on `localhost:1420` and opens the Tauri window with hot module replacement on the frontend and auto-rebuild on Rust changes.

---

## Available Scripts

| Command | Description |
|---|---|
| `pnpm tauri dev` | Start the full app in development mode |
| `pnpm tauri build` | Build a production-ready native binary |
| `pnpm test` | Run all tests (Rust + frontend) |
| `pnpm test:frontend` | Run Vitest tests only |
| `pnpm test:rust` | Run Rust tests only |
| `pnpm check` | Full quality check (typecheck + lint + format + tests) |
| `pnpm typecheck` | TypeScript type check |
| `pnpm lint` | ESLint |
| `pnpm lint:fix` | ESLint with auto-fix |
| `pnpm lint:rust` | Cargo clippy |
| `pnpm format` | Prettier (frontend) + cargo fmt (Rust) |

---

## Repo Configuration

Create a `.signalpr.yml` at the root of any repository to customize review behavior for that project:

```yaml
version: 1
agents:
  security: true
  architecture: true
  performance: true
  nitpick: false           # Off by default — too noisy
cleaner:
  similarity_threshold: 0.85
  drop_nitpicks: true
  max_surface_findings: 8  # Cap how many findings reach the workspace
submission:
  require_human_approval: true
```

Configuration precedence: **repo config > user config > defaults**

---

## Architecture Notes

**IPC boundary** — The frontend never calls `invoke()` directly. All Tauri commands are wrapped in typed functions in [src/lib/ipc.ts](src/lib/ipc.ts). Backend events (`review_progress`, `codex_lane_delta`, `codex_approval_requested`) are emitted to the frontend for real-time progress updates.

**Provider adapter pattern** — Every review provider implements the `ReviewProvider` trait in [src-tauri/src/providers/traits.rs](src-tauri/src/providers/traits.rs). Adding a new provider means implementing that trait — the orchestration engine does not need to change.

**Review run state machine** — A `ReviewRun` moves through: `Created → RunningAgents → Cleaning → ReadyForReview → Submitting → Submitted` (with `Failed` as a terminal error state from any stage). This makes recovery from restarts deterministic.

**Credential storage** — Secrets are stored in the OS keychain via `keyring`. Nothing sensitive is written to SQLite or log files.

---

## Contributing

1. Fork the repo and create a branch from `main`.
2. Run `pnpm check` before opening a PR — this runs the full quality gate (typecheck, lint, format, tests).
3. Keep frontend and backend concerns cleanly separated: React components talk to the backend exclusively through [src/lib/ipc.ts](src/lib/ipc.ts).
4. New AI providers belong in `src-tauri/src/providers/` and must implement the `ReviewProvider` trait.
5. New cleaner stages belong in `src-tauri/src/cleaner/` and should be independently testable with unit tests.
6. The cleaner pipeline and orchestration engine have in-memory SQLite test fixtures — add tests there rather than mocking at the integration layer.
7. Open an issue first for large changes so the direction can be agreed on before implementation.

---

## License

MIT
