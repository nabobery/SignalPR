import { useCallback, useEffect, useMemo, useRef, useState, startTransition } from "react";
import { useParams, useNavigate } from "react-router";
import { listen } from "@tauri-apps/api/event";
import {
  ArrowLeft,
  Loader2,
  LayoutDashboard,
  List,
  FileCode,
  PenLine,
  Activity,
} from "lucide-react";
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

function DraftReviewTabLazy({ runId, onSubmitted }: { runId: string; onSubmitted: () => void }) {
  return <DraftReviewTab runId={runId} onSubmitted={onSubmitted} />;
}

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
              if (d === "accept" || d === "skip") {
                seeded[id] = d;
              }
            }
            return seeded;
          }
          const nextFindingIds = new Set(snap.findings.map((f) => f.id));
          const prevDecisions = prev?.sessionDecisions ?? {};
          return Object.fromEntries(
            Object.entries(prevDecisions).filter(([id]) => nextFindingIds.has(id)),
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
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    const unlisten = listen<{ run_id?: string }>("review_progress", (event) => {
      // Ignore events for other runs
      if (event.payload?.run_id && event.payload.run_id !== runId) return;
      if (debounceTimer) clearTimeout(debounceTimer);
      debounceTimer = setTimeout(() => refreshSnapshot(), 300);
    });
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer);
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
      return {
        ...prev,
        sessionDecisions: {
          ...prev.sessionDecisions,
          [findingId]: decision,
        },
      };
    });
  };

  const knownFilesSet = useMemo(() => new Set(state?.changedFiles ?? []), [state?.changedFiles]);

  const focusTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (focusTimerRef.current) clearTimeout(focusTimerRef.current);
    };
  }, []);

  const revealFinding = useCallback(
    (findingId: string) => {
      const finding = state?.findings.find((f) => f.id === findingId);
      if (!finding) return;

      if (finding.file_path) {
        setSelectedFile(normalizeFilePath(finding.file_path, knownFilesSet));
      }
      setActiveTab("findings");
      setState((prev) => (prev ? { ...prev, focusedFindingId: findingId } : prev));

      if (focusTimerRef.current) clearTimeout(focusTimerRef.current);
      focusTimerRef.current = setTimeout(() => {
        setState((prev) => (prev ? { ...prev, focusedFindingId: null } : prev));
      }, 2000);
    },
    [state?.findings, knownFilesSet],
  );

  if (loading) {
    return (
      <div className="min-h-screen bg-zinc-950 text-zinc-100 flex items-center justify-center">
        <Loader2 className="w-6 h-6 animate-spin text-zinc-400" />
      </div>
    );
  }

  if (error || !state) {
    return (
      <div className="min-h-screen bg-zinc-950 text-zinc-100 flex flex-col items-center justify-center gap-4">
        <p className="text-red-400">{error ?? "Failed to load review"}</p>
        <button onClick={() => navigate("/")} className="text-zinc-400 hover:text-zinc-200 text-sm">
          Back to inbox
        </button>
      </div>
    );
  }

  const isReady = state.status === "ready" || state.status === "submitted";
  const isRunning =
    state.status === "created" || state.status === "running_agents" || state.status === "cleaning";
  const activeCount = state.findings.filter((f) => f.status === "active").length;
  const pendingFixes = state.findings.filter(
    (f) => f.fix_search && !["applied", "accepted", "rejected"].includes(f.fix_status ?? ""),
  );

  const handleAcceptAllFixes = async () => {
    for (const f of pendingFixes) {
      await acceptFix(f.id);
    }
    await refreshSnapshot();
  };

  const tabs: { id: WorkspaceTab; label: string; icon: typeof LayoutDashboard }[] = [
    { id: "summary", label: "Summary", icon: LayoutDashboard },
    { id: "findings", label: `Findings (${activeCount})`, icon: List },
    { id: "diff", label: "Diff", icon: FileCode },
    { id: "draft", label: "Draft Review", icon: PenLine },
    { id: "diagnostics", label: "Diagnostics", icon: Activity },
  ];

  const showSidebar = activeTab === "findings" || activeTab === "diff";

  return (
    <ReviewContext.Provider
      value={{ state, setSelectedFile, setSessionDecision, refreshSnapshot, revealFinding }}
    >
      <div className="h-screen bg-zinc-950 text-zinc-100 flex flex-col">
        {/* Header */}
        <header className="flex items-center gap-3 px-4 py-2.5 border-b border-zinc-800 shrink-0">
          <button onClick={() => navigate("/")} className="text-zinc-400 hover:text-zinc-200">
            <ArrowLeft className="w-4 h-4" />
          </button>
          <div className="flex-1 min-w-0">
            <span className="text-zinc-500 text-xs">PR #{state.prNumber}</span>
            <h1 className="text-sm font-semibold truncate">{state.prTitle}</h1>
          </div>

          <div className="flex items-center gap-2">
            {isRunning && (
              <span className="flex items-center gap-1 text-xs text-yellow-400">
                <Loader2 className="w-3 h-3 animate-spin" />
                {state.status === "running_agents" ? "Analyzing..." : "Cleaning..."}
              </span>
            )}
            {state.status === "failed" && (
              <span className="text-xs text-red-400">
                Failed: {state.errorMessage ?? "Unknown error"}
              </span>
            )}
            {state.status === "submitted" && (
              <span className="text-xs text-emerald-400">Submitted</span>
            )}
          </div>
        </header>

        {/* Secondary tab navigation */}
        <nav className="flex items-center gap-0.5 px-4 py-1.5 border-b border-zinc-800 shrink-0">
          {tabs.map((tab) => {
            const Icon = tab.icon;
            return (
              <button
                key={tab.id}
                onClick={() => switchTab(tab.id)}
                className={`flex items-center gap-1.5 px-2.5 py-1.5 rounded-md text-xs font-medium transition-colors ${
                  activeTab === tab.id
                    ? "bg-zinc-800 text-zinc-100"
                    : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                {tab.label}
              </button>
            );
          })}
        </nav>

        {/* Lane progress */}
        {isRunning && state.laneStatuses.length > 0 && (
          <div className="px-4 py-2 border-b border-zinc-800 shrink-0">
            <LaneProgress lanes={state.laneStatuses} />
            <SessionDrawer runId={state.runId} />
          </div>
        )}

        {!isRunning && <SessionDrawer runId={state.runId} />}

        {/* Partial success banner */}
        {isReady &&
          state.laneStatuses.some((l) => l.status === "failed" || l.status === "timed_out") && (
            <div className="px-4 py-2 border-b border-zinc-800 bg-yellow-900/20 shrink-0">
              <p className="text-xs text-yellow-400">
                {state.laneStatuses.filter((l) => l.status === "completed").length} of{" "}
                {state.laneStatuses.length} analysis lanes completed. Some lanes failed — findings
                from successful lanes are still submittable.
              </p>
            </div>
          )}

        {/* Degraded-mode banner */}
        {isReady && state.laneStatuses.every((l) => l.provider_name === "mock") && (
          <div className="px-4 py-2 border-b border-zinc-800 bg-amber-900/20 shrink-0">
            <p className="text-xs text-amber-400">
              Running in demo mode — no AI providers available. Install Codex CLI or set
              ANTHROPIC_API_KEY for real analysis.
            </p>
          </div>
        )}

        {/* Body */}
        <div className="flex flex-1 min-h-0">
          {/* File tree sidebar (only for Findings/Diff) */}
          {showSidebar && (
            <aside className="w-56 border-r border-zinc-800 shrink-0 overflow-hidden">
              <FileTree />
            </aside>
          )}

          {/* Main panel */}
          <main className="flex-1 min-w-0 overflow-hidden">
            {activeTab === "summary" && <SummaryTab />}
            {activeTab === "findings" && <SignalBoard />}
            {activeTab === "diff" && <DiffPanel onRevealFinding={revealFinding} />}
            {activeTab === "draft" && runId && (
              <DraftReviewTabLazy runId={runId} onSubmitted={refreshSnapshot} />
            )}
            {activeTab === "diagnostics" && runId && (
              <DiagnosticsTab
                runId={runId}
                prId={state.prId}
                onMetadataRefreshed={refreshSnapshot}
                contextPackSummary={state?.contextPackSummary ?? null}
                localChecksSummary={state?.localChecksSummary ?? null}
                platformMetadata={state?.platformMetadata ?? null}
                platformMetadataFetchedAt={state?.platformMetadataFetchedAt ?? null}
                platformCapabilities={state?.platformCapabilities ?? null}
                platformCapabilitiesFetchedAt={state?.platformCapabilitiesFetchedAt ?? null}
                providerSelection={state?.providerSelection ?? null}
                providerControl={state?.providerControl ?? null}
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
