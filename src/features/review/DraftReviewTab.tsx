import { useState, useEffect, useRef, useCallback } from "react";
import { Send, Loader2, AlertTriangle, FileText, MessageSquare } from "lucide-react";
import {
  getEnvironmentSummary,
  getReviewDraft,
  saveReviewDraft,
  submitReview,
  parseError,
} from "../../lib/ipc";
import { useReviewContext } from "../../lib/store";
import {
  getPlatformAuthDiagnostic,
  getPlatformCapability,
  isPlatformAuthReady,
  type PlatformCapabilityKey,
  type ReviewAction,
} from "../../lib/types";
import { buildFindingTrustViewModel } from "../../lib/trust";

const AUTOSAVE_DELAY = 1500;

const actions: { value: ReviewAction; label: string; description: string }[] = [
  { value: "comment", label: "Comment", description: "Leave feedback without explicit approval" },
  { value: "approve", label: "Approve", description: "Approve the pull request" },
  {
    value: "request-changes",
    label: "Request Changes",
    description: "Block merging until addressed",
  },
];

const actionCapabilities: Record<ReviewAction, PlatformCapabilityKey> = {
  comment: "review_summary_comment",
  approve: "approve_review",
  "request-changes": "request_changes_review",
};

export function DraftReviewTab({ runId, onSubmitted }: { runId: string; onSubmitted: () => void }) {
  const { state } = useReviewContext();
  const [summary, setSummary] = useState("");
  const [action, setAction] = useState<ReviewAction>("comment");
  const [loadingDraft, setLoadingDraft] = useState(true);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [lastSaved, setLastSaved] = useState<string | null>(null);
  const [environmentSummary, setEnvironmentSummary] = useState<Awaited<
    ReturnType<typeof getEnvironmentSummary>
  > | null>(null);
  const autosaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const draftLoaded = useRef(false);
  const summaryRef = useRef(summary);
  const actionRef = useRef<ReviewAction>(action);

  useEffect(() => {
    let cancelled = false;
    setLoadingDraft(true);
    getReviewDraft(runId)
      .then((draft) => {
        if (cancelled) return;
        if (draft) {
          setSummary(draft.summary_markdown);
          setAction(draft.review_action as ReviewAction);
          summaryRef.current = draft.summary_markdown;
          actionRef.current = draft.review_action as ReviewAction;
        }
        draftLoaded.current = true;
      })
      .catch(() => {
        draftLoaded.current = true;
      })
      .finally(() => {
        if (!cancelled) setLoadingDraft(false);
      });
    return () => {
      cancelled = true;
    };
  }, [runId]);

  const doSave = useCallback(
    async (text: string, act: string) => {
      try {
        await saveReviewDraft(runId, text, act);
        setLastSaved(new Date().toLocaleTimeString());
      } catch {
        // autosave is best-effort
      }
    },
    [runId],
  );

  const handleSummaryChange = (text: string) => {
    setSummary(text);
    summaryRef.current = text;
    if (autosaveTimer.current) clearTimeout(autosaveTimer.current);
    autosaveTimer.current = setTimeout(() => {
      doSave(summaryRef.current, actionRef.current);
    }, AUTOSAVE_DELAY);
  };

  const handleActionChange = (act: ReviewAction) => {
    setAction(act);
    actionRef.current = act;
    if (draftLoaded.current) {
      doSave(summaryRef.current, act);
    }
  };

  useEffect(() => {
    let cancelled = false;
    getEnvironmentSummary()
      .then((summary) => {
        if (!cancelled) {
          setEnvironmentSummary(summary);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnvironmentSummary(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    return () => {
      if (autosaveTimer.current) clearTimeout(autosaveTimer.current);
    };
  }, []);

  const handleSubmit = async (forceResubmit?: boolean) => {
    setSubmitting(true);
    setError(null);
    try {
      await saveReviewDraft(runId, summary, action);
      await submitReview(runId, action, forceResubmit, summary || undefined);
      onSubmitted();
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setSubmitting(false);
    }
  };

  const activeFindings = state.findings.filter((f) => f.status === "active");
  const includedFindings = activeFindings.filter((f) => state.sessionDecisions[f.id] !== "skip");
  const anchoredCount = includedFindings.filter(
    (f) => f.diff_new_line !== null && f.file_path !== null,
  ).length;
  const findingsByFile = includedFindings.reduce<Record<string, typeof includedFindings>>(
    (acc, f) => {
      const key = f.file_path ?? "(no file)";
      if (!acc[key]) acc[key] = [];
      acc[key].push(f);
      return acc;
    },
    {},
  );

  const isReady = state.status === "ready";
  const isSubmitted = state.status === "submitted";
  const hasPartialLanes = state.laneStatuses.some(
    (l) => l.status === "failed" || l.status === "timed_out",
  );
  const hasStaleAnchors = includedFindings.some((f) => !f.is_anchored && f.file_path);
  const hasAiOnlyFindings = includedFindings.some((finding) =>
    buildFindingTrustViewModel(finding, {
      platformMetadata: state.platformMetadata,
    }).warningBadges.some((badge) => badge.key === "ai-only"),
  );
  const selectedActionCapability = getPlatformCapability(
    state.platformCapabilities,
    actionCapabilities[action],
  );
  const draftBatchCapability = getPlatformCapability(
    state.platformCapabilities,
    "pending_comment_batch",
  );
  const platformId =
    state.platformCapabilities?.platform ?? state.platformMetadata?.platform ?? null;
  const authReady = isPlatformAuthReady(platformId, environmentSummary);
  const authDiagnostic = getPlatformAuthDiagnostic(platformId, environmentSummary);
  const capabilityMetadataMissing = selectedActionCapability === null;
  const hasSubmitContent = summary.trim().length > 0 || includedFindings.length > 0;
  const canSubmit =
    hasSubmitContent &&
    authReady &&
    selectedActionCapability !== null &&
    selectedActionCapability.support !== "none";

  if (loadingDraft) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="w-5 h-5 animate-spin text-zinc-500" />
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 overflow-y-auto p-4 space-y-5">
        {/* Summary editor */}
        <section>
          <div className="flex items-center gap-2 mb-2">
            <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider">
              Review summary
            </h3>
            {lastSaved && <span className="text-xs text-zinc-600">Saved {lastSaved}</span>}
          </div>
          <textarea
            value={summary}
            onChange={(e) => handleSummaryChange(e.target.value)}
            placeholder="Write an optional summary for the top of your review..."
            rows={4}
            className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-200 placeholder-zinc-600 focus:outline-none focus:border-zinc-500 resize-y"
          />
        </section>

        {/* Pending comments preview */}
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Pending comments ({includedFindings.length})
          </h3>
          {includedFindings.length === 0 ? (
            <p className="text-zinc-500 text-sm">No active findings to include in the review.</p>
          ) : (
            <div className="space-y-3">
              <div className="flex items-center gap-3 text-xs text-zinc-400">
                <span className="flex items-center gap-1">
                  <MessageSquare className="w-3 h-3" />
                  {anchoredCount} inline comment{anchoredCount !== 1 ? "s" : ""}
                </span>
                <span className="flex items-center gap-1">
                  <FileText className="w-3 h-3" />
                  {Object.keys(findingsByFile).length} file
                  {Object.keys(findingsByFile).length !== 1 ? "s" : ""}
                </span>
              </div>
              {Object.entries(findingsByFile).map(([file, findings]) => (
                <div key={file} className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
                  <code className="text-xs text-zinc-400 block mb-2">{file}</code>
                  <div className="space-y-1.5">
                    {findings.map((f) => {
                      const sev = (f.user_severity_override ?? f.severity).toUpperCase();
                      const trust = buildFindingTrustViewModel(f, {
                        platformMetadata: state.platformMetadata,
                      });
                      return (
                        <div
                          key={f.id}
                          className="rounded border border-zinc-800/50 bg-zinc-950/30 p-2"
                        >
                          <div className="flex items-start gap-2 text-xs">
                            <span className="text-zinc-500 shrink-0 font-mono">
                              {f.is_anchored && f.line_start ? `L${f.line_start}` : "—"}
                            </span>
                            <span className="text-zinc-400">[{sev}]</span>
                            <span className="text-zinc-300 truncate">{f.title}</span>
                          </div>
                          <div className="mt-1 flex gap-1.5 flex-wrap">
                            {trust.provenanceBadges.map((badge) => (
                              <span
                                key={badge.key}
                                className="text-[11px] px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-300"
                              >
                                {badge.label}
                              </span>
                            ))}
                            {trust.supportBadges.map((badge) => (
                              <span
                                key={badge.key}
                                className="text-[11px] px-1.5 py-0.5 rounded bg-sky-950/40 text-sky-300"
                              >
                                {badge.label}
                              </span>
                            ))}
                            {trust.warningBadges.map((badge) => (
                              <span
                                key={badge.key}
                                className="text-[11px] px-1.5 py-0.5 rounded bg-yellow-900/30 text-yellow-300"
                              >
                                {badge.label}
                              </span>
                            ))}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </div>
              ))}
            </div>
          )}
        </section>

        {/* Warnings */}
        {(hasPartialLanes ||
          hasStaleAnchors ||
          hasAiOnlyFindings ||
          !authReady ||
          capabilityMetadataMissing ||
          selectedActionCapability?.support === "partial" ||
          draftBatchCapability?.support !== "full") && (
          <section className="space-y-2">
            {hasPartialLanes && (
              <div className="flex items-center gap-2 text-xs text-yellow-400 bg-yellow-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                Some analysis lanes failed. The review includes only findings from completed lanes.
              </div>
            )}
            {hasStaleAnchors && (
              <div className="flex items-center gap-2 text-xs text-yellow-400 bg-yellow-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                Some findings have stale line anchors and will appear as body text only.
              </div>
            )}
            {hasAiOnlyFindings && (
              <div className="flex items-center gap-2 text-xs text-yellow-400 bg-yellow-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                Some findings rely on AI inference without deterministic support. Review the
                evidence trail before submitting.
              </div>
            )}
            {!authReady && (
              <div className="flex items-center gap-2 text-xs text-red-300 bg-red-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                {authDiagnostic ?? "Platform authentication is not ready for submission."}
              </div>
            )}
            {capabilityMetadataMissing && (
              <div className="flex items-center gap-2 text-xs text-yellow-400 bg-yellow-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                Refresh platform metadata to load the available review actions for this pull
                request.
              </div>
            )}
            {selectedActionCapability?.support === "partial" && (
              <div className="flex items-center gap-2 text-xs text-yellow-400 bg-yellow-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                {selectedActionCapability.constraints[0]?.message ??
                  "This platform only partially supports the selected review action."}
              </div>
            )}
            {draftBatchCapability && draftBatchCapability.support !== "full" && (
              <div className="flex items-center gap-2 text-xs text-yellow-400 bg-yellow-900/20 px-3 py-2 rounded-lg">
                <AlertTriangle className="w-3.5 h-3.5 shrink-0" />
                {draftBatchCapability.constraints[0]?.message ??
                  "This platform does not preserve pending review batches the same way SignalPR drafts do."}
              </div>
            )}
          </section>
        )}

        {/* Review action selector */}
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Review action
          </h3>
          <div className="space-y-1.5">
            {actions.map((a) => {
              const capability = getPlatformCapability(
                state.platformCapabilities,
                actionCapabilities[a.value],
              );
              const disabled = capability === null || capability.support === "none";
              return (
                <label
                  key={a.value}
                  className={`flex items-start gap-3 p-3 rounded-lg border ${
                    disabled ? "opacity-50 cursor-not-allowed" : "cursor-pointer"
                  } ${
                    action === a.value
                      ? "border-emerald-500/50 bg-emerald-900/10"
                      : "border-zinc-800 hover:border-zinc-700"
                  }`}
                >
                  <input
                    type="radio"
                    name="draft-action"
                    value={a.value}
                    checked={action === a.value}
                    onChange={() => !disabled && handleActionChange(a.value)}
                    disabled={disabled}
                    className="mt-0.5"
                  />
                  <div>
                    <div className="text-sm font-medium text-zinc-100">{a.label}</div>
                    <div className="text-xs text-zinc-400">{a.description}</div>
                    {capability?.support === "partial" && capability.constraints[0] && (
                      <div className="mt-1 text-[11px] text-yellow-300">
                        {capability.constraints[0].message}
                      </div>
                    )}
                    {capability?.support === "none" && capability.constraints[0] && (
                      <div className="mt-1 text-[11px] text-red-300">
                        {capability.constraints[0].message}
                      </div>
                    )}
                  </div>
                </label>
              );
            })}
          </div>
        </section>
      </div>

      {/* Footer submit bar */}
      <div className="border-t border-zinc-800 px-4 py-3 shrink-0">
        {error && <p className="text-red-400 text-sm mb-2">{error}</p>}

        <div className="flex items-center gap-2">
          {isSubmitted && <span className="text-xs text-zinc-500">Already submitted.</span>}
          {!isSubmitted && !authReady && (
            <span className="text-xs text-red-400">
              {authDiagnostic ?? "Platform authentication is not ready for submission."}
            </span>
          )}
          {!isSubmitted && authReady && capabilityMetadataMissing && (
            <span className="text-xs text-yellow-400">
              Refresh platform metadata to load the available review actions.
            </span>
          )}
          {!isSubmitted && selectedActionCapability?.support === "none" && (
            <span className="text-xs text-red-400">
              {selectedActionCapability.constraints[0]?.message ??
                "This action is not available on the active platform."}
            </span>
          )}
          <div className="flex-1" />
          {isSubmitted && (
            <button
              onClick={() => handleSubmit(true)}
              disabled={submitting}
              className="flex items-center gap-1.5 px-4 py-2 text-sm border border-zinc-700 rounded-lg text-zinc-300 hover:bg-zinc-800 disabled:opacity-50"
            >
              {submitting && <Loader2 className="w-3 h-3 animate-spin" />}
              Force resubmit
            </button>
          )}
          {isReady && !isSubmitted && (
            <button
              onClick={() => handleSubmit()}
              disabled={submitting || !canSubmit}
              className="flex items-center gap-1.5 bg-emerald-600 text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-emerald-500 disabled:opacity-50"
            >
              {submitting ? (
                <Loader2 className="w-3 h-3 animate-spin" />
              ) : (
                <Send className="w-3 h-3" />
              )}
              Submit review ({includedFindings.length})
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
