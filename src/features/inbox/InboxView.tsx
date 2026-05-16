import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router";
import {
  AlertTriangle,
  CheckCircle,
  Eye,
  Filter,
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
} from "../../lib/ipc";
import { IntakeQuickAction } from "../intake/IntakeQuickAction";
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

function queueBadge(queueState: string) {
  const styles: Record<string, string> = {
    needs_your_review: "bg-blue-950/60 text-blue-300 border-blue-900/60",
    updated_since_review: "bg-cyan-950/60 text-cyan-300 border-cyan-900/60",
    review_requested: "bg-sky-950/60 text-sky-300 border-sky-900/60",
    attention_needed: "bg-red-950/60 text-red-300 border-red-900/60",
    in_progress: "bg-amber-950/60 text-amber-300 border-amber-900/60",
    ready_to_submit: "bg-emerald-950/60 text-emerald-300 border-emerald-900/60",
    waiting_on_author: "bg-violet-950/60 text-violet-300 border-violet-900/60",
    submitted_recently: "bg-zinc-900 text-zinc-300 border-zinc-800",
  };
  const labels: Record<string, string> = {
    needs_your_review: "Needs your review",
    updated_since_review: "Updated since review",
    review_requested: "Review requested",
    attention_needed: "Attention needed",
    in_progress: "In progress",
    ready_to_submit: "Ready to submit",
    waiting_on_author: "Waiting on author",
    submitted_recently: "Submitted recently",
  };

  return (
    <span
      className={`inline-flex items-center rounded-md border px-2 py-0.5 text-[11px] font-medium ${styles[queueState] ?? styles.submitted_recently}`}
    >
      {labels[queueState] ?? queueState}
    </span>
  );
}

function statusBadge(status: string) {
  const styles: Record<string, string> = {
    ready: "bg-emerald-950/50 text-emerald-400",
    submitted: "bg-blue-950/50 text-blue-400",
    failed: "bg-red-950/50 text-red-400",
    running_agents: "bg-amber-950/50 text-amber-400",
    cleaning: "bg-amber-950/50 text-amber-400",
    created: "bg-zinc-800 text-zinc-300",
  };
  const labels: Record<string, string> = {
    running_agents: "Analyzing",
    created: "Queued",
  };

  return (
    <span
      className={`inline-flex items-center rounded-md px-1.5 py-0.5 text-[11px] ${styles[status] ?? "bg-zinc-800 text-zinc-400"}`}
    >
      {labels[status] ?? status}
    </span>
  );
}

