import { useMemo } from "react";
import { FindingCard } from "./FindingCard";
import ClusterCard from "./ClusterCard";
import { useReviewContext } from "../../lib/store";
import type { Finding, FindingCluster } from "../../lib/types";

interface ClusterGroup {
  cluster: FindingCluster;
  members: Finding[];
  representative: Finding;
}

export function SignalBoard() {
  const { state, refreshSnapshot } = useReviewContext();
  const activeFindings = state.findings.filter((f) => f.status === "active");
  const selectedFile = state.selectedFile;

  const displayFindings = selectedFile
    ? activeFindings.filter((f) => f.file_path === selectedFile)
    : activeFindings;

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

  if (displayFindings.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-zinc-500 text-sm">
        {activeFindings.length === 0
          ? "No findings to display."
          : `No findings for ${selectedFile}`}
      </div>
    );
  }

  return (
    <div className="space-y-3 overflow-y-auto p-4">
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
        />
      ))}
    </div>
  );
}
