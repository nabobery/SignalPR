import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ShieldAlert, Check, X, CheckCheck } from "lucide-react";
import {
  resolveCodexApproval,
  resolveCopilotPermission,
  resolveOpenCodePermission,
} from "../../lib/ipc";
import type {
  ClaudeCodePermissionRequest,
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
  | { source: "cursor"; request: CursorPermissionRequest }
  | { source: "claude_code"; request: ClaudeCodePermissionRequest };

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
    const unlistenClaudeCode = listen<ClaudeCodePermissionRequest>(
      "claude_code_permission_requested",
      (event) => {
        setQueue((prev) => [...prev, { source: "claude_code", request: event.payload }]);
      },
    );
    return () => {
      unlistenCodex.then((fn) => fn());
      unlistenCopilot.then((fn) => fn());
      unlistenOpenCode.then((fn) => fn());
      unlistenGemini.then((fn) => fn());
      unlistenCursor.then((fn) => fn());
      unlistenClaudeCode.then((fn) => fn());
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
        const tool = item.request.tool_call as Record<string, unknown> | undefined;
        const title = typeof tool?.title === "string" ? (tool.title as string) : undefined;
        const kind = typeof tool?.kind === "string" ? (tool.kind as string) : undefined;
        const name = typeof tool?.name === "string" ? (tool.name as string) : undefined;
        return title ?? kind ?? name;
      }
      case "claude_code":
        return item.request.tool_name;
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
      case "claude_code":
        return `Lane: ${item.request.lane_id.slice(0, 8)}...`;
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
      case "claude_code":
        return `Tool '${item.request.tool_name}' denied — ${item.request.reason}`;
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
    claude_code: "Claude Code",
  };
  const isObservational =
    current.source === "gemini" || current.source === "cursor" || current.source === "claude_code";
  const title = isObservational
    ? `${providerLabel[current.source]} Tool Request (Denied)`
    : `${providerLabel[current.source]} Approval Required`;

  const isOpenCode = current.source === "opencode";
  const observationalModeLabel = current.source === "cursor" ? "ask" : "plan";
  const observationalProvider = providerLabel[current.source];

  const btnPrimary =
    "flex items-center gap-1 bg-[--color-accent] text-white px-3 py-1.5 rounded-md text-xs font-medium hover:bg-[--color-accent-hover] flex-1 justify-center transition-colors";
  const btnSecondary =
    "flex items-center gap-1 bg-[--color-elevated] text-[--color-text-secondary] px-3 py-1.5 rounded-md text-xs font-medium hover:text-[--color-text-primary] flex-1 justify-center transition-colors";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70">
      <div className="bg-[--color-surface] border border-[--color-border] rounded-xl shadow-2xl w-full max-w-md mx-4">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-[--color-border-subtle]">
          <ShieldAlert
            className={`w-4 h-4 ${isObservational ? "text-[--color-text-tertiary]" : "text-[--color-sev-warning]"}`}
          />
          <h3 className="text-sm font-semibold text-[--color-text-primary]">{title}</h3>
          {queue.length > 1 && (
            <span className="ml-auto text-xs text-[--color-text-tertiary]">
              {queue.length} pending
            </span>
          )}
        </div>

        <div className="px-4 py-3 space-y-2">
          <p className="text-xs text-[--color-text-secondary]">{description}</p>

          {command && (
            <div className="bg-[--color-base] rounded-md p-2 font-mono text-xs text-[--color-text-secondary] break-all border border-[--color-border-subtle]">
              {command}
            </div>
          )}

          {cwd && <p className="text-xs text-[--color-text-tertiary] truncate">in {cwd}</p>}
          <p className="text-xs text-[--color-text-tertiary]">{identifier}</p>
        </div>

        {isObservational && (
          <div className="px-4 pb-3">
            <p className="text-xs text-[--color-text-tertiary]">
              A future release will let you review and allow individual requests. For now,{" "}
              {observationalProvider} review lanes run under {observationalModeLabel} mode with
              deny-by-default tool permissions.
            </p>
          </div>
        )}

        <div className="flex gap-2 px-4 py-3 border-t border-[--color-border-subtle]">
          {isObservational ? (
            <button onClick={() => setQueue((prev) => prev.slice(1))} className={btnSecondary}>
              <Check className="w-3 h-3" />
              Acknowledge
            </button>
          ) : isOpenCode ? (
            <>
              <button onClick={() => handleDecision("once")} className={btnPrimary}>
                <Check className="w-3 h-3" /> Allow Once
              </button>
              <button onClick={() => handleDecision("always")} className={btnSecondary}>
                <CheckCheck className="w-3 h-3" /> Always
              </button>
              <button onClick={() => handleDecision("reject")} className={btnSecondary}>
                <X className="w-3 h-3" /> Reject
              </button>
            </>
          ) : (
            <>
              <button onClick={() => handleDecision("accept")} className={btnPrimary}>
                <Check className="w-3 h-3" /> Accept
              </button>
              <button onClick={() => handleDecision("decline")} className={btnSecondary}>
                <X className="w-3 h-3" /> Decline
              </button>
              <button
                onClick={() => handleDecision("cancel")}
                className="text-xs text-[--color-text-tertiary] hover:text-[--color-text-secondary] px-2 transition-colors"
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
