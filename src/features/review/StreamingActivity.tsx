import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Activity } from "lucide-react";
import type {
  CodexLaneDelta,
  CopilotLaneDelta,
  CursorLaneDelta,
  GeminiLaneDelta,
  OpenCodeLaneDelta,
} from "../../lib/types";

interface Props {
  laneId: string;
}

export function StreamingActivity({ laneId }: Props) {
  const [lastLine, setLastLine] = useState<string>("");

  useEffect(() => {
    let debounce: ReturnType<typeof setTimeout> | null = null;

    const handleDelta = (payload: { lane_id: string; buffer: string }) => {
      if (payload.lane_id !== laneId) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => {
        const lines = payload.buffer.split("\n").filter((l) => l.trim());
        const last = lines[lines.length - 1] ?? "";
        setLastLine(last.length > 120 ? last.slice(0, 120) + "..." : last);
      }, 100);
    };

    const unlistenCodex = listen<CodexLaneDelta>("codex_lane_delta", (event) => {
      handleDelta(event.payload);
    });

    const unlistenCopilot = listen<CopilotLaneDelta>("copilot_lane_delta", (event) => {
      handleDelta(event.payload);
    });

    const unlistenOpenCode = listen<OpenCodeLaneDelta>("opencode_lane_delta", (event) => {
      handleDelta(event.payload);
    });

    const unlistenGemini = listen<GeminiLaneDelta>("gemini_lane_delta", (event) => {
      handleDelta(event.payload);
    });

    const unlistenCursor = listen<CursorLaneDelta>("cursor_lane_delta", (event) => {
      handleDelta(event.payload);
    });

    return () => {
      if (debounce) clearTimeout(debounce);
      unlistenCodex.then((fn) => fn());
      unlistenCopilot.then((fn) => fn());
      unlistenOpenCode.then((fn) => fn());
      unlistenGemini.then((fn) => fn());
      unlistenCursor.then((fn) => fn());
    };
  }, [laneId]);

  if (!lastLine) return null;

  return (
    <div className="flex items-start gap-1.5 mt-1">
      <Activity className="w-3 h-3 text-zinc-500 mt-0.5 shrink-0" />
      <p className="text-xs text-zinc-500 font-mono truncate">{lastLine}</p>
    </div>
  );
}