function formatTimestamp(value: string | null | undefined) {
  if (!value) return "Unknown";
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function matchesFilters(row: InboxReviewRow, filters: Filters) {
  if (filters.repo !== "all" && `${row.repo_owner}/${row.repo_name}` !== filters.repo) {
    return false;
  }
  if (filters.platform !== "all" && row.platform !== filters.platform) {
    return false;
  }
  if (filters.provider !== "all" && !row.providers_used.includes(filters.provider)) {
    return false;
  }
  if (filters.attentionOnly && row.attention_reasons.length === 0) {
    return false;
  }
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

function ReadinessBanner({
  env,
  attentionCount,
}: {
  env: EnvironmentSummary;
  attentionCount: number;
}) {
  const readyCount = env.tools.filter((tool) => tool.status === "ready").length;
  const total = env.tools.length;
  const allGood = env.warnings.length === 0 && attentionCount === 0;

  return (
    <section
      className={`rounded-lg border px-4 py-3 ${
        allGood ? "border-emerald-900/60 bg-emerald-950/20" : "border-amber-900/60 bg-amber-950/20"
      }`}
    >
      <div className="flex flex-wrap items-center gap-2">
        {allGood ? (
          <CheckCircle className="h-4 w-4 text-emerald-400" />
        ) : (
          <AlertTriangle className="h-4 w-4 text-amber-400" />
        )}
        <span className="text-sm font-medium text-zinc-100">
          {allGood ? "Ready to review" : "Queue or setup needs attention"}
        </span>
        <span className="ml-auto text-xs text-zinc-500">
          {readyCount}/{total} tools ready
        </span>
      </div>

      <div className="mt-2 flex flex-wrap gap-2 text-xs">
        {attentionCount > 0 && (
          <span className="rounded-md bg-red-950/50 px-2 py-1 text-red-300">
            {attentionCount} PR{attentionCount === 1 ? "" : "s"} need attention
          </span>
        )}
        {env.available_providers.length > 0 && (
          <span className="rounded-md bg-zinc-900 px-2 py-1 text-zinc-300">
            Providers: {env.available_providers.join(", ")}
          </span>
        )}
      </div>

      {env.warnings.length > 0 && (
        <ul className="mt-3 space-y-1">
          {env.warnings.map((warning) => (
            <li key={warning} className="flex items-center gap-1.5 text-xs text-amber-300">
              <XCircle className="h-3 w-3 shrink-0" />
              {warning}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

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
  return (
    <section className="rounded-lg border border-zinc-800 bg-zinc-950/70 p-3">
      <div className="mb-3 flex items-center gap-2 text-sm text-zinc-300">
        <Filter className="h-4 w-4 text-zinc-500" />
        Queue filters
      </div>

      <div className="grid gap-3 md:grid-cols-[minmax(0,2fr)_repeat(3,minmax(0,1fr))]">
        <label className="relative block">
          <Search className="absolute left-3 top-2.5 h-4 w-4 text-zinc-600" />
          <input
            value={filters.query}
            onChange={(event) => onChange({ ...filters, query: event.target.value })}
            placeholder="Search PR, author, repo, workspace..."
            className="w-full rounded-lg border border-zinc-800 bg-zinc-900 py-2 pl-9 pr-3 text-sm text-zinc-100 placeholder:text-zinc-600 focus:border-zinc-700 focus:outline-none"
          />
        </label>

        <select
          value={filters.repo}
          onChange={(event) => onChange({ ...filters, repo: event.target.value })}
          className="rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 focus:border-zinc-700 focus:outline-none"
        >
          <option value="all">All repos</option>
          {repoOptions.map((repo) => (
            <option key={repo} value={repo}>
              {repo}
            </option>
          ))}
        </select>

        <select
          value={filters.platform}
          onChange={(event) => onChange({ ...filters, platform: event.target.value })}
          className="rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 focus:border-zinc-700 focus:outline-none"
        >
          <option value="all">All platforms</option>
          {platformOptions.map((platform) => (
            <option key={platform} value={platform}>
              {platform}
            </option>
          ))}
        </select>

        <select
          value={filters.provider}
          onChange={(event) => onChange({ ...filters, provider: event.target.value })}
          className="rounded-lg border border-zinc-800 bg-zinc-900 px-3 py-2 text-sm text-zinc-100 focus:border-zinc-700 focus:outline-none"
        >
          <option value="all">All providers</option>
          {providerOptions.map((provider) => (
            <option key={provider} value={provider}>
              {provider}
            </option>
          ))}
        </select>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-sm text-zinc-300">
          <input
            type="checkbox"
            checked={filters.attentionOnly}
            onChange={(event) => onChange({ ...filters, attentionOnly: event.target.checked })}
            className="h-4 w-4 rounded border-zinc-700 bg-zinc-900"
          />
          Attention only
        </label>
        <button
          onClick={onReset}
          className="text-xs text-zinc-400 transition-colors hover:text-zinc-200"
        >
          Reset filters
        </button>
      </div>
    </section>
  );
}

function ReviewRow({
  row,
  refreshBusy,
  onOpen,
  onRerun,
  onResume,
  onRefresh,
}: {
  row: InboxReviewRow;
  refreshBusy: boolean;
  onOpen: () => void;
  onRerun: () => void;
  onResume: () => void;
  onRefresh: () => void;
}) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-950/70 p-3 transition-colors hover:border-zinc-700">
      <div className="flex flex-col gap-3 lg:flex-row lg:items-start">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-xs text-zinc-500">#{row.pr_number}</span>
            <h3 className="truncate text-sm font-medium text-zinc-100">{row.title}</h3>
            {queueBadge(row.queue_state)}
            {statusBadge(row.status)}
            {row.draft && (
              <span className="rounded-md bg-zinc-900 px-1.5 py-0.5 text-[11px] text-zinc-400">
                PR draft
              </span>
            )}
            {row.has_saved_review_draft && (
              <span className="rounded-md bg-zinc-900 px-1.5 py-0.5 text-[11px] text-zinc-400">
                Review draft
              </span>
            )}
          </div>

          <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-zinc-500">
            <span>
              {row.repo_owner}/{row.repo_name}
            </span>
            {row.author && <span>{row.author}</span>}
            <span>{row.active_finding_count} active findings</span>
            {row.providers_used.length > 0 && <span>{row.providers_used.join(", ")}</span>}
            <span>{formatTimestamp(row.last_updated)}</span>
          </div>

          <div className="mt-3 flex flex-wrap gap-2 text-[11px]">
            <span className="rounded-md bg-zinc-900 px-2 py-1 text-zinc-300">
              Reviewer signal: {row.reviewer_signal.label}
            </span>
            <span className="rounded-md bg-zinc-900 px-2 py-1 text-zinc-300">
              Lane health: {row.lane_health.state}
            </span>
            <span className="rounded-md bg-zinc-900 px-2 py-1 text-zinc-300">
              Submission: {row.submission_health.state}
            </span>
            <span className="rounded-md bg-zinc-900 px-2 py-1 text-zinc-300">
              Review freshness: {row.review_freshness.state}
            </span>
            {row.metadata_freshness.is_stale && (
              <span className="rounded-md bg-red-950/40 px-2 py-1 text-red-300">
                Metadata stale
              </span>
            )}
          </div>

          {row.attention_reasons.length > 0 && (
            <ul className="mt-3 space-y-1">
              {row.attention_reasons.map((reason) => (
                <li key={reason} className="flex items-center gap-1.5 text-xs text-amber-300">
                  <AlertTriangle className="h-3 w-3 shrink-0" />
                  {reason}
                </li>
              ))}
            </ul>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-2">
          {row.allowed_actions.includes("refresh_metadata") && (
            <button
              onClick={onRefresh}
              disabled={refreshBusy}
              className="inline-flex items-center gap-1 rounded-lg border border-zinc-800 bg-zinc-900 px-2.5 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-700 hover:text-zinc-100 disabled:opacity-50"
            >
              {refreshBusy ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <RefreshCcw className="h-3.5 w-3.5" />
              )}
              Refresh
            </button>
          )}

          {row.allowed_actions.includes("resume") && (
            <button
              onClick={onResume}
              className="inline-flex items-center gap-1 rounded-lg border border-zinc-800 bg-zinc-900 px-2.5 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-700 hover:text-zinc-100"
            >
              <RotateCcw className="h-3.5 w-3.5" />
              Resume
            </button>
          )}

          {row.allowed_actions.includes("rerun") && (
            <button
              onClick={onRerun}
              className="inline-flex items-center gap-1 rounded-lg border border-zinc-800 bg-zinc-900 px-2.5 py-1.5 text-xs text-zinc-300 transition-colors hover:border-zinc-700 hover:text-zinc-100"
            >
              <RotateCcw className="h-3.5 w-3.5" />
              Rerun
            </button>
          )}

          <button
            onClick={onOpen}
            className="inline-flex items-center gap-1 rounded-lg bg-zinc-100 px-2.5 py-1.5 text-xs font-medium text-zinc-900 transition-colors hover:bg-zinc-200"
          >
            <Eye className="h-3.5 w-3.5" />
            Open
          </button>
        </div>
      </div>
    </div>
  );
}

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

  const allRows = useMemo(
    () => overview?.sections.flatMap((section) => section.items) ?? [],
    [overview],
  );

  const repoOptions = useMemo(
    () =>
      Array.from(new Set(allRows.map((row) => `${row.repo_owner}/${row.repo_name}`))).sort((a, b) =>
        a.localeCompare(b),
      ),
    [allRows],
  );
  const platformOptions = useMemo(
    () =>
      Array.from(
        new Set(allRows.map((row) => row.platform).filter((platform) => platform !== "unknown")),
      ).sort(),
    [allRows],
  );
  const providerOptions = useMemo(
    () =>
      Array.from(new Set(allRows.flatMap((row) => row.providers_used))).sort((a, b) =>
        a.localeCompare(b),
      ),
    [allRows],
  );

  const visibleSections = useMemo(() => {
    if (!overview) return [];
    return overview.sections
      .map((section) => ({
        ...section,
        items: section.items.filter((row) => matchesFilters(row, filters)),
      }))
      .filter((section) => section.items.length > 0);
  }, [filters, overview]);

  const handleResume = async (runId: string) => {
    try {
      const resumedRunId = await resumeReview(runId);
      navigate(`/review/${resumedRunId}`);
    } catch (err) {
      setError(parseError(err).message);
    }
  };

  const handleRefreshMetadata = async (prId: string) => {
    setRefreshingPrIds((current) => ({ ...current, [prId]: true }));
    try {
      await refreshPrMetadata(prId);
      await fetchOverview();
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setRefreshingPrIds((current) => {
        const next = { ...current };
        delete next[prId];
        return next;
      });
    }
  };

  const handleRerun = async (runId: string, hasUnreviewedUpdates: boolean) => {
    try {
      const newRunId = await rerunReview(runId, {
        triggerSource: "queue",
        reason: hasUnreviewedUpdates ? "head_updated" : "manual",
      });
      navigate(`/review/${newRunId}`);
    } catch (err) {
      setError(parseError(err).message);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-6xl space-y-6 p-6">
        <section>
          <h2 className="mb-2 text-xs font-medium uppercase tracking-wider text-zinc-500">
            New review
          </h2>
          <IntakeQuickAction />
        </section>

        {loading && !overview && (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="h-5 w-5 animate-spin text-zinc-500" />
          </div>
        )}

        {error && <p className="text-sm text-red-400">{error}</p>}

        {overview && (
          <>
            <ReadinessBanner
              env={overview.environment_summary}
              attentionCount={overview.attention_summary.total_items}
            />

            <FilterBar
              filters={filters}
              repoOptions={repoOptions}
              platformOptions={platformOptions}
              providerOptions={providerOptions}
              onChange={setFilters}
              onReset={() => setFilters(defaultFilters)}
            />

            <div className="space-y-5">
              {visibleSections.map((section: InboxSection) => (
                <section key={section.id}>
                  <div className="mb-2 flex items-center justify-between">
                    <h2 className="text-xs font-medium uppercase tracking-wider text-zinc-500">
                      {section.title}
                    </h2>
                    <span className="text-xs text-zinc-600">{section.items.length}</span>
                  </div>
                  <div className="space-y-2">
                    {section.items.map((row) => (
                      <ReviewRow
                        key={row.pr_id}
                        row={row}
                        refreshBusy={Boolean(refreshingPrIds[row.pr_id])}
                        onOpen={() => navigate(`/review/${row.run_id}`)}
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
                <section className="rounded-lg border border-dashed border-zinc-800 bg-zinc-950/40 px-5 py-10 text-center">
                  <p className="text-sm text-zinc-300">No queue items match the current filters.</p>
                  <p className="mt-1 text-xs text-zinc-500">
                    Clear the filters or start a new review to populate the inbox.
                  </p>
                </section>
              )}
            </div>

            {overview.recent_workspaces.length > 0 && (
              <section>
                <div className="mb-2 flex items-center justify-between">
                  <h2 className="text-xs font-medium uppercase tracking-wider text-zinc-500">
                    Recent workspaces
                  </h2>
                  <span className="text-xs text-zinc-600">{overview.recent_workspaces.length}</span>
                </div>
                <div className="space-y-2">
                  {overview.recent_workspaces.map((workspace) => (
                    <div
                      key={workspace.workspace_id}
                      className="rounded-lg border border-zinc-800 bg-zinc-950/60 px-3 py-2.5"
                    >
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-zinc-100">
                          {workspace.remote_owner}/{workspace.remote_repo}
                        </span>
                        <span className="ml-auto text-xs text-zinc-500">
                          {formatTimestamp(workspace.last_reviewed_at)}
                        </span>
                      </div>
                      <code className="mt-1 block truncate text-xs text-zinc-500">
                        {workspace.local_path}
                      </code>
                    </div>
                  ))}
                </div>
              </section>
            )}
          </>
        )}
      </div>
    </div>
  );
}
