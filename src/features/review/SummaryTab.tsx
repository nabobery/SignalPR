import { useMemo, useState } from "react";
import { useNavigate } from "react-router";
import {
  AlertTriangle,
  ShieldAlert,
  Zap,
  Info,
  Sparkles,
  FileWarning,
  CheckCircle,
  XCircle,
  Loader2,
  RefreshCw,
} from "lucide-react";
import { useReviewContext } from "../../lib/store";
import { rerunReview, refreshPrMetadata, parseError } from "../../lib/ipc";
import type { RunScorecard } from "../../lib/types";

const severityIcons: Record<string, typeof AlertTriangle> = {
  blocker: ShieldAlert,
  critical: AlertTriangle,
  warning: Zap,
  info: Info,
  nitpick: Sparkles,
};

const severityColors: Record<string, string> = {
  blocker: "text-red-400",
  critical: "text-orange-400",
  warning: "text-yellow-400",
  info: "text-blue-400",
  nitpick: "text-zinc-400",
};

export function SummaryTab() {
  const { state, setSelectedFile, refreshSnapshot } = useReviewContext();
  const navigate = useNavigate();
  const [rerunning, setRerunning] = useState(false);
  const [rerunError, setRerunError] = useState<string | null>(null);
  const [refreshingMetadata, setRefreshingMetadata] = useState(false);
  const [metadataRefreshError, setMetadataRefreshError] = useState<string | null>(null);
  const activeFindings = state.findings.filter((f) => f.status === "active");

  const severityBreakdown = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const f of activeFindings) {
      const sev = f.user_severity_override ?? f.severity;
      counts[sev] = (counts[sev] ?? 0) + 1;
    }
    return counts;
  }, [activeFindings]);

  const hotspots = useMemo(() => {
    const fileCounts: Record<string, number> = {};
    for (const f of activeFindings) {
      if (f.file_path) {
        fileCounts[f.file_path] = (fileCounts[f.file_path] ?? 0) + 1;
      }
    }
    return Object.entries(fileCounts)
      .sort(([, a], [, b]) => b - a)
      .slice(0, 8);
  }, [activeFindings]);

  const hasBlocker = "blocker" in severityBreakdown;
  const hasCritical = "critical" in severityBreakdown;
  const isRunning =
    state.status === "created" || state.status === "running_agents" || state.status === "cleaning";
  const canRerun = state.status === "ready" || state.status === "submitted";
  const completedLanes = state.laneStatuses.filter((l) => l.status === "completed").length;
  const failedLanes = state.laneStatuses.filter(
    (l) => l.status === "failed" || l.status === "timed_out",
  ).length;

  const handleRerun = async () => {
    setRerunning(true);
    setRerunError(null);
    try {
      const newRunId = await rerunReview(state.runId);
      navigate(`/review/${newRunId}`);
    } catch (err) {
      setRerunError(parseError(err).message);
    } finally {
      setRerunning(false);
    }
  };

  const handleRefreshMetadata = async () => {
    setRefreshingMetadata(true);
    setMetadataRefreshError(null);
    try {
      await refreshPrMetadata(state.prId);
      await refreshSnapshot();
    } catch (err) {
      setMetadataRefreshError(parseError(err).message);
    } finally {
      setRefreshingMetadata(false);
    }
  };

  return (
    <div className="overflow-y-auto p-4 space-y-5">
      {/* Status + risk */}
      <div className="flex items-center gap-3 flex-wrap">
        {isRunning && (
          <span className="flex items-center gap-1.5 text-xs text-yellow-400 bg-yellow-900/20 px-2 py-1 rounded">
            <Loader2 className="w-3 h-3 animate-spin" />
            {state.status === "running_agents" ? "Analyzing..." : "Cleaning..."}
          </span>
        )}
        {state.status === "ready" && (
          <span className="flex items-center gap-1.5 text-xs text-emerald-400 bg-emerald-900/20 px-2 py-1 rounded">
            <CheckCircle className="w-3 h-3" /> Review ready
          </span>
        )}
        {state.status === "submitted" && (
          <span className="flex items-center gap-1.5 text-xs text-blue-400 bg-blue-900/20 px-2 py-1 rounded">
            <CheckCircle className="w-3 h-3" /> Submitted
          </span>
        )}
        {state.status === "failed" && (
          <span className="flex items-center gap-1.5 text-xs text-red-400 bg-red-900/20 px-2 py-1 rounded">
            <XCircle className="w-3 h-3" /> Failed
          </span>
        )}
        {(hasBlocker || hasCritical) && (
          <span className="flex items-center gap-1.5 text-xs text-red-400 bg-red-900/20 px-2 py-1 rounded">
            <ShieldAlert className="w-3 h-3" /> High risk
          </span>
        )}
        {canRerun && (
          <button
            onClick={handleRerun}
            disabled={rerunning}
            className="flex items-center gap-1.5 text-xs text-zinc-300 bg-zinc-800 hover:bg-zinc-700 px-2.5 py-1 rounded transition-colors disabled:opacity-50 ml-auto"
          >
            <RefreshCw className={`w-3 h-3 ${rerunning ? "animate-spin" : ""}`} />
            {rerunning ? "Rerunning..." : "Rerun"}
          </button>
        )}
      </div>
      {rerunError && (
        <p className="text-xs text-red-400 bg-red-900/20 px-2 py-1 rounded">{rerunError}</p>
      )}

      {/* Stats row */}
      <div className="grid grid-cols-3 gap-3">
        <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
          <div className="text-2xl font-bold text-zinc-100">{state.changedFiles.length}</div>
          <div className="text-xs text-zinc-500">files changed</div>
        </div>
        <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
          <div className="text-2xl font-bold text-zinc-100">{activeFindings.length}</div>
          <div className="text-xs text-zinc-500">active findings</div>
        </div>
        <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
          <div className="text-2xl font-bold text-zinc-100">
            {completedLanes}/{state.laneStatuses.length}
          </div>
          <div className="text-xs text-zinc-500">
            lanes {failedLanes > 0 ? `(${failedLanes} failed)` : "completed"}
          </div>
        </div>
      </div>

      {/* Lane statuses */}
      {state.laneStatuses.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Analysis lanes
          </h3>
          <div className="space-y-1.5">
            {state.laneStatuses.map((lane) => (
              <div
                key={lane.lane_id}
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-zinc-900/50 border border-zinc-800/50"
              >
                {lane.status === "completed" && (
                  <CheckCircle className="w-3.5 h-3.5 text-emerald-400" />
                )}
                {(lane.status === "failed" || lane.status === "timed_out") && (
                  <XCircle className="w-3.5 h-3.5 text-red-400" />
                )}
                {lane.status !== "completed" &&
                  lane.status !== "failed" &&
                  lane.status !== "timed_out" && (
                    <Loader2 className="w-3.5 h-3.5 text-yellow-400 animate-spin" />
                  )}
                <span className="text-sm text-zinc-200 capitalize">{lane.lane_id}</span>
                <span className="text-xs text-zinc-500">{lane.provider_name}</span>
                {lane.finding_count > 0 && (
                  <span className="text-xs text-zinc-400 ml-auto">
                    {lane.finding_count} finding{lane.finding_count !== 1 ? "s" : ""}
                  </span>
                )}
                {lane.error_message && (
                  <span className="text-xs text-red-400 truncate ml-auto max-w-48">
                    {lane.error_message}
                  </span>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Severity breakdown */}
      {activeFindings.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Severity breakdown
          </h3>
          <div className="flex gap-3 flex-wrap">
            {["blocker", "critical", "warning", "info", "nitpick"].map((sev) => {
              const count = severityBreakdown[sev];
              if (!count) return null;
              const Icon = severityIcons[sev] ?? Info;
              const color = severityColors[sev] ?? "text-zinc-400";
              return (
                <div
                  key={sev}
                  className="flex items-center gap-1.5 bg-zinc-900/50 border border-zinc-800/50 rounded px-2.5 py-1.5"
                >
                  <Icon className={`w-3.5 h-3.5 ${color}`} />
                  <span className={`text-sm font-medium ${color}`}>{count}</span>
                  <span className="text-xs text-zinc-500 capitalize">{sev}</span>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* Hotspots */}
      {hotspots.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Hotspots
          </h3>
          <div className="space-y-1">
            {hotspots.map(([file, count]) => (
              <button
                key={file}
                onClick={() => setSelectedFile(file)}
                className="flex items-center gap-2 w-full text-left px-3 py-2 rounded-lg bg-zinc-900/50 border border-zinc-800/50 hover:border-zinc-700 transition-colors"
              >
                <FileWarning className="w-3.5 h-3.5 text-yellow-400 shrink-0" />
                <code className="text-xs text-zinc-300 truncate flex-1">{file}</code>
                <span className="text-xs text-zinc-500 shrink-0">
                  {count} finding{count !== 1 ? "s" : ""}
                </span>
              </button>
            ))}
          </div>
        </section>
      )}

      {/* Delta summary (for reruns) */}
      {state.delta && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Changes since last run
          </h3>
          <div className="flex gap-3 flex-wrap">
            <div className="bg-emerald-900/20 border border-emerald-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-emerald-400">{state.delta.counts.new}</span>
              <span className="text-xs text-zinc-500 ml-1">new</span>
            </div>
            <div className="bg-zinc-900/50 border border-zinc-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-zinc-300">
                {state.delta.counts.unchanged}
              </span>
              <span className="text-xs text-zinc-500 ml-1">unchanged</span>
            </div>
            <div className="bg-yellow-900/20 border border-yellow-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-yellow-400">
                {state.delta.counts.stale}
              </span>
              <span className="text-xs text-zinc-500 ml-1">stale</span>
            </div>
            <div className="bg-blue-900/20 border border-blue-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-blue-400">
                {state.delta.counts.resolved}
              </span>
              <span className="text-xs text-zinc-500 ml-1">resolved</span>
            </div>
          </div>
          {state.delta.changed_files.length > 0 && (
            <p className="text-xs text-zinc-500 mt-2">
              {state.delta.changed_files.length} file
              {state.delta.changed_files.length !== 1 ? "s" : ""} changed since baseline
            </p>
          )}
        </section>
      )}

      {/* GitHub metadata */}
      {state.platformMetadata && (
        <PlatformMetadataSection
          metadata={state.platformMetadata}
          fetchedAt={state.platformMetadataFetchedAt}
          isRefreshing={refreshingMetadata}
          onRefresh={handleRefreshMetadata}
          refreshError={metadataRefreshError}
        />
      )}

      {/* Provider scorecard */}
      {state.metrics && <ProviderScorecard scorecard={state.metrics} />}
    </div>
  );
}

function PlatformMetadataSection({
  metadata,
  fetchedAt,
  isRefreshing,
  onRefresh,
  refreshError,
}: {
  metadata: import("../../lib/types").PlatformMetadata;
  fetchedAt: string | null;
  isRefreshing: boolean;
  onRefresh: () => void;
  refreshError: string | null;
}) {
  const fetchedLabel = formatMaybeDate(fetchedAt);
  const isGitLab = metadata.platform === "gitlab";
  const platformLabel = isGitLab ? "GitLab" : "GitHub";
  const entityLabel = isGitLab ? "MR" : "PR";
  return (
    <section>
      <div className="flex items-center gap-2 mb-2">
        <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider">
          {platformLabel} metadata
          {fetchedLabel && (
            <span className="ml-2 font-normal normal-case text-zinc-600">{fetchedLabel}</span>
          )}
        </h3>
        <button
          onClick={onRefresh}
          disabled={isRefreshing}
          className="ml-auto flex items-center gap-1 text-[11px] text-zinc-300 bg-zinc-800 hover:bg-zinc-700 px-2 py-1 rounded transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-3 h-3 ${isRefreshing ? "animate-spin" : ""}`} />
          {isRefreshing ? "Refreshing..." : `Refresh ${platformLabel} metadata`}
        </button>
      </div>
      {refreshError && (
        <p className="text-xs text-red-400 bg-red-900/20 px-2 py-1 rounded mb-2">{refreshError}</p>
      )}
      <div className="space-y-2">
        {metadata.draft && (
          <span className="inline-flex items-center text-xs text-yellow-400 bg-yellow-900/20 px-2 py-0.5 rounded">
            Draft {entityLabel}
          </span>
        )}
        {metadata.labels.length > 0 && (
          <div className="flex gap-1.5 flex-wrap">
            {metadata.labels.map((label) => (
              <span
                key={label}
                className="text-xs bg-zinc-800 text-zinc-300 px-2 py-0.5 rounded-full"
              >
                {label}
              </span>
            ))}
          </div>
        )}
        {metadata.platform === "github" && metadata.requested_reviewers.length > 0 && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Requested reviewers:</span>{" "}
            {metadata.requested_reviewers.join(", ")}
          </div>
        )}
        {metadata.platform === "github" && metadata.requested_teams.length > 0 && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Requested teams:</span>{" "}
            {metadata.requested_teams.join(", ")}
          </div>
        )}
        {metadata.platform === "gitlab" && metadata.reviewers.length > 0 && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Reviewers:</span> {metadata.reviewers.join(", ")}
          </div>
        )}
        {metadata.platform === "github" && metadata.review_state_summary.length > 0 && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Reviews:</span>{" "}
            {metadata.review_state_summary.map((r) => (
              <span key={r.login} className="mr-2">
                {r.login}{" "}
                <span
                  className={
                    r.state === "APPROVED"
                      ? "text-emerald-400"
                      : r.state === "CHANGES_REQUESTED"
                        ? "text-red-400"
                        : "text-zinc-500"
                  }
                >
                  ({r.state.toLowerCase().replace(/_/g, " ")})
                </span>
              </span>
            ))}
          </div>
        )}
        {metadata.platform === "gitlab" && metadata.approval_status && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Approval:</span>{" "}
            <span
              className={metadata.approval_status.approved ? "text-emerald-400" : "text-zinc-400"}
            >
              {metadata.approval_status.approved ? "Approved" : "Pending"}
            </span>
            {metadata.approval_status.approved_by.length > 0 && (
              <span> by {metadata.approval_status.approved_by.join(", ")}</span>
            )}
            {metadata.approval_status.approvals_left !== null &&
              metadata.approval_status.approvals_left > 0 && (
                <span className="text-zinc-500 ml-1">
                  ({metadata.approval_status.approvals_left} more needed)
                </span>
              )}
          </div>
        )}
        {metadata.platform === "github" && metadata.linked_issue_numbers.length > 0 && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Linked issues:</span>{" "}
            {metadata.linked_issue_numbers.map((n) => `#${n}`).join(", ")}
          </div>
        )}
        {metadata.platform === "gitlab" && metadata.closes_issues.length > 0 && (
          <div className="text-xs text-zinc-400">
            <span className="text-zinc-500">Closes issues:</span>{" "}
            {metadata.closes_issues.map((n) => `#${n}`).join(", ")}
          </div>
        )}
      </div>
    </section>
  );
}

