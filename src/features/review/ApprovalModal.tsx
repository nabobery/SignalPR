import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ShieldAlert, Check, X, CheckCheck } from "lucide-react";
import {
  resolveCodexApproval,
  resolveCopilotPermission,
  resolveOpenCodePermission,
} from "../../lib/ipc";
import type {
  CodexApprovalRequest,
  CopilotPermissionRequest,
  CursorPermissionRequest,
  GeminiPermissionRequest,
  OpenCodePermissionRequest,
} from "../../lib/types";

type QueueItem =
  | { source: "codex"; request: CodexApprovalRequest }
  | { source: "copilot"; request: CopilotPermissionRequest }
  | { source: "opencode"; request: OpenCodePermissionRequest }
  | { source: "gemini"; request: GeminiPermissionRequest }
  | { source: "cursor"; request: CursorPermissionRequest };

export function ApprovalModal() {
  const [queue, setQueue] = useState<QueueItem[]>([]);

  useEffect(() => {
    const unlistenCodex = listen<CodexApprovalRequest>("codex_approval_requested", (event) => {
      setQueue((prev) => [...prev, { source: "codex", request: event.payload }]);
    });
    const unlistenCopilot = listen<CopilotPermissionRequest>(
      "copilot_permission_requested",
      (event) => {
        setQueue((prev) => [...prev, { source: "copilot", request: event.payload }]);
      },
    );
    const unlistenOpenCode = listen<OpenCodePermissionRequest>(
      "opencode_permission_requested",
      (event) => {
        setQueue((prev) => [...prev, { source: "opencode", request: event.payload }]);
      },
    );
    const unlistenGemini = listen<GeminiPermissionRequest>(
      "gemini_permission_requested",
      (event) => {
        setQueue((prev) => [...prev, { source: "gemini", request: event.payload }]);
      },
    );
    const unlistenCursor = listen<CursorPermissionRequest>(
      "cursor_permission_requested",
      (event) => {
        setQueue((prev) => [...prev, { source: "cursor", request: event.payload }]);
      },
    );
    return () => {
      unlistenCodex.then((fn) => fn());
      unlistenCopilot.then((fn) => fn());
      unlistenOpenCode.then((fn) => fn());
      unlistenGemini.then((fn) => fn());
      unlistenCursor.then((fn) => fn());
    };
  }, []);

  if (queue.length === 0) return null;

  const current = queue[0];

  const getCommand = (item: QueueItem): string | undefined => {
    switch (item.source) {
      case "codex":
        return item.request.params?.command as string | undefined;
      case "copilot":
        return item.request.command ?? undefined;
      case "opencode":
        return item.request.metadata?.command as string | undefined;
      case "gemini":
      case "cursor": {
        // ACP tool-call shape:
        //   https://agentclientprotocol.com/protocol/tool-calls
        // `title` is the preferred UI hint; fall back to `kind` and
        // then `name` so we never show an empty card on an unexpected
        // shape.
        const tool = item.request.tool_call as Record<string, unknown> | undefined;
        const title = typeof tool?.title === "string" ? (tool.title as string) : undefined;
        const kind = typeof tool?.kind === "string" ? (tool.kind as string) : undefined;
        const name = typeof tool?.name === "string" ? (tool.name as string) : undefined;
        return title ?? kind ?? name;
      }
    }
  };

  const getCwd = (item: QueueItem): string | undefined =>
    item.source === "codex" ? (item.request.params?.cwd as string | undefined) : undefined;

  const getIdentifier = (item: QueueItem): string => {
    switch (item.source) {
      case "codex":
        return `Lane: ${item.request.thread_id.slice(0, 8)}... / Turn: ${item.request.turn_id.slice(0, 8)}...`;
      case "copilot":
        return `Session: ${item.request.session_id.slice(0, 8)}... / ${item.request.kind}`;
      case "opencode":
        return `Session: ${item.request.session_id.slice(0, 8)}...${item.request.tool ? ` / ${item.request.tool}` : ""}`;
      case "gemini":
        return `Session: ${item.request.session_id.slice(0, 8)}...`;
      case "cursor":
        return `Session: ${item.request.session_id.slice(0, 8)}...`;
    }
  };

  const getDescription = (item: QueueItem): string => {
    switch (item.source) {
      case "codex":
        return item.request.method.replace("item/", "").replace("/requestApproval", "");
      case "copilot":
        return `Permission: ${item.request.kind}${item.request.file_name ? ` (${item.request.file_name})` : ""}`;
      case "opencode":
        return `Permission: ${item.request.permission}${item.request.patterns.length > 0 ? ` (${item.request.patterns.join(", ")})` : ""}`;
      case "gemini":
      case "cursor": {
        const tool = item.request.tool_call as Record<string, unknown> | undefined;
        const kind = typeof tool?.kind === "string" ? (tool.kind as string) : undefined;
        const mode = item.source === "cursor" ? "ask" : "plan";
        const label = kind ? `Tool request (${kind})` : "Tool request";
        return `${label} denied — SignalPR runs in ${mode} mode (review-only)`;
      }
    }
  };

  const command = getCommand(current);
  const cwd = getCwd(current);
  const identifier = getIdentifier(current);
  const description = getDescription(current);

  const handleDecision = async (decision: string) => {
    if (current.source === "codex") {
      await resolveCodexApproval(current.request.request_id, decision);
    } else if (current.source === "copilot") {
      const v3Decision = decision === "accept" ? "approved" : "denied-interactively-by-user";
      await resolveCopilotPermission(
        current.request.session_id,
        current.request.event_id,
        v3Decision,
      );
    } else if (current.source === "opencode") {
      // OpenCode uses once/always/reject
      await resolveOpenCodePermission(current.request.request_id, decision);
    }
    // Gemini/Cursor items are observational (backend already denied); no IPC call.
    setQueue((prev) => prev.slice(1));
  };

  const providerLabel: Record<QueueItem["source"], string> = {
    codex: "Codex",
    copilot: "Copilot",
    opencode: "OpenCode",
    gemini: "Gemini",
    cursor: "Cursor",
  };
  const isObservational = current.source === "gemini" || current.source === "cursor";
  const title = isObservational
    ? `${providerLabel[current.source]} Tool Request (Denied)`
    : `${providerLabel[current.source]} Approval Required`;

  const isOpenCode = current.source === "opencode";
  const observationalModeLabel = current.source === "cursor" ? "ask" : "plan";
  const observationalProvider = providerLabel[current.source];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl w-full max-w-md mx-4">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-zinc-800">
          <ShieldAlert
            className={`w-4 h-4 ${isObservational ? "text-zinc-500" : "text-yellow-400"}`}
          />
          <h3 className="text-sm font-semibold text-zinc-100">{title}</h3>
          {queue.length > 1 && (
            <span className="ml-auto text-xs text-zinc-500">{queue.length} pending</span>
          )}
        </div>

        <div className="px-4 py-3 space-y-2">
          <p className="text-xs text-zinc-400">{description}</p>

          {command && (
            <div className="bg-zinc-950 rounded-lg p-2 font-mono text-xs text-zinc-300 break-all">
              {command}
            </div>
          )}

          {cwd && <p className="text-xs text-zinc-500 truncate">in {cwd}</p>}

          <p className="text-xs text-zinc-500">{identifier}</p>
        </div>

        {isObservational && (
          <div className="px-4 pb-3">
            <p className="text-xs text-zinc-500">
              A future release will let you review and allow individual requests. For now,{" "}
              {observationalProvider} review lanes run under {observationalModeLabel} mode with
              deny-by-default tool permissions.
            </p>
          </div>
        )}

        <div className="flex gap-2 px-4 py-3 border-t border-zinc-800">
          {isObservational ? (
            <button
              onClick={() => setQueue((prev) => prev.slice(1))}
              className="flex items-center gap-1 bg-zinc-700 text-zinc-200 px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-zinc-600 flex-1 justify-center"
            >
              <Check className="w-3 h-3" />
              Acknowledge
            </button>
          ) : isOpenCode ? (
            <>
              <button
                onClick={() => handleDecision("once")}
                className="flex items-center gap-1 bg-emerald-600 text-white px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-emerald-500 flex-1 justify-center"
              >
                <Check className="w-3 h-3" />
                Allow Once
              </button>
              <button
                onClick={() => handleDecision("always")}
                className="flex items-center gap-1 bg-blue-600 text-white px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-blue-500 flex-1 justify-center"
              >
                <CheckCheck className="w-3 h-3" />
                Always
              </button>
              <button
                onClick={() => handleDecision("reject")}
                className="flex items-center gap-1 bg-zinc-700 text-zinc-200 px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-zinc-600 flex-1 justify-center"
              >
                <X className="w-3 h-3" />
                Reject
              </button>
            </>
          ) : (
            <>
              <button
                onClick={() => handleDecision("accept")}
                className="flex items-center gap-1 bg-emerald-600 text-white px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-emerald-500 flex-1 justify-center"
              >
                <Check className="w-3 h-3" />
                Accept
              </button>
              <button
                onClick={() => handleDecision("decline")}
                className="flex items-center gap-1 bg-zinc-700 text-zinc-200 px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-zinc-600 flex-1 justify-center"
              >
                <X className="w-3 h-3" />
                Decline
              </button>
              <button
                onClick={() => handleDecision("cancel")}
                className="text-xs text-zinc-500 hover:text-zinc-300 px-2"
              >
                Cancel turn
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
