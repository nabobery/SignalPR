import { useMemo, useState, useCallback, useEffect } from "react";
import { FindingCard } from "./FindingCard";
import ClusterCard from "./ClusterCard";
import { useReviewContext } from "../../lib/store";
import type { Finding, FindingCluster } from "../../lib/types";

interface ClusterGroup {
  cluster: FindingCluster;
  members: Finding[];
  representative: Finding;
}

type FilterPreset =
  | "active"
  | "high-confidence"
  | "security"
  | "fresh-risk"
  | "new"
  | "stale"
  | "unchanged"
  | "suppressed"
  | "edited";

const defaultPresets: { value: FilterPreset; label: string }[] = [
  { value: "active", label: "Active" },
  { value: "high-confidence", label: "High confidence" },
  { value: "security", label: "Security" },
  { value: "suppressed", label: "Suppressed" },
  { value: "edited", label: "Edited" },
];

const rerunPresets: { value: FilterPreset; label: string }[] = [
  { value: "fresh-risk", label: "Fresh risk" },
  { value: "new", label: "New" },
  { value: "stale", label: "Stale" },
  { value: "unchanged", label: "Unchanged" },
  { value: "suppressed", label: "Suppressed" },
  { value: "edited", label: "Edited" },
];

function applyPreset(findings: Finding[], preset: FilterPreset): Finding[] {
  switch (preset) {
    case "active":
      return findings.filter((f) => f.status === "active");
    case "high-confidence":
      return findings.filter((f) => f.status === "active" && f.confidence >= 0.8);
    case "security":
      return findings.filter(
        (f) => f.status === "active" && (f.lane_id === "security" || f.agent_type === "security"),
      );
    case "fresh-risk":
      return findings.filter(
        (f) => f.status === "active" && (f.delta_state === "new" || f.delta_state === "stale"),
      );
    case "new":
      return findings.filter((f) => f.status === "active" && f.delta_state === "new");
    case "stale":
      return findings.filter((f) => f.status === "active" && f.delta_state === "stale");
    case "unchanged":
      return findings.filter((f) => f.status === "active" && f.delta_state === "unchanged");
    case "suppressed":
      return findings.filter((f) => f.status === "suppressed");
    case "edited":
      return findings.filter(
        (f) => f.user_edited_body !== null || f.user_severity_override !== null,
      );
    default:
      return findings.filter((f) => f.status === "active");
  }
}

export function SignalBoard() {
  const { state, refreshSnapshot, setSessionDecision } = useReviewContext();
  const isRerun = state.baselineRunId !== null;
  const presets = isRerun ? rerunPresets : defaultPresets;
  const [preset, setPreset] = useState<FilterPreset>(isRerun ? "fresh-risk" : "active");
  const selectedFile = state.selectedFile;

  useEffect(() => {
    setPreset(isRerun ? "fresh-risk" : "active");
  }, [isRerun, state.runId]);

  const handleDecision = useCallback(
    (findingId: string, decision: string) => {
      if (decision === "accept" || decision === "skip") {
        setSessionDecision(findingId, decision);
      }
    },
    [setSessionDecision],
  );

  const filteredFindings = useMemo(
    () => applyPreset(state.findings, preset),
    [state.findings, preset],
  );

  const displayFindings = selectedFile
    ? filteredFindings.filter((f) => f.file_path === selectedFile)
    : filteredFindings;

  // Group findings by cluster_id for rendering
  const { clusterGroups, unclustered } = useMemo(() => {
    const groups: ClusterGroup[] = [];
    const unclustered: Finding[] = [];
    const clusterMap = new Map<string, Finding[]>();

    for (const f of displayFindings) {
      if (f.cluster_id) {
        const existing = clusterMap.get(f.cluster_id) ?? [];
        existing.push(f);
        clusterMap.set(f.cluster_id, existing);
      } else {
        unclustered.push(f);
      }
    }

    for (const [clusterId, members] of clusterMap) {
      const cluster = state.clusters.find((c) => c.id === clusterId);
      if (!cluster || members.length <= 1) {
        // Single-member cluster or missing cluster data — render as regular finding
        unclustered.push(...members);
        continue;
      }
      // Find the representative
      const rep = cluster.representative_finding_id
        ? (members.find((m) => m.id === cluster.representative_finding_id) ?? members[0])
        : members[0];

      groups.push({ cluster, members, representative: rep });
    }

    return { clusterGroups: groups, unclustered };
  }, [displayFindings, state.clusters]);

  return (
    <div className="flex flex-col h-full">
      {/* Filter presets */}
      <div className="flex items-center gap-1.5 px-4 py-2 border-b border-zinc-800 shrink-0 flex-wrap">
        {presets.map((p) => (
          <button
            key={p.value}
            onClick={() => setPreset(p.value)}
            className={`text-xs px-2 py-1 rounded ${preset === p.value ? "bg-zinc-700 text-zinc-100" : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800"}`}
          >
            {p.label}
          </button>
        ))}
      </div>

      {displayFindings.length === 0 ? (
        <div className="flex items-center justify-center flex-1 text-zinc-500 text-sm">
          {state.findings.length === 0
            ? "No findings to display."
            : selectedFile
              ? `No ${preset} findings for ${selectedFile}`
              : `No ${preset} findings`}
        </div>
      ) : (
        <div className="space-y-3 overflow-y-auto p-4 flex-1">
          <div className="text-xs text-zinc-400 mb-2">
            {displayFindings.length} finding{displayFindings.length !== 1 ? "s" : ""}
            {clusterGroups.length > 0 && ` in ${clusterGroups.length + unclustered.length} groups`}
            {selectedFile && <span> in {selectedFile}</span>}
          </div>
          {clusterGroups.map((group) => {
            const isFocused =
              state.focusedFindingId != null &&
              group.members.some((m) => m.id === state.focusedFindingId);
            return (
              <ClusterCard
                key={group.cluster.id}
                cluster={{
                  ...group.cluster,
                  representative: group.representative,
                  members: group.members,
                }}
                onUpdate={refreshSnapshot}
                focused={isFocused}
              />
            );
          })}
          {unclustered.map((finding) => (
            <FindingCard
              key={finding.id}
              finding={finding}
              onUpdated={refreshSnapshot}
              focused={state.focusedFindingId === finding.id}
              sessionDecision={state.sessionDecisions[finding.id] ?? null}
              onDecision={handleDecision}
            />
          ))}
        </div>
      )}
    </div>
  );
}
