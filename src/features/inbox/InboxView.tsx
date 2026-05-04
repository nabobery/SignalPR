import { useEffect, useState } from "react";
import { useNavigate } from "react-router";
import { Loader2, CheckCircle, XCircle, AlertTriangle, RotateCcw, Eye } from "lucide-react";
import { getInboxOverview, resumeReview, parseError } from "../../lib/ipc";
import { IntakeQuickAction } from "../intake/IntakeQuickAction";
import type { InboxOverview, InboxReviewRow, EnvironmentSummary } from "../../lib/types";

function statusBadge(status: string) {
  switch (status) {
    case "ready":
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-emerald-900/30 text-emerald-400">
          Ready
        </span>
      );
    case "submitted":
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-blue-900/30 text-blue-400">
          Submitted
        </span>
      );
    case "failed":
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-red-900/30 text-red-400">Failed</span>
      );
    case "running_agents":
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-yellow-900/30 text-yellow-400">
          Analyzing
        </span>
      );
    case "cleaning":
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-yellow-900/30 text-yellow-400">
          Cleaning
        </span>
      );
    default:
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-400">{status}</span>
      );
  }
}

function ReviewRow({
  row,
  onOpen,
  onRestart,
  isIncomplete,
}: {
  row: InboxReviewRow;
  onOpen: () => void;
  onRestart?: () => void;
  isIncomplete?: boolean;
}) {
  return (
    <div className="flex items-center gap-3 px-3 py-2.5 rounded-lg bg-zinc-900/50 border border-zinc-800/50 hover:border-zinc-700 transition-colors">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-zinc-500 text-xs">#{row.pr_number}</span>
          <span className="text-sm font-medium text-zinc-100 truncate">{row.title}</span>
          {statusBadge(row.status)}
        </div>
        <div className="flex items-center gap-3 mt-0.5">
          {row.author && <span className="text-xs text-zinc-500">{row.author}</span>}
          {row.active_finding_count > 0 && (
            <span className="text-xs text-zinc-400">
              {row.active_finding_count} finding{row.active_finding_count !== 1 ? "s" : ""}
            </span>
          )}
          {row.providers_used.length > 0 && (
            <span className="text-xs text-zinc-600">{row.providers_used.join(", ")}</span>
          )}
        </div>
      </div>
      <div className="flex items-center gap-1.5 shrink-0">
        {isIncomplete && onRestart && (
          <button
            onClick={onRestart}
            className="flex items-center gap-1 text-xs px-2 py-1 rounded bg-zinc-800 text-zinc-300 hover:bg-zinc-700 hover:text-zinc-100"
            title="Restart review (starts a fresh run)"
          >
            <RotateCcw className="w-3 h-3" /> Restart
          </button>
        )}
        <button
          onClick={onOpen}
          className="flex items-center gap-1 text-xs px-2 py-1 rounded bg-zinc-800 text-zinc-300 hover:bg-zinc-700 hover:text-zinc-100"
        >
          <Eye className="w-3 h-3" /> Open
        </button>
      </div>
    </div>
  );
}

function ReadinessCard({ env }: { env: EnvironmentSummary }) {
  const readyCount = env.tools.filter((t) => t.status === "ready").length;
  const total = env.tools.length;
  const allGood = readyCount === total && env.warnings.length === 0;

  return (
    <div
      className={`rounded-lg border p-3 ${allGood ? "border-emerald-800/50 bg-emerald-950/20" : "border-yellow-800/50 bg-yellow-950/20"}`}
    >
      <div className="flex items-center gap-2 mb-2">
        {allGood ? (
          <CheckCircle className="w-4 h-4 text-emerald-400" />
        ) : (
          <AlertTriangle className="w-4 h-4 text-yellow-400" />
        )}
        <span className="text-sm font-medium text-zinc-100">
          {allGood ? "Ready to review" : "Setup needed"}
        </span>
        <span className="text-xs text-zinc-500 ml-auto">
          {readyCount}/{total} tools ready
        </span>
      </div>
      {env.warnings.length > 0 && (
        <ul className="space-y-1">
          {env.warnings.map((w, i) => (
            <li key={i} className="text-xs text-yellow-400 flex items-center gap-1.5">
              <XCircle className="w-3 h-3 shrink-0" /> {w}
            </li>
          ))}
        </ul>
      )}
      {env.available_providers.length > 0 && (
        <div className="text-xs text-zinc-500 mt-1.5">
          Providers: {env.available_providers.join(", ")}
        </div>
      )}
    </div>
  );
}

