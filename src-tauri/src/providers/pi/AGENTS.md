# PI Provider

**PI Agent SDK** — local-first AI provider via `pi --mode rpc` (LF-delimited JSONL).

## OVERVIEW

Wraps the `@mariozechner/pi-coding-agent` CLI in a persistent `--mode rpc` subprocess.
Single-session: only one review lane runs at a time, serialized via `session_guard`.
No structured-output tools — PI returns free-text; the provider extracts the outermost JSON object.

## STRUCTURE

```
pi/
├── mod.rs       # Barrel exports
├── provider.rs  # ReviewProvider impl, prompt building, JSON extraction
└── manager.rs   # Persistent pi --mode rpc process lifecycle + JSONL protocol
```

## PROTOCOL: LF-Delimited JSONL

PI uses a custom RPC protocol — **not** JSON-RPC 2.0:

| Field | Notes |
|-------|-------|
| Discriminator | `type` field (not `command`) |
| Streaming text | `message_update` events; delta at `assistantMessageEvent.delta` when `assistantMessageEvent.type == "text_delta"` |
| Run complete | `agent_end` event (**not** `turn_end` — that's per-turn) |
| Response correlation | optional `id` field |

## SESSION LIFECYCLE

```
has_pi_binary()                     ← checks PATH
  → ensure_started()                ← spawns pi --mode rpc (15s timeout)
    → acquire_session_guard()       ← serializes all lanes (single-session)
      → new_session(lane_id)
        → prompt(lane_id, text)
          → wait for agent_end
            → parse_output()
              → release session_guard
```

## KEY DETAILS

| Detail            | Value                                      |
| ----------------- | ------------------------------------------ |
| Binary            | `pi --mode rpc`                            |
| Install           | `npm i -g @mariozechner/pi-coding-agent`   |
| Framing           | LF-delimited JSONL (not Content-Length)    |
| Concurrency       | Single-session — `session_guard` serializes all lanes |
| Review timeout    | 300s                                       |
| Startup timeout   | 15s                                        |
| Buffer cap        | 1 MiB per session (safety net)             |
| Error type        | `ProviderError::PiFailed`                  |
| Streaming event   | `pi_lane_delta`                            |

## OUTPUT EXTRACTION

PI lacks structured tool output — uses prompt-injected JSON extraction:

1. System prompt + JSON schema + diff concatenated into one user message
2. Response may include markdown fences or leading prose
3. `strip_code_fences()` unwraps ` ```json ... ``` ` and ` ``` ... ``` ` wrappers
4. `locate_json_object()` finds outermost `{...}` by bracket scan (tolerates surrounding prose)
5. `serde_json::from_str::<CodexReviewOutput>()` deserializes the slice

## ANTI-PATTERNS

- **Never** run concurrent lanes without `session_guard` — the PI process is single-session
- **Never** skip `new_session()` between reviews — session buffers are keyed by session ID
- **Never** treat `turn_end` as run completion — only `agent_end` signals the full agent run is done
- **Never** use Content-Length framing — PI uses bare LF-delimited JSONL
