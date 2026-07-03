import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import {
  AlertTriangle,
  CheckCircle,
  Loader2,
  RefreshCcw,
  RotateCcw,
  Search,
  XCircle,
} from "lucide-react";
import {
  getInboxOverview,
  parseError,
  refreshPrMetadata,
  rerunReview,
  resumeReview,
  startReview,
} from "../../lib/ipc";
import { IntakeQuickAction } from "../intake/IntakeQuickAction";
import { queueBadge, queueBadgeLabel, statusLabel, statusTextClass } from "../../ui/badge";
import type {
  EnvironmentSummary,
  InboxOverview,
  InboxReviewRow,
  InboxSection,
} from "../../lib/types";

type Filters = {
  query: string;
  repo: string;
  platform: string;
  provider: string;
  attentionOnly: boolean;
};

const defaultFilters: Filters = {
  query: "",
  repo: "all",
  platform: "all",
  provider: "all",
  attentionOnly: false,
};

function QueueBadge({ state }: { state: string }) {
  const style = queueBadge(state);
  return (
    <span
      className={`inline-flex items-center rounded border px-2 py-0.5 text-[11px] font-medium ${style.text} ${style.bg} ${style.border}`}
    >
      {queueBadgeLabel(state)}
    </span>
  );
}

function StatusPill({ status }: { status: string }) {
  const cls = statusTextClass(status);
  const label = statusLabel(status);
  return <span className={`text-[11px] font-medium ${cls}`}>{label}</span>;
}

/* ── Relative timestamp ─────────────────────────────────────── */

function relativeTime(value: string | null | undefined): string {
  if (!value) return "";
  const d = new Date(value);
  if (Number.isNaN(d.getTime())) return "";
  const diff = (Date.now() - d.getTime()) / 1000;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
  return `${Math.floor(diff / 86400)}d ago`;
}

function isNotStartedInboxRow(row: InboxReviewRow): boolean {
  return row.run_id === "";
}

/* ── Filter logic ───────────────────────────────────────────── */

function matchesFilters(row: InboxReviewRow, filters: Filters) {
  if (filters.repo !== "all" && `${row.repo_owner}/${row.repo_name}` !== filters.repo) return false;
  if (filters.platform !== "all" && row.platform !== filters.platform) return false;
  if (filters.provider !== "all" && !row.providers_used.includes(filters.provider)) return false;
  if (filters.attentionOnly && row.attention_reasons.length === 0) return false;
  const q = filters.query.trim().toLowerCase();
  if (!q) return true;
  const haystack = [
    row.pr_number.toString(),
    row.title,
    row.author ?? "",
    row.repo_owner,
    row.repo_name,
    row.workspace_path,
  ]
    .join(" ")
    .toLowerCase();
  return haystack.includes(q);
}

/* ── StatusStrip ────────────────────────────────────────────── */

function StatusStrip({ env, attentionCount }: { env: EnvironmentSummary; attentionCount: number }) {
  const allGood = env.warnings.length === 0 && attentionCount === 0;
  return (
    <div
      className={`flex items-center gap-2 px-4 py-2 border-b text-xs ${
        allGood
          ? "border-(--color-border-subtle) text-(--color-text-secondary)"
          : "border-(--color-state-alert)/20 bg-(--color-state-alert-bg) text-(--color-state-alert)"
      }`}
    >
      {allGood ? (
        <CheckCircle className="h-3.5 w-3.5 text-(--color-state-ready) shrink-0" />
      ) : (
        <AlertTriangle className="h-3.5 w-3.5 shrink-0" />
      )}
      <span>
        {allGood
          ? "Environment ready"
          : (env.warnings[0] ??
            `${attentionCount} item${attentionCount === 1 ? "" : "s"} need attention`)}
      </span>
      {!allGood && env.warnings.length > 1 && (
        <span className="text-(--color-text-tertiary)">+{env.warnings.length - 1} more</span>
      )}
      <span className="ml-auto text-(--color-text-tertiary)">
        {env.available_providers.length > 0 && env.available_providers.join(", ")}
      </span>
    </div>
  );
}

/* ── FilterBar ──────────────────────────────────────────────── */

