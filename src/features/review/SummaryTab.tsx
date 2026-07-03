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
import type {
  ProviderControlPlaneSnapshot,
  ProviderSelectionTrace,
  RunScorecard,
} from "../../lib/types";
import { buildRunTrustOverview } from "../../lib/trust";

const severityIcons: Record<string, typeof AlertTriangle> = {
  blocker: ShieldAlert,
  critical: AlertTriangle,
  warning: Zap,
  info: Info,
  nitpick: Sparkles,
};

const severityColors: Record<string, string> = {
  blocker: "text-(--color-sev-blocker)",
  critical: "text-(--color-sev-critical)",
  warning: "text-(--color-sev-warning)",
  info: "text-(--color-sev-info)",
  nitpick: "text-(--color-sev-nitpick)",
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
  const isRerun = state.reviewFreshness.is_rerun;
  const rerunSupportingCopy = isRerun
    ? state.reviewFreshness.head_changed_since_review
      ? "This rerun reviews the latest changes since the last run."
      : "This rerun refreshes findings on the current head."
    : null;
  const trustOverview = useMemo(
    () =>
      buildRunTrustOverview({
        findings: state.findings,
        localChecksSummary: state.localChecksSummary,
        contextPackSummary: state.contextPackSummary,
        platformMetadata: state.platformMetadata,
        platformMetadataFetchedAt: state.platformMetadataFetchedAt,
        reviewFreshness: state.reviewFreshness,
      }),
    [
      state.findings,
      state.localChecksSummary,
      state.contextPackSummary,
      state.platformMetadata,
      state.platformMetadataFetchedAt,
      state.reviewFreshness,
    ],
  );

  const handleRerun = async () => {
    setRerunning(true);
    setRerunError(null);
    try {
      const newRunId = await rerunReview(state.runId, {
        triggerSource: "workspace",
        reason: "manual",
      });
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
          <span className="flex items-center gap-1.5 text-xs text-(--color-sev-warning) bg-(--color-sev-warning-bg) px-2 py-1 rounded">
            <Loader2 className="w-3 h-3 animate-spin" />
            {state.status === "running_agents" ? "Analyzing..." : "Cleaning..."}
          </span>
        )}
        {state.status === "ready" && (
          <span className="flex items-center gap-1.5 text-xs text-(--color-accent) bg-(--color-state-ready-bg) px-2 py-1 rounded">
            <CheckCircle className="w-3 h-3" /> Review ready
          </span>
        )}
        {state.status === "submitted" && (
          <span className="flex items-center gap-1.5 text-xs text-(--color-sev-info) bg-(--color-sev-info-bg) px-2 py-1 rounded">
            <CheckCircle className="w-3 h-3" /> Submitted
          </span>
        )}
        {state.status === "failed" && (
          <span className="flex items-center gap-1.5 text-xs text-(--color-sev-blocker) bg-(--color-sev-blocker-bg) px-2 py-1 rounded">
            <XCircle className="w-3 h-3" /> Failed
          </span>
        )}
        {(hasBlocker || hasCritical) && (
          <span className="flex items-center gap-1.5 text-xs text-(--color-sev-blocker) bg-(--color-sev-blocker-bg) px-2 py-1 rounded">
            <ShieldAlert className="w-3 h-3" /> High risk
          </span>
        )}
        {canRerun && (
          <button
            onClick={handleRerun}
            disabled={rerunning}
            className="flex items-center gap-1.5 text-xs text-(--color-text-secondary) bg-(--color-elevated) hover:bg-(--color-elevated) px-2.5 py-1 rounded transition-colors disabled:opacity-50 ml-auto"
          >
            <RefreshCw className={`w-3 h-3 ${rerunning ? "animate-spin" : ""}`} />
            {rerunning ? "Rerunning..." : "Rerun review"}
          </button>
        )}
      </div>
      {rerunSupportingCopy && (
        <p className="text-xs text-(--color-text-tertiary)">{rerunSupportingCopy}</p>
      )}
      {rerunError && (
        <p className="text-xs text-(--color-sev-blocker) bg-(--color-sev-blocker-bg) px-2 py-1 rounded">
          {rerunError}
        </p>
      )}

      {/* Stats row */}
      <div className="grid grid-cols-3 gap-3">
        <div className="bg-(--color-surface) border border-(--color-border-subtle) rounded-lg p-3">
          <div className="text-2xl font-bold text-(--color-text-primary)">
            {state.changedFiles.length}
          </div>
          <div className="text-xs text-(--color-text-tertiary)">files changed</div>
        </div>
        <div className="bg-(--color-surface) border border-(--color-border-subtle) rounded-lg p-3">
          <div className="text-2xl font-bold text-(--color-text-primary)">
            {activeFindings.length}
          </div>
          <div className="text-xs text-(--color-text-tertiary)">active findings</div>
        </div>
        <div className="bg-(--color-surface) border border-(--color-border-subtle) rounded-lg p-3">
          <div className="text-2xl font-bold text-(--color-text-primary)">
            {completedLanes}/{state.laneStatuses.length}
          </div>
          <div className="text-xs text-(--color-text-tertiary)">
            lanes {failedLanes > 0 ? `(${failedLanes} failed)` : "completed"}
          </div>
        </div>
      </div>

      {/* Lane statuses */}
      {state.laneStatuses.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
            Analysis lanes
          </h3>
          <div className="space-y-1.5">
            {state.laneStatuses.map((lane) => (
              <div
                key={lane.lane_id}
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-(--color-surface) border border-(--color-border-subtle)"
              >
                {lane.status === "completed" && (
                  <CheckCircle className="w-3.5 h-3.5 text-(--color-accent)" />
                )}
                {(lane.status === "failed" || lane.status === "timed_out") && (
                  <XCircle className="w-3.5 h-3.5 text-(--color-sev-blocker)" />
                )}
                {lane.status !== "completed" &&
                  lane.status !== "failed" &&
                  lane.status !== "timed_out" && (
                    <Loader2 className="w-3.5 h-3.5 text-(--color-sev-warning) animate-spin" />
                  )}
                <span className="text-sm text-(--color-text-primary) capitalize">
                  {lane.lane_id}
                </span>
                <span className="text-xs text-(--color-text-tertiary)">{lane.provider_name}</span>
                {lane.finding_count > 0 && (
                  <span className="text-xs text-(--color-text-secondary) ml-auto">
                    {lane.finding_count} finding{lane.finding_count !== 1 ? "s" : ""}
                  </span>
                )}
                {lane.error_message && (
                  <span className="text-xs text-(--color-sev-blocker) truncate ml-auto max-w-48">
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
          <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
            Severity breakdown
          </h3>
          <div className="flex gap-3 flex-wrap">
            {["blocker", "critical", "warning", "info", "nitpick"].map((sev) => {
              const count = severityBreakdown[sev];
              if (!count) return null;
              const Icon = severityIcons[sev] ?? Info;
              const color = severityColors[sev] ?? "text-(--color-text-secondary)";
              return (
                <div
                  key={sev}
                  className="flex items-center gap-1.5 bg-(--color-surface) border border-(--color-border-subtle) rounded px-2.5 py-1.5"
                >
                  <Icon className={`w-3.5 h-3.5 ${color}`} />
                  <span className={`text-sm font-medium ${color}`}>{count}</span>
                  <span className="text-xs text-(--color-text-tertiary) capitalize">{sev}</span>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* Hotspots */}
      {hotspots.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
            Hotspots
          </h3>
          <div className="space-y-1">
            {hotspots.map(([file, count]) => (
              <button
                key={file}
                onClick={() => setSelectedFile(file)}
                className="flex items-center gap-2 w-full text-left px-3 py-2 rounded-lg bg-(--color-surface) border border-(--color-border-subtle) hover:border-(--color-border) transition-colors"
              >
                <FileWarning className="w-3.5 h-3.5 text-(--color-sev-warning) shrink-0" />
                <code className="text-xs text-(--color-text-secondary) truncate flex-1">
                  {file}
                </code>
                <span className="text-xs text-(--color-text-tertiary) shrink-0">
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
          <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
            Changes since last run
          </h3>
          <div className="flex gap-3 flex-wrap">
            <div className="bg-(--color-state-ready-bg) border border-emerald-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-(--color-accent)">
                {state.delta.counts.new}
              </span>
              <span className="text-xs text-(--color-text-tertiary) ml-1">new</span>
            </div>
            <div className="bg-(--color-surface) border border-(--color-border-subtle) rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-(--color-text-secondary)">
                {state.delta.counts.unchanged}
              </span>
              <span className="text-xs text-(--color-text-tertiary) ml-1">unchanged</span>
            </div>
            <div className="bg-(--color-sev-warning-bg) border border-yellow-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-(--color-sev-warning)">
                {state.delta.counts.stale}
              </span>
              <span className="text-xs text-(--color-text-tertiary) ml-1">stale</span>
            </div>
            <div className="bg-(--color-sev-info-bg) border border-blue-800/50 rounded px-2.5 py-1.5">
              <span className="text-sm font-medium text-(--color-sev-info)">
                {state.delta.counts.resolved}
              </span>
              <span className="text-xs text-(--color-text-tertiary) ml-1">resolved</span>
            </div>
          </div>
          {state.delta.changed_files.length > 0 && (
            <p className="text-xs text-(--color-text-tertiary) mt-2">
              {state.delta.changed_files.length} file
              {state.delta.changed_files.length !== 1 ? "s" : ""} changed since the last review
            </p>
          )}
          {state.delta.resolved.length > 0 && (
            <div className="mt-3 space-y-2">
              <h4 className="text-xs font-medium text-(--color-text-secondary) uppercase tracking-wider">
                Resolved findings
              </h4>
              <div className="space-y-1.5">
                {state.delta.resolved.map((finding) => (
                  <div
                    key={finding.id}
                    className="rounded-lg border border-(--color-border-subtle) bg-(--color-surface) px-3 py-2"
                  >
                    <div className="flex items-center gap-2">
                      <span className="text-xs rounded bg-(--color-sev-info-bg) px-1.5 py-0.5 text-(--color-sev-info)">
                        resolved
                      </span>
                      <span className="text-sm text-(--color-text-primary)">{finding.title}</span>
                      <span className="text-xs text-(--color-text-tertiary) ml-auto capitalize">
                        {finding.severity}
                      </span>
                    </div>
                    {finding.file_path && (
                      <code className="mt-1 block text-xs text-(--color-text-tertiary)">
                        {finding.file_path}
                      </code>
                    )}
                  </div>
                ))}
              </div>
            </div>
          )}
        </section>
      )}

      <TrustOverviewSection trustOverview={trustOverview} />

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

      {(state.providerSelection || state.providerControl) && (
        <ProviderControlSection
          selection={state.providerSelection}
          control={state.providerControl}
        />
      )}

      {/* Provider scorecard */}
      {state.metrics && <ProviderScorecard scorecard={state.metrics} />}
    </div>
  );
}

function TrustOverviewSection({
  trustOverview,
}: {
  trustOverview: ReturnType<typeof buildRunTrustOverview>;
}) {
  return (
    <section>
      <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
        Review trust overview
      </h3>
      <div className="grid grid-cols-2 gap-3">
        <div className="bg-(--color-surface) border border-(--color-border-subtle) rounded-lg p-3 space-y-2">
          <div className="text-xs text-(--color-text-tertiary)">Provenance</div>
          <div className="flex gap-1.5 flex-wrap">
            {trustOverview.sourceCounts.length === 0 && (
              <span className="text-xs text-(--color-text-tertiary)">No surfaced findings yet</span>
            )}
            {trustOverview.sourceCounts.map((source) => (
              <span
                key={source.key}
                className="text-xs px-1.5 py-0.5 rounded bg-(--color-elevated) text-(--color-text-secondary)"
              >
                {source.label}: {source.count}
              </span>
            ))}
          </div>
        </div>
        <div className="bg-(--color-surface) border border-(--color-border-subtle) rounded-lg p-3 space-y-2">
          <div className="text-xs text-(--color-text-tertiary)">Deterministic inputs</div>
          <div className="flex gap-1.5 flex-wrap">
            <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-sev-info-bg) text-(--color-sev-info)">
              Evidence: {trustOverview.findingsWithEvidence}
            </span>
            <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-sev-info-bg) text-(--color-sev-info)">
              Issue context: {trustOverview.findingsWithIssueContext}
            </span>
            <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-sev-info-bg) text-(--color-sev-info)">
              Owners: {trustOverview.findingsWithOwnership}
            </span>
            <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-sev-info-bg) text-(--color-sev-info)">
              Supported findings: {trustOverview.findingsWithDeterministicSupport}
            </span>
          </div>
        </div>
      </div>
      <div className="mt-3 flex gap-1.5 flex-wrap">
        <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-elevated) text-(--color-text-secondary)">
          Local checks: {trustOverview.localChecksIncluded}
          {trustOverview.localCheckTools.length > 0 &&
            ` via ${trustOverview.localCheckTools.join(", ")}`}
        </span>
        <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-elevated) text-(--color-text-secondary)">
          Platform context: {trustOverview.hasPlatformMetadata ? "available" : "not available"}
          {trustOverview.platformFreshnessLabel ? `, ${trustOverview.platformFreshnessLabel}` : ""}
        </span>
        {trustOverview.reviewFreshnessLabel && (
          <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-elevated) text-(--color-text-secondary)">
            Review freshness: {trustOverview.reviewFreshnessLabel}
          </span>
        )}
      </div>
    </section>
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
  const platformLabel =
    metadata.platform === "gitlab"
      ? "GitLab"
      : metadata.platform === "bitbucket"
        ? "Bitbucket"
        : "GitHub";
  const entityLabel = metadata.platform === "gitlab" ? "MR" : "PR";
  return (
    <section>
      <div className="flex items-center gap-2 mb-2">
        <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider">
          {platformLabel} metadata
          {fetchedLabel && (
            <span className="ml-2 font-normal normal-case text-(--color-text-tertiary)">
              {fetchedLabel}
            </span>
          )}
        </h3>
        <button
          onClick={onRefresh}
          disabled={isRefreshing}
          className="ml-auto flex items-center gap-1 text-[11px] text-(--color-text-secondary) bg-(--color-elevated) hover:bg-(--color-elevated) px-2 py-1 rounded transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-3 h-3 ${isRefreshing ? "animate-spin" : ""}`} />
          {isRefreshing ? "Refreshing..." : `Refresh ${platformLabel} metadata`}
        </button>
      </div>
      {refreshError && (
        <p className="text-xs text-(--color-sev-blocker) bg-(--color-sev-blocker-bg) px-2 py-1 rounded mb-2">
          {refreshError}
        </p>
      )}
      <div className="space-y-2">
        {metadata.draft && (
          <span className="inline-flex items-center text-xs text-(--color-sev-warning) bg-(--color-sev-warning-bg) px-2 py-0.5 rounded">
            Draft {entityLabel}
          </span>
        )}
        {metadata.labels.length > 0 && (
          <div className="flex gap-1.5 flex-wrap">
            {metadata.labels.map((label) => (
              <span
                key={label}
                className="text-xs bg-(--color-elevated) text-(--color-text-secondary) px-2 py-0.5 rounded-full"
              >
                {label}
              </span>
            ))}
          </div>
        )}
        {metadata.platform === "github" && metadata.requested_reviewers.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Requested reviewers:</span>{" "}
            {metadata.requested_reviewers.join(", ")}
          </div>
        )}
        {metadata.platform === "github" && metadata.requested_teams.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Requested teams:</span>{" "}
            {metadata.requested_teams.join(", ")}
          </div>
        )}
        {metadata.platform === "gitlab" && metadata.reviewers.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Reviewers:</span>{" "}
            {metadata.reviewers.join(", ")}
          </div>
        )}
        {metadata.platform === "github" && metadata.review_state_summary.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Reviews:</span>{" "}
            {metadata.review_state_summary.map((r) => (
              <span key={r.login} className="mr-2">
                {r.login}{" "}
                <span
                  className={
                    r.state === "APPROVED"
                      ? "text-(--color-accent)"
                      : r.state === "CHANGES_REQUESTED"
                        ? "text-(--color-sev-blocker)"
                        : "text-(--color-text-tertiary)"
                  }
                >
                  ({r.state.toLowerCase().replace(/_/g, " ")})
                </span>
              </span>
            ))}
          </div>
        )}
        {metadata.platform === "gitlab" && metadata.approval_status && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Approval:</span>{" "}
            <span
              className={
                metadata.approval_status.approved
                  ? "text-(--color-accent)"
                  : "text-(--color-text-secondary)"
              }
            >
              {metadata.approval_status.approved ? "Approved" : "Pending"}
            </span>
            {metadata.approval_status.approved_by.length > 0 && (
              <span> by {metadata.approval_status.approved_by.join(", ")}</span>
            )}
            {metadata.approval_status.approvals_left !== null &&
              metadata.approval_status.approvals_left > 0 && (
                <span className="text-(--color-text-tertiary) ml-1">
                  ({metadata.approval_status.approvals_left} more needed)
                </span>
              )}
          </div>
        )}
        {metadata.platform === "github" && metadata.linked_issue_numbers.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Linked issues:</span>{" "}
            {metadata.linked_issue_numbers.map((n) => `#${n}`).join(", ")}
          </div>
        )}
        {metadata.platform === "gitlab" && metadata.closes_issues.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Closes issues:</span>{" "}
            {metadata.closes_issues.map((n) => `#${n}`).join(", ")}
          </div>
        )}
        {metadata.platform === "bitbucket" && metadata.reviewers.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Reviewers:</span>{" "}
            {metadata.reviewers.join(", ")}
          </div>
        )}
        {metadata.platform === "bitbucket" && metadata.default_reviewers.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Default reviewers:</span>{" "}
            {metadata.default_reviewers.join(", ")}
          </div>
        )}
        {metadata.platform === "bitbucket" && metadata.approval_status && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Approval:</span>{" "}
            <span
              className={
                metadata.approval_status.approved
                  ? "text-(--color-accent)"
                  : "text-(--color-text-secondary)"
              }
            >
              {metadata.approval_status.approved ? "Approved" : "Pending"}
            </span>
            {metadata.approval_status.approved_by.length > 0 && (
              <span> by {metadata.approval_status.approved_by.join(", ")}</span>
            )}
          </div>
        )}
        {metadata.platform === "bitbucket" && metadata.jira_issue_keys.length > 0 && (
          <div className="text-xs text-(--color-text-secondary)">
            <span className="text-(--color-text-tertiary)">Jira issues:</span>{" "}
            {metadata.jira_issue_keys.join(", ")}
          </div>
        )}
        {metadata.platform === "bitbucket" && (
          <div className="text-[10px] text-(--color-text-tertiary) mt-1 italic">
            Bitbucket does not support pending review groups or first-class suggestions.
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

function ProviderControlSection({
  selection,
  control,
}: {
  selection: ProviderSelectionTrace | null;
  control: ProviderControlPlaneSnapshot | null;
}) {
  const providerRows =
    control?.providers
      .slice()
      .sort((left, right) => {
        if (left.recommended_default && !right.recommended_default) return -1;
        if (!left.recommended_default && right.recommended_default) return 1;
        return left.display_name.localeCompare(right.display_name);
      })
      .slice(0, 4) ?? [];

  const hasFallback = selection?.selection_mode === "fallback" || selection?.warnings.length;
  const degradedProviders =
    control?.providers.filter((provider) => provider.status !== "ready") ?? [];

  return (
    <section>
      <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
        Provider control
      </h3>
      <div className="space-y-3">
        {selection && (
          <div
            className={`rounded-lg border px-3 py-3 ${
              hasFallback
                ? "border-(--color-sev-warning)/30 bg-(--color-sev-warning-bg)"
                : "border-(--color-border-subtle) bg-(--color-surface)"
            }`}
          >
            <div className="flex items-center gap-2">
              <span className="text-sm text-(--color-text-primary)">
                Why this provider was chosen
              </span>
              <span className="text-[10px] uppercase tracking-wide text-(--color-text-tertiary)">
                {selection.selection_mode}
              </span>
            </div>
            <p className="mt-1 text-xs text-(--color-text-secondary)">
              Requested{" "}
              <span className="text-(--color-text-primary)">{selection.requested_provider}</span>,
              selected{" "}
              <span className="text-(--color-text-primary)">{selection.selected_provider}</span>.
            </p>
            {selection.warnings.map((warning) => (
              <p key={warning} className="mt-2 text-xs text-(--color-sev-warning)">
                {warning}
              </p>
            ))}
          </div>
        )}

        {control?.recommendation_reason && (
          <div className="rounded-lg border border-(--color-border-subtle) bg-(--color-surface) px-3 py-3">
            <div className="text-xs text-(--color-text-tertiary)">Recommended default</div>
            <p className="mt-1 text-sm text-(--color-text-primary)">
              {control.recommendation_reason}
            </p>
          </div>
        )}

        {degradedProviders.length > 0 && (
          <div className="rounded-lg border border-red-900/40 bg-(--color-sev-blocker-bg) px-3 py-3">
            <div className="text-xs text-red-300">Degraded provider warnings</div>
            <div className="mt-2 space-y-1">
              {degradedProviders.slice(0, 3).map((provider) => (
                <p key={provider.provider_id} className="text-xs text-(--color-text-secondary)">
                  <span className="text-(--color-text-primary)">{provider.display_name}:</span>{" "}
                  {provider.status_reason}
                </p>
              ))}
            </div>
          </div>
        )}

        {providerRows.length > 0 && (
          <div className="grid grid-cols-2 gap-3">
            {providerRows.map((provider) => (
              <div
                key={provider.provider_id}
                className="rounded-lg border border-(--color-border-subtle) bg-(--color-surface) p-3"
              >
                <div className="flex items-center gap-2">
                  <div className="text-sm text-(--color-text-primary)">{provider.display_name}</div>
                  {provider.recommended_default && (
                    <span className="text-[10px] rounded bg-(--color-state-ready-bg) px-1.5 py-0.5 text-emerald-300">
                      recommended
                    </span>
                  )}
                </div>
                <div className="mt-1 text-[11px] text-(--color-text-tertiary) uppercase tracking-wide">
                  {provider.status}
                </div>
                <div className="mt-2 flex gap-1.5 flex-wrap">
                  <MetricChip
                    label="trust"
                    value={pctMaybe(provider.recent_metrics.avg_accept_rate)}
                  />
                  <MetricChip
                    label="latency"
                    value={secondsMaybe(provider.recent_metrics.avg_latency_ms)}
                  />
                  <MetricChip
                    label="cost"
                    value={costMaybe(provider.recent_metrics.avg_cost_usd)}
                  />
                </div>
                <p className="mt-2 text-xs text-(--color-text-secondary) line-clamp-3">
                  {provider.fit_narrative}
                </p>
              </div>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function ProviderScorecard({ scorecard }: { scorecard: RunScorecard }) {
  return (
    <section>
      <h3 className="text-xs font-medium text-(--color-text-tertiary) uppercase tracking-wider mb-2">
        Provider scorecard
      </h3>
      <div className="overflow-x-auto">
        <table className="w-full text-xs">
          <thead>
            <tr className="text-(--color-text-tertiary) border-b border-(--color-border-subtle)">
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
              <tr key={lane.lane_id} className="border-b border-(--color-border-subtle)">
                <td className="py-1.5 pr-3 text-(--color-text-secondary) capitalize">
                  {lane.lane_id}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-text-secondary)">
                  {lane.lane_latency_ms ? `${(lane.lane_latency_ms / 1000).toFixed(1)}s` : "—"}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-text-secondary)">
                  {lane.raw_findings_count}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-text-secondary)">
                  {lane.surfaced_findings_count}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-accent)">
                  {pct(lane.reviewer_accept_rate)}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-sev-info)">
                  {pct(lane.reviewer_edit_rate)}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-sev-warning)">
                  {pct(lane.suppress_rate)}
                </td>
                <td className="text-right py-1.5 px-2 text-(--color-text-secondary)">
                  {pct(lane.anchor_validity)}
                </td>
                {scorecard.lanes.some((l) => l.cost_usd !== null) && (
                  <td className="text-right py-1.5 pl-2 text-(--color-text-secondary)">
                    {lane.cost_usd !== null ? `$${lane.cost_usd.toFixed(4)}` : "—"}
                  </td>
                )}
              </tr>
            ))}
            {/* Overall row */}
            <tr className="border-t border-(--color-border) font-medium">
              <td className="py-1.5 pr-3 text-(--color-text-primary)">Overall</td>
              <td className="text-right py-1.5 px-2 text-(--color-text-tertiary)">—</td>
              <td className="text-right py-1.5 px-2 text-(--color-text-tertiary)">—</td>
              <td className="text-right py-1.5 px-2 text-(--color-text-primary)">
                {scorecard.overall_surfaced}
              </td>
              <td className="text-right py-1.5 px-2 text-(--color-accent)">
                {pct(scorecard.overall_accept_rate)}
              </td>
              <td className="text-right py-1.5 px-2 text-(--color-sev-info)">
                {pct(scorecard.overall_edit_rate)}
              </td>
              <td className="text-right py-1.5 px-2 text-(--color-sev-warning)">
                {pct(scorecard.overall_suppress_rate)}
              </td>
              <td className="text-right py-1.5 px-2 text-(--color-text-tertiary)">—</td>
              {scorecard.lanes.some((l) => l.cost_usd !== null) && (
                <td className="text-right py-1.5 pl-2 text-(--color-text-tertiary)">—</td>
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

function pctMaybe(value: number | null): string {
  return value == null ? "—" : pct(value);
}

function secondsMaybe(value: number | null): string {
  return value == null ? "—" : `${(value / 1000).toFixed(1)}s`;
}

function costMaybe(value: number | null): string {
  return value == null ? "—" : `$${value.toFixed(2)}`;
}

function MetricChip({ label, value }: { label: string; value: string }) {
  return (
    <span className="text-[10px] rounded bg-(--color-elevated) px-1.5 py-0.5 text-(--color-text-secondary)">
      {label}: {value}
    </span>
  );
}
