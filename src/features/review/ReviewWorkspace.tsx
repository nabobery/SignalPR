import { useCallback, useEffect, useMemo, useRef, useState, startTransition } from "react";
import { useParams, useNavigate } from "react-router";
import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, Loader2 } from "lucide-react";
import { getReviewSnapshot, acceptFix, parseError } from "../../lib/ipc";
import { FixBatchBar } from "./FixBatchBar";
import { ReviewContext, type ReviewState } from "../../lib/store";
import { FileTree } from "./FileTree";
import { SignalBoard } from "./SignalBoard";
import { DiffPanel } from "./DiffPanel";
import { SummaryTab } from "./SummaryTab";
import { DiagnosticsTab } from "./DiagnosticsTab";
import LaneProgress from "./LaneProgress";
import { SessionDrawer } from "./SessionDrawer";
import { ApprovalModal } from "./ApprovalModal";
import { DraftReviewTab } from "./DraftReviewTab";
import { normalizeFilePath } from "./diff/normalizeFilePath";

type WorkspaceTab = "summary" | "findings" | "diff" | "draft" | "diagnostics";

export function ReviewWorkspace() {
  const { runId } = useParams<{ runId: string }>();
  const navigate = useNavigate();
  const [state, setState] = useState<ReviewState | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<WorkspaceTab>("summary");
  const [batchBarDismissed, setBatchBarDismissed] = useState(false);

  const switchTab = (tab: WorkspaceTab) => {
    startTransition(() => {
      setActiveTab(tab);
    });
  };

  const refreshSnapshot = useCallback(async () => {
    if (!runId) return;
    try {
      const snap = await getReviewSnapshot(runId);
      setState((prev) => ({
        runId: snap.run_id,
        prId: snap.pr_id,
        workspaceId: snap.workspace_id,
        status: snap.status,
        prTitle: snap.pr_title,
        prNumber: snap.pr_number,
        prUrl: snap.pr_url,
        diffText: snap.diff_text,
        changedFiles: snap.changed_files,
        findings: snap.findings,
        errorMessage: snap.error_message,
        laneStatuses: snap.lane_statuses,
        clusters: snap.clusters,
        baselineRunId: snap.baseline_run_id ?? null,
        metrics: snap.metrics ?? null,
        delta: snap.delta ?? null,
        reviewFreshness: snap.review_freshness,
        contextPackSummary: snap.context_pack_summary ?? null,
        localChecksSummary: snap.local_checks_summary ?? null,
        platformMetadata: snap.platform_metadata ?? null,
        platformMetadataFetchedAt: snap.platform_metadata_fetched_at ?? null,
        platformCapabilities: snap.platform_capabilities ?? null,
        platformCapabilitiesFetchedAt: snap.platform_capabilities_fetched_at ?? null,
        providerSelection: snap.provider_selection ?? null,
        providerControl: snap.provider_control ?? null,
        selectedFile:
          prev?.selectedFile && snap.changed_files.includes(prev.selectedFile)
            ? prev.selectedFile
            : null,
        focusedFindingId: null,
        sessionDecisions: (() => {
          if (snap.decisions_by_finding_id) {
            const seeded: Record<string, "accept" | "skip"> = {};
            for (const [id, d] of Object.entries(snap.decisions_by_finding_id)) {
              if (d === "accept" || d === "skip") seeded[id] = d;
            }
            return seeded;
          }
          const nextIds = new Set(snap.findings.map((f) => f.id));
          const prev_ = prev?.sessionDecisions ?? {};
          return Object.fromEntries(
            Object.entries(prev_).filter(([id]) => nextIds.has(id)),
          ) as Record<string, "accept" | "skip">;
        })(),
      }));
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => {
    refreshSnapshot();
  }, [refreshSnapshot]);

  useEffect(() => {
    let debounce: ReturnType<typeof setTimeout> | null = null;
    const unlisten = listen<{ run_id?: string }>("review_progress", (event) => {
      if (event.payload?.run_id && event.payload.run_id !== runId) return;
      if (debounce) clearTimeout(debounce);
      debounce = setTimeout(() => refreshSnapshot(), 300);
    });
    return () => {
      if (debounce) clearTimeout(debounce);
      unlisten.then((fn) => fn());
    };
  }, [refreshSnapshot, runId]);

  const setSelectedFile = (file: string | null) => {
    setState((prev) => (prev ? { ...prev, selectedFile: file } : prev));
  };

  const setSessionDecision = (findingId: string, decision: "accept" | "skip" | null) => {
    setState((prev) => {
      if (!prev) return prev;
      if (decision === null) {
        const next = { ...prev.sessionDecisions };
        delete next[findingId];
        return { ...prev, sessionDecisions: next };
      }
      return { ...prev, sessionDecisions: { ...prev.sessionDecisions, [findingId]: decision } };
    });
  };

  const knownFilesSet = useMemo(() => new Set(state?.changedFiles ?? []), [state?.changedFiles]);
  const focusTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (focusTimerRef.current) clearTimeout(focusTimerRef.current);
    },
    [],
  );

  const revealFinding = useCallback(
    (findingId: string) => {
      const finding = state?.findings.find((f) => f.id === findingId);
      if (!finding) return;
      if (finding.file_path) setSelectedFile(normalizeFilePath(finding.file_path, knownFilesSet));
      setActiveTab("findings");
      setState((prev) => (prev ? { ...prev, focusedFindingId: findingId } : prev));
      if (focusTimerRef.current) clearTimeout(focusTimerRef.current);
      focusTimerRef.current = setTimeout(() => {
        setState((prev) => (prev ? { ...prev, focusedFindingId: null } : prev));
      }, 2000);
    },
    [state?.findings, knownFilesSet],
  );

  /* ── Loading / error states ── */

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center bg-(--color-base)">
        <Loader2 className="w-5 h-5 animate-spin text-(--color-text-tertiary)" />
      </div>
    );
  }

  if (error || !state) {
    return (
      <div className="h-full flex flex-col items-center justify-center gap-3 bg-(--color-base)">
        <p className="text-sm text-(--color-sev-blocker)">{error ?? "Failed to load review"}</p>
        <button
          onClick={() => navigate("/")}
          className="text-xs text-(--color-text-tertiary) hover:text-(--color-text-secondary) transition-colors"
        >
          Back to inbox
        </button>
      </div>
    );
  }

  const isRunning =
    state.status === "created" || state.status === "running_agents" || state.status === "cleaning";
  const isReady = state.status === "ready" || state.status === "submitted";
  const activeCount = state.findings.filter((f) => f.status === "active").length;
  const pendingFixes = state.findings.filter(
    (f) => f.fix_search && !["applied", "accepted", "rejected"].includes(f.fix_status ?? ""),
  );

  const handleAcceptAllFixes = async () => {
    for (const f of pendingFixes) await acceptFix(f.id);
    await refreshSnapshot();
  };

  type Tab = { id: WorkspaceTab; label: string };
  const tabs: Tab[] = [
    { id: "summary", label: "Summary" },
    { id: "findings", label: `Findings${activeCount > 0 ? ` (${activeCount})` : ""}` },
    { id: "diff", label: "Diff" },
    { id: "draft", label: "Draft Review" },
    { id: "diagnostics", label: "Diagnostics" },
  ];

  const showSidebar = activeTab === "findings" || activeTab === "diff";

  /* ── Running status label ── */

  const runningLabel =
    state.status === "running_agents"
      ? "Analyzing"
      : state.status === "cleaning"
        ? "Cleaning"
        : state.status === "created"
          ? "Queued"
          : null;

  return (
    <ReviewContext.Provider
      value={{ state, setSelectedFile, setSessionDecision, refreshSnapshot, revealFinding }}
    >
      <div className="h-full flex flex-col bg-(--color-base) text-(--color-text-primary) overflow-hidden">
        {/* ── Header ── */}
        <header className="flex items-center gap-3 px-4 h-11 border-b border-(--color-border-subtle) shrink-0">
          <button
            onClick={() => navigate("/")}
            className="flex items-center justify-center w-7 h-7 rounded-md text-(--color-text-tertiary) hover:text-(--color-text-secondary) hover:bg-(--color-elevated) transition-colors"
          >
            <ArrowLeft className="w-3.5 h-3.5" />
          </button>

          <div className="h-4 w-px bg-(--color-border-subtle) shrink-0" />

          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 min-w-0">
              <span className="text-[11px] font-mono text-(--color-text-tertiary) shrink-0">
                #{state.prNumber}
              </span>
              <h1 className="text-sm font-medium truncate text-(--color-text-primary)">
                {state.prTitle}
              </h1>
            </div>
          </div>

          {/* Status indicators */}
          <div className="flex items-center gap-3 shrink-0">
            {isRunning && runningLabel && (
              <span className="flex items-center gap-1.5 text-xs text-(--color-state-progress)">
                <Loader2 className="w-3 h-3 animate-spin" />
                {runningLabel}
              </span>
            )}
            {state.status === "failed" && (
              <span className="text-xs text-(--color-sev-blocker)">
                {state.errorMessage ? `Error: ${state.errorMessage}` : "Analysis failed"}
              </span>
            )}
            {state.status === "submitted" && (
              <span className="text-xs text-(--color-state-ready)">Submitted</span>
            )}
          </div>
        </header>

        {/* ── Lane progress (slim 2px bar) ── */}
        {isRunning && state.laneStatuses.length > 0 && (
          <div className="px-4 py-1.5 border-b border-(--color-border-subtle) shrink-0">
            <LaneProgress lanes={state.laneStatuses} />
          </div>
        )}

        {/* ── Partial success / degraded banners ── */}
        {isReady &&
          state.laneStatuses.some((l) => l.status === "failed" || l.status === "timed_out") && (
            <div className="flex items-center gap-2 px-4 py-1.5 border-b border-(--color-state-alert)/20 bg-(--color-state-alert-bg) shrink-0">
              <p className="text-xs text-(--color-state-alert)">
                {state.laneStatuses.filter((l) => l.status === "completed").length}/
                {state.laneStatuses.length} lanes completed — some lanes failed.
              </p>
            </div>
          )}
        {isReady && state.laneStatuses.every((l) => l.provider_name === "mock") && (
          <div className="flex items-center gap-2 px-4 py-1.5 border-b border-(--color-state-progress)/20 bg-(--color-state-progress-bg) shrink-0">
            <p className="text-xs text-(--color-state-progress)">
              Demo mode — install Codex CLI or set ANTHROPIC_API_KEY for real analysis.
            </p>
          </div>
        )}

        {/* ── Session drawer ── */}
        <SessionDrawer runId={state.runId} />

        {/* ── Tab bar ── */}
        <nav className="flex items-center gap-0.5 px-3 h-9 border-b border-(--color-border-subtle) shrink-0">
          {tabs.map((tab) => (
            <button
              key={tab.id}
              onClick={() => switchTab(tab.id)}
              className={`px-3 py-1 rounded-md text-xs font-medium transition-colors ${
                activeTab === tab.id
                  ? "bg-(--color-elevated) text-(--color-text-primary)"
                  : "text-(--color-text-tertiary) hover:text-(--color-text-secondary) hover:bg-(--color-elevated)/60"
              }`}
            >
              {tab.label}
            </button>
          ))}
        </nav>

        {/* ── Body ── */}
        <div className="flex flex-1 min-h-0">
          {showSidebar && (
            <aside className="w-52 border-r border-(--color-border-subtle) shrink-0 overflow-hidden">
              <FileTree />
            </aside>
          )}

          <main className="flex-1 min-w-0 overflow-hidden">
            {activeTab === "summary" && <SummaryTab />}
            {activeTab === "findings" && <SignalBoard />}
            {activeTab === "diff" && <DiffPanel onRevealFinding={revealFinding} />}
            {activeTab === "draft" && runId && (
              <DraftReviewTab runId={runId} onSubmitted={refreshSnapshot} />
            )}
            {activeTab === "diagnostics" && runId && (
              <DiagnosticsTab
                runId={runId}
                prId={state.prId}
                onMetadataRefreshed={refreshSnapshot}
                contextPackSummary={state.contextPackSummary ?? null}
                localChecksSummary={state.localChecksSummary ?? null}
                platformMetadata={state.platformMetadata ?? null}
                platformMetadataFetchedAt={state.platformMetadataFetchedAt ?? null}
                platformCapabilities={state.platformCapabilities ?? null}
                platformCapabilitiesFetchedAt={state.platformCapabilitiesFetchedAt ?? null}
                providerSelection={state.providerSelection ?? null}
                providerControl={state.providerControl ?? null}
              />
            )}
          </main>
        </div>

        <ApprovalModal />

        {!batchBarDismissed && pendingFixes.length > 0 && (
          <FixBatchBar
            fixCount={pendingFixes.length}
            onAcceptAll={handleAcceptAllFixes}
            onDismiss={() => setBatchBarDismissed(true)}
          />
        )}
      </div>
    </ReviewContext.Provider>
  );
}