function FilterBar({
  filters,
  repoOptions,
  platformOptions,
  providerOptions,
  onChange,
  onReset,
}: {
  filters: Filters;
  repoOptions: string[];
  platformOptions: string[];
  providerOptions: string[];
  onChange: (next: Filters) => void;
  onReset: () => void;
}) {
  const selectCls =
    "rounded-md border border-(--color-border) bg-(--color-elevated) px-3 py-1.5 text-xs text-(--color-text-primary) focus:border-(--color-border-strong) focus:outline-none cursor-pointer";

  return (
    <div className="flex items-center gap-2 flex-wrap">
      <label className="relative flex items-center flex-1 min-w-[160px]">
        <Search className="absolute left-2.5 h-3.5 w-3.5 text-(--color-text-tertiary) pointer-events-none" />
        <input
          value={filters.query}
          onChange={(e) => onChange({ ...filters, query: e.target.value })}
          placeholder="Search PR, author, repo..."
          className="w-full rounded-md border border-(--color-border) bg-(--color-elevated) py-1.5 pl-8 pr-3 text-xs text-(--color-text-primary) placeholder:text-(--color-text-tertiary) focus:border-(--color-border-strong) focus:outline-none"
        />
      </label>

      <select
        value={filters.repo}
        onChange={(e) => onChange({ ...filters, repo: e.target.value })}
        className={selectCls}
      >
        <option value="all">All repos</option>
        {repoOptions.map((r) => (
          <option key={r} value={r}>
            {r}
          </option>
        ))}
      </select>

      <select
        value={filters.platform}
        onChange={(e) => onChange({ ...filters, platform: e.target.value })}
        className={selectCls}
      >
        <option value="all">All platforms</option>
        {platformOptions.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
      </select>

      <select
        value={filters.provider}
        onChange={(e) => onChange({ ...filters, provider: e.target.value })}
        className={selectCls}
      >
        <option value="all">All providers</option>
        {providerOptions.map((p) => (
          <option key={p} value={p}>
            {p}
          </option>
        ))}
      </select>

      <label className="flex items-center gap-1.5 text-xs text-(--color-text-secondary) cursor-pointer select-none">
        <input
          type="checkbox"
          checked={filters.attentionOnly}
          onChange={(e) => onChange({ ...filters, attentionOnly: e.target.checked })}
          className="h-3.5 w-3.5 rounded accent-(--color-accent)"
        />
        Attention only
      </label>

      {(filters.query ||
        filters.repo !== "all" ||
        filters.platform !== "all" ||
        filters.provider !== "all" ||
        filters.attentionOnly) && (
        <button
          onClick={onReset}
          className="text-xs text-(--color-text-tertiary) hover:text-(--color-text-secondary) transition-colors"
        >
          Clear
        </button>
      )}
    </div>
  );
}

/* ── ReviewRow ──────────────────────────────────────────────── */

function ReviewRow({
  row,
  refreshBusy,
  onOpen,
  onStart,
  onRerun,
  onResume,
  onRefresh,
}: {
  row: InboxReviewRow;
  refreshBusy: boolean;
  onOpen: () => void;
  onStart: () => void;
  onRerun: () => void;
  onResume: () => void;
  onRefresh: () => void;
}) {
  const actionBtnCls =
    "inline-flex items-center gap-1 rounded-md border border-(--color-border) bg-(--color-elevated) px-2 py-1 text-xs text-(--color-text-secondary) transition-colors hover:border-(--color-border-strong) hover:text-(--color-text-primary) disabled:opacity-40";

  // A row with no run yet can only be started, not opened.
  const notStarted = isNotStartedInboxRow(row);
  const primaryClick = notStarted ? onStart : onOpen;

  return (
    <div
      className="group flex items-start gap-3 rounded-lg border border-(--color-border-subtle) bg-(--color-surface) px-4 py-3 transition-colors hover:border-(--color-border) hover:bg-(--color-elevated) cursor-pointer"
      onClick={primaryClick}
    >
      {/* Left: PR info */}
      <div className="flex-1 min-w-0">
        {/* Row 1: number + title + status */}
        <div className="flex items-center gap-2 min-w-0">
          <span className="text-[11px] font-mono text-(--color-text-tertiary) shrink-0">
            #{row.pr_number}
          </span>
          <h3 className="truncate text-sm font-medium text-(--color-text-primary) flex-1">
            {row.title}
          </h3>
          <StatusPill status={row.status} />
        </div>

        {/* Row 2: metadata */}
        <div className="mt-1 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-(--color-text-tertiary)">
          <span>
            {row.repo_owner}/{row.repo_name}
          </span>
          {row.author && <span>{row.author}</span>}
          {row.active_finding_count > 0 && (
            <span>
              {row.active_finding_count} finding{row.active_finding_count === 1 ? "" : "s"}
            </span>
          )}
          {row.providers_used.length > 0 && <span>{row.providers_used.join(", ")}</span>}
          <span>{relativeTime(row.last_updated)}</span>
          {row.draft && <span className="text-(--color-text-tertiary)">PR draft</span>}
          {row.has_saved_review_draft && (
            <span className="text-(--color-accent)">Review draft</span>
          )}
        </div>

        {/* Row 3: attention reasons */}
        {row.attention_reasons.length > 0 && (
          <div className="mt-1.5 flex flex-wrap gap-1">
            {row.attention_reasons.map((reason) => (
              <span
                key={reason}
                className="inline-flex items-center gap-1 text-[11px] text-(--color-state-alert)"
              >
                <AlertTriangle className="h-3 w-3 shrink-0" />
                {reason}
              </span>
            ))}
          </div>
        )}
      </div>

      {/* Right: badge + actions */}
      <div className="flex shrink-0 items-center gap-2 ml-2" onClick={(e) => e.stopPropagation()}>
        <QueueBadge state={row.queue_state} />

        {row.allowed_actions.includes("refresh_metadata") && (
          <button
            onClick={onRefresh}
            disabled={refreshBusy}
            className={actionBtnCls}
            title="Refresh metadata"
          >
            {refreshBusy ? (
              <Loader2 className="h-3 w-3 animate-spin" />
            ) : (
              <RefreshCcw className="h-3 w-3" />
            )}
          </button>
        )}
        {row.allowed_actions.includes("resume") && (
          <button onClick={onResume} className={actionBtnCls} title="Resume review">
            <RotateCcw className="h-3 w-3" />
            Resume
          </button>
        )}
        {row.allowed_actions.includes("rerun") && (
          <button onClick={onRerun} className={actionBtnCls} title="Rerun review">
            <RotateCcw className="h-3 w-3" />
            Rerun
          </button>
        )}

        <button
          onClick={primaryClick}
          className="inline-flex items-center gap-1 rounded-md bg-(--color-accent) px-2.5 py-1 text-xs font-medium text-white transition-colors hover:bg-(--color-accent-hover)"
        >
          {notStarted ? "Start review" : "Open"}
        </button>
      </div>
    </div>
  );
}

/* ── InboxView ──────────────────────────────────────────────── */

export function InboxView() {
  const navigate = useNavigate();
  const [overview, setOverview] = useState<InboxOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filters, setFilters] = useState<Filters>(defaultFilters);
  const [refreshingPrIds, setRefreshingPrIds] = useState<Record<string, boolean>>({});

  const fetchOverview = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await getInboxOverview();
      setOverview(data);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    fetchOverview();
  }, []);

  const allRows = useMemo(() => overview?.sections.flatMap((s) => s.items) ?? [], [overview]);

  const repoOptions = useMemo(
    () => Array.from(new Set(allRows.map((r) => `${r.repo_owner}/${r.repo_name}`))).sort(),
    [allRows],
  );
  const platformOptions = useMemo(
    () => Array.from(new Set(allRows.map((r) => r.platform).filter((p) => p !== "unknown"))).sort(),
    [allRows],
  );
  const providerOptions = useMemo(
    () => Array.from(new Set(allRows.flatMap((r) => r.providers_used))).sort(),
    [allRows],
  );

  const visibleSections = useMemo(() => {
    if (!overview) return [];
    return overview.sections
      .map((s) => ({ ...s, items: s.items.filter((r) => matchesFilters(r, filters)) }))
      .filter((s) => s.items.length > 0);
  }, [filters, overview]);

  const handleResume = async (runId: string) => {
    try {
      const id = await resumeReview(runId);
      navigate(`/review/${id}`);
    } catch (err) {
      setError(parseError(err).message);
    }
  };

  const handleStart = async (row: InboxReviewRow) => {
    if (!row.workspace_path) {
      setError(
        `Set a local repository path for ${row.repo_owner}/${row.repo_name} before starting a review. Fetch the PR again above to confirm the workspace.`,
      );
      return;
    }
    try {
      const id = await startReview(row.pr_id);
      navigate(`/review/${id}`);
    } catch (err) {
      setError(parseError(err).message);
    }
  };

  const handleRefreshMetadata = async (prId: string) => {
    setRefreshingPrIds((c) => ({ ...c, [prId]: true }));
    try {
      await refreshPrMetadata(prId);
      await fetchOverview();
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setRefreshingPrIds((c) => {
        const n = { ...c };
        delete n[prId];
        return n;
      });
    }
  };

  const handleRerun = async (runId: string, hasUnreviewedUpdates: boolean) => {
    try {
      const id = await rerunReview(runId, {
        triggerSource: "queue",
        reason: hasUnreviewedUpdates ? "head_updated" : "manual",
      });
      navigate(`/review/${id}`);
    } catch (err) {
      setError(parseError(err).message);
    }
  };

  return (
    <div className="h-full flex flex-col overflow-hidden bg-(--color-base)">
      {/* Status strip */}
      {overview && (
        <StatusStrip
          env={overview.environment_summary}
          attentionCount={overview.attention_summary.total_items}
        />
      )}

      {/* Scrollable content */}
      <div className="flex-1 overflow-y-auto">
        {/* Intake section */}
        <div className="border-b border-(--color-border-subtle) px-5 py-4">
          <div className="mb-3 flex items-center gap-3 text-sm">
            <span className="text-xs font-semibold uppercase tracking-wider text-(--color-text-tertiary)">
              New Review
            </span>
          </div>
          <IntakeQuickAction />
        </div>

        <div className="px-5 py-4 space-y-5">
          {error && (
            <div className="flex items-center gap-2 rounded-md border border-(--color-state-alert)/30 bg-(--color-state-alert-bg) px-3 py-2 text-xs text-(--color-state-alert)">
              <XCircle className="h-3.5 w-3.5 shrink-0" />
              {error}
            </div>
          )}

          {loading && !overview && (
            <div className="flex items-center justify-center py-16">
              <Loader2 className="h-5 w-5 animate-spin text-(--color-text-tertiary)" />
            </div>
          )}

          {overview && (
            <>
              {/* Filter bar */}
              <FilterBar
                filters={filters}
                repoOptions={repoOptions}
                platformOptions={platformOptions}
                providerOptions={providerOptions}
                onChange={setFilters}
                onReset={() => setFilters(defaultFilters)}
              />

              {/* Queue sections */}
              <div className="space-y-5">
                {visibleSections.map((section: InboxSection) => (
                  <section key={section.id}>
                    <div className="mb-2 flex items-center gap-2">
                      <h2 className="text-[11px] font-semibold uppercase tracking-wider text-(--color-text-tertiary)">
                        {section.title}
                      </h2>
                      <span className="text-[11px] text-(--color-text-tertiary) tabular-nums">
                        {section.items.length}
                      </span>
                    </div>
                    <div className="space-y-1.5">
                      {section.items.map((row) => (
                        <ReviewRow
                          key={row.pr_id}
                          row={row}
                          refreshBusy={Boolean(refreshingPrIds[row.pr_id])}
                          onOpen={() => navigate(`/review/${row.run_id}`)}
                          onStart={() => handleStart(row)}
                          onRerun={() =>
                            handleRerun(row.run_id, row.review_freshness.has_unreviewed_updates)
                          }
                          onResume={() => handleResume(row.run_id)}
                          onRefresh={() => handleRefreshMetadata(row.pr_id)}
                        />
                      ))}
                    </div>
                  </section>
                ))}

                {visibleSections.length === 0 && (
                  <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-(--color-border) py-16 text-center">
                    <p className="text-sm text-(--color-text-secondary)">
                      No items match your filters
                    </p>
                    <p className="mt-1 text-xs text-(--color-text-tertiary)">
                      Clear the filters or start a new review.
                    </p>
                  </div>
                )}
              </div>

              {/* Recent workspaces */}
              {overview.recent_workspaces.length > 0 && (
                <section>
                  <h2 className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-(--color-text-tertiary)">
                    Recent Workspaces
                  </h2>
                  <div className="space-y-1">
                    {overview.recent_workspaces.map((ws) => {
                      const repoKey = `${ws.remote_owner}/${ws.remote_repo}`;
                      const active = filters.repo === repoKey;
                      return (
                        <button
                          key={ws.workspace_id}
                          type="button"
                          onClick={() =>
                            setFilters((f) => ({ ...f, repo: active ? "all" : repoKey }))
                          }
                          title={active ? "Clear repo filter" : `Filter inbox to ${repoKey}`}
                          className={`flex w-full items-center gap-3 rounded-md border px-3 py-2 text-left transition-colors ${
                            active
                              ? "border-(--color-accent)/40 bg-(--color-accent-subtle)"
                              : "border-(--color-border-subtle) hover:border-(--color-border) hover:bg-(--color-elevated)"
                          }`}
                        >
                          <span className="text-xs font-medium text-(--color-text-primary)">
                            {repoKey}
                          </span>
                          <code className="flex-1 truncate text-[11px] text-(--color-text-tertiary) font-mono">
                            {ws.local_path}
                          </code>
                          <span className="text-[11px] text-(--color-text-tertiary) shrink-0">
                            {relativeTime(ws.last_reviewed_at)}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                </section>
              )}
            </>
          )}
        </div>
      </div>
    </div>
  );
}
