import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { ShieldAlert, Check, X } from "lucide-react";
import { resolveCodexApproval, resolveCopilotPermission } from "../../lib/ipc";
import type { CodexApprovalRequest, CopilotPermissionRequest } from "../../lib/types";

type QueueItem =
  | { source: "codex"; request: CodexApprovalRequest }
  | { source: "copilot"; request: CopilotPermissionRequest };

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
    return () => {
      unlistenCodex.then((fn) => fn());
      unlistenCopilot.then((fn) => fn());
    };
  }, []);

  if (queue.length === 0) return null;

  const current = queue[0];

  const command =
    current.source === "codex"
      ? (current.request.params?.command as string | undefined)
      : (current.request.command ?? undefined);

  const cwd =
    current.source === "codex" ? (current.request.params?.cwd as string | undefined) : undefined;

  const identifier =
    current.source === "codex"
      ? `Lane: ${current.request.thread_id.slice(0, 8)}... / Turn: ${current.request.turn_id.slice(0, 8)}...`
      : `Session: ${current.request.session_id.slice(0, 8)}... / ${current.request.kind}`;

  const description =
    current.source === "codex"
      ? current.request.method.replace("item/", "").replace("/requestApproval", "")
      : `Permission: ${current.request.kind}${current.request.file_name ? ` (${current.request.file_name})` : ""}`;

  const handleDecision = async (decision: string) => {
    if (current.source === "codex") {
      await resolveCodexApproval(current.request.request_id, decision);
    } else {
      // Map to v3 decision values
      const v3Decision = decision === "accept" ? "approved" : "denied-interactively-by-user";
      await resolveCopilotPermission(
        current.request.session_id,
        current.request.event_id,
        v3Decision,
      );
    }
    setQueue((prev) => prev.slice(1));
  };

  const providerLabel = current.source === "codex" ? "Codex" : "Copilot";

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="bg-zinc-900 border border-zinc-700 rounded-xl shadow-2xl w-full max-w-md mx-4">
        <div className="flex items-center gap-2 px-4 py-3 border-b border-zinc-800">
          <ShieldAlert className="w-4 h-4 text-yellow-400" />
          <h3 className="text-sm font-semibold text-zinc-100">{providerLabel} Approval Required</h3>
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

        <div className="flex gap-2 px-4 py-3 border-t border-zinc-800">
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
        </div>
      </div>
    </div>
  );
}