export function InboxView() {
  const navigate = useNavigate();
  const [overview, setOverview] = useState<InboxOverview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

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

  const handleRestart = async (runId: string) => {
    try {
      const newRunId = await resumeReview(runId);
      navigate(`/review/${newRunId}`);
    } catch (err) {
      setError(parseError(err).message);
    }
  };

  return (
    <div className="flex-1 overflow-y-auto">
      <div className="max-w-2xl mx-auto p-6 space-y-6">
        {/* Quick action */}
        <section>
          <h2 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            New review
          </h2>
          <IntakeQuickAction />
        </section>

        {loading && !overview && (
          <div className="flex items-center justify-center py-12">
            <Loader2 className="w-5 h-5 animate-spin text-zinc-500" />
          </div>
        )}

        {error && <p className="text-red-400 text-sm">{error}</p>}

        {overview && (
          <>
            {/* Readiness */}
            <section>
              <h2 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
                Environment
              </h2>
              <ReadinessCard env={overview.environment_summary} />
            </section>

            {/* Incomplete reviews */}
            {overview.incomplete_reviews.length > 0 && (
              <section>
                <h2 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
                  In progress ({overview.incomplete_reviews.length})
                </h2>
                <div className="space-y-1.5">
                  {overview.incomplete_reviews.map((row) => (
                    <ReviewRow
                      key={row.run_id}
                      row={row}
                      isIncomplete
                      onOpen={() => navigate(`/review/${row.run_id}`)}
                      onRestart={() => handleRestart(row.run_id)}
                    />
                  ))}
                </div>
              </section>
            )}

            {/* Recent reviews */}
            {overview.recent_reviews.length > 0 && (
              <section>
                <h2 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
                  Recent ({overview.recent_reviews.length})
                </h2>
                <div className="space-y-1.5">
                  {overview.recent_reviews.map((row) => (
                    <ReviewRow
                      key={row.run_id}
                      row={row}
                      onOpen={() => navigate(`/review/${row.run_id}`)}
                    />
                  ))}
                </div>
              </section>
            )}

            {/* Recent workspaces */}
            {overview.recent_workspaces.length > 0 && (
              <section>
                <h2 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
                  Recent workspaces ({overview.recent_workspaces.length})
                </h2>
                <div className="space-y-1.5">
                  {overview.recent_workspaces.map((workspace) => (
                    <div
                      key={workspace.workspace_id}
                      className="px-3 py-2.5 rounded-lg bg-zinc-900/50 border border-zinc-800/50"
                    >
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-zinc-100">
                          {workspace.remote_owner}/{workspace.remote_repo}
                        </span>
                        <span className="text-xs text-zinc-500 ml-auto">
                          {workspace.last_reviewed_at}
                        </span>
                      </div>
                      <code className="text-xs text-zinc-500 block mt-1 truncate">
                        {workspace.local_path}
                      </code>
                    </div>
                  ))}
                </div>
              </section>
            )}

            {overview.incomplete_reviews.length === 0 &&
              overview.recent_reviews.length === 0 &&
              overview.recent_workspaces.length === 0 && (
                <p className="text-zinc-500 text-sm text-center py-8">
                  No reviews yet. Paste a PR URL above to get started.
                </p>
              )}
          </>
        )}
      </div>
    </div>
  );
}
