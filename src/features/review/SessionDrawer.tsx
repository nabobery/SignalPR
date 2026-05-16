import { useEffect, useState } from "react";
import { Database, Clock, DollarSign, ChevronDown, ChevronRight } from "lucide-react";
import { getAgentRunMetadata, getProviderCapabilities, parseError } from "../../lib/ipc";
import type {
  AgentRunMetadata,
  AgentRunMetadataResponse,
  ProviderCapabilities,
  ProviderSelectionTrace,
} from "../../lib/types";

interface Props {
  runId: string;
}

export function SessionDrawer({ runId }: Props) {
  const [runs, setRuns] = useState<AgentRunMetadata[]>([]);
  const [caps, setCaps] = useState<ProviderCapabilities[]>([]);
  const [selection, setSelection] = useState<ProviderSelectionTrace | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [metadata, capabilities] = await Promise.all([
          getAgentRunMetadata(runId),
          getProviderCapabilities(),
        ]);
        if (cancelled) return;
        const response = metadata as AgentRunMetadataResponse;
        setRuns(response.runs);
        setSelection(response.provider_selection ?? null);
        setCaps(capabilities);
      } catch (err) {
        if (!cancelled) setError(parseError(err).message);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [runId]);

  if (error) {
    return <p className="text-xs text-red-400 px-2">{error}</p>;
  }

  if (runs.length === 0) return null;

  const hasSessionData = runs.some((r) => r.provider_session_id);

  return (
    <div className="border-t border-zinc-800 mt-2">
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-1.5 px-3 py-2 text-xs text-zinc-400 hover:text-zinc-200 w-full"
      >
        {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
        <Database className="w-3 h-3" />
        Session Info
        {hasSessionData && <span className="ml-1 text-emerald-500">●</span>}
      </button>

      {expanded && (
        <div className="px-3 pb-3 space-y-2">
          {selection && (
            <div className="bg-zinc-900 rounded-lg p-2 text-xs space-y-1">
              <div className="flex items-center justify-between">
                <span className="text-zinc-300 font-medium">Provider selection</span>
                <span className="text-[10px] uppercase tracking-wide text-zinc-500">
                  {selection.selection_mode}
                </span>
              </div>
              <p className="text-zinc-500">
                Requested <span className="text-zinc-300">{selection.requested_provider}</span>,
                selected <span className="text-zinc-300">{selection.selected_provider}</span>
              </p>
              {selection.warnings.map((warning) => (
                <p key={warning} className="text-amber-400 text-[10px]">
                  {warning}
                </p>
              ))}
            </div>
          )}
          {runs.map((run) => {
            const providerKey = normalizeProviderId(run.provider_name);
            const providerCaps = caps.find((c) => c.provider_id === providerKey);
            return (
              <div key={run.id} className="bg-zinc-900 rounded-lg p-2 text-xs space-y-1">
                <div className="flex items-center justify-between">
                  <span className="text-zinc-300 font-medium">
                    {run.lane_id} — {run.provider_name}
                  </span>
                  <span
                    className={`px-1.5 py-0.5 rounded text-[10px] ${
                      run.status === "completed"
                        ? "bg-emerald-900/50 text-emerald-300"
                        : run.status === "failed"
                          ? "bg-red-900/50 text-red-300"
                          : "bg-zinc-800 text-zinc-400"
                    }`}
                  >
                    {run.status}
                  </span>
                </div>

                {run.governance_tier_at_run && (
                  <p className="text-zinc-500">
                    Tier: <span className="text-zinc-300">{run.governance_tier_at_run}</span>
                  </p>
                )}

                {run.provider_session_id && (
                  <p className="text-zinc-500 truncate">
                    Session:{" "}
                    <span className="text-zinc-300 font-mono">{run.provider_session_id}</span>
                  </p>
                )}

                {run.cost_usd != null && (
                  <p className="text-zinc-500 flex items-center gap-1">
                    <DollarSign className="w-3 h-3" />${run.cost_usd.toFixed(4)}
                  </p>
                )}

                {run.started_at && (
                  <p className="text-zinc-500 flex items-center gap-1">
                    <Clock className="w-3 h-3" />
                    {new Date(run.started_at).toLocaleTimeString()}
                  </p>
                )}

                {providerCaps?.supports_session_resume && run.provider_session_id && (
                  <p className="text-amber-400 text-[10px] mt-1">
                    Resume capability available (feature flag required)
                  </p>
                )}

                {providerCaps?.supports_checkpointing && run.checkpoint_metadata_json && (
                  <p className="text-amber-400 text-[10px]">
                    File checkpoints stored — bash/manual changes not covered
                  </p>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

function normalizeProviderId(value: string) {
  switch (value) {
    case "codex-app-server":
      return "codex_app_server";
    default:
      return value;
  }
}