function formatMaybeDate(value: string | null): string | null {
  if (!value) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleString();
}

function ProviderScorecard({ scorecard }: { scorecard: RunScorecard }) {
  return (
    <section>
      <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
        Provider scorecard
      </h3>
      <div className="overflow-x-auto">
        <table className="w-full text-xs">
          <thead>
            <tr className="text-zinc-500 border-b border-zinc-800">
              <th className="text-left py-1.5 pr-3 font-medium">Lane</th>
              <th className="text-right py-1.5 px-2 font-medium">Latency</th>
              <th className="text-right py-1.5 px-2 font-medium">Raw</th>
              <th className="text-right py-1.5 px-2 font-medium">Surfaced</th>
              <th className="text-right py-1.5 px-2 font-medium">Accept%</th>
              <th className="text-right py-1.5 px-2 font-medium">Edit%</th>
              <th className="text-right py-1.5 px-2 font-medium">Suppress%</th>
              <th className="text-right py-1.5 px-2 font-medium">Anchored%</th>
              {scorecard.lanes.some((l) => l.cost_usd !== null) && (
                <th className="text-right py-1.5 pl-2 font-medium">Cost</th>
              )}
            </tr>
          </thead>
          <tbody>
            {scorecard.lanes.map((lane) => (
              <tr key={lane.lane_id} className="border-b border-zinc-800/50">
                <td className="py-1.5 pr-3 text-zinc-300 capitalize">{lane.lane_id}</td>
                <td className="text-right py-1.5 px-2 text-zinc-400">
                  {lane.lane_latency_ms ? `${(lane.lane_latency_ms / 1000).toFixed(1)}s` : "—"}
                </td>
                <td className="text-right py-1.5 px-2 text-zinc-400">{lane.raw_findings_count}</td>
                <td className="text-right py-1.5 px-2 text-zinc-300">
                  {lane.surfaced_findings_count}
                </td>
                <td className="text-right py-1.5 px-2 text-emerald-400">
                  {pct(lane.reviewer_accept_rate)}
                </td>
                <td className="text-right py-1.5 px-2 text-blue-400">
                  {pct(lane.reviewer_edit_rate)}
                </td>
                <td className="text-right py-1.5 px-2 text-yellow-400">
                  {pct(lane.suppress_rate)}
                </td>
                <td className="text-right py-1.5 px-2 text-zinc-400">
                  {pct(lane.anchor_validity)}
                </td>
                {scorecard.lanes.some((l) => l.cost_usd !== null) && (
                  <td className="text-right py-1.5 pl-2 text-zinc-400">
                    {lane.cost_usd !== null ? `$${lane.cost_usd.toFixed(4)}` : "—"}
                  </td>
                )}
              </tr>
            ))}
            {/* Overall row */}
            <tr className="border-t border-zinc-700 font-medium">
              <td className="py-1.5 pr-3 text-zinc-200">Overall</td>
              <td className="text-right py-1.5 px-2 text-zinc-500">—</td>
              <td className="text-right py-1.5 px-2 text-zinc-500">—</td>
              <td className="text-right py-1.5 px-2 text-zinc-200">{scorecard.overall_surfaced}</td>
              <td className="text-right py-1.5 px-2 text-emerald-400">
                {pct(scorecard.overall_accept_rate)}
              </td>
              <td className="text-right py-1.5 px-2 text-blue-400">
                {pct(scorecard.overall_edit_rate)}
              </td>
              <td className="text-right py-1.5 px-2 text-yellow-400">
                {pct(scorecard.overall_suppress_rate)}
              </td>
              <td className="text-right py-1.5 px-2 text-zinc-500">—</td>
              {scorecard.lanes.some((l) => l.cost_usd !== null) && (
                <td className="text-right py-1.5 pl-2 text-zinc-500">—</td>
              )}
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  );
}

function pct(value: number): string {
  if (value === 0) return "0%";
  return `${Math.round(value * 100)}%`;
}
