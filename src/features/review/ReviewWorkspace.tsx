import { useCallback, useEffect, useState } from "react";
import { useParams, useNavigate } from "react-router";
import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, Loader2, Send, FileCode, List } from "lucide-react";
import { getReviewSnapshot } from "../../lib/ipc";
import { ReviewContext, type ReviewState } from "../../lib/store";
import { FileTree } from "./FileTree";
import { SignalBoard } from "./SignalBoard";
import { DiffPanel } from "./DiffPanel";
import { SubmitDialog } from "../submission/SubmitDialog";

type Panel = "signals" | "diff";

export function ReviewWorkspace() {
  const { runId } = useParams<{ runId: string }>();
  const navigate = useNavigate();
  const [state, setState] = useState<ReviewState | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activePanel, setActivePanel] = useState<Panel>("signals");
  const [showSubmit, setShowSubmit] = useState(false);

  const refreshSnapshot = useCallback(async () => {
    if (!runId) return;
    try {
      const snap = await getReviewSnapshot(runId);
      setState((prev) => ({
        runId: snap.run_id,
        status: snap.status,
        prTitle: snap.pr_title,
        prNumber: snap.pr_number,
        prUrl: snap.pr_url,
        diffText: snap.diff_text,
        changedFiles: snap.changed_files,
        findings: snap.findings,
        errorMessage: snap.error_message,
        selectedFile:
          prev?.selectedFile && snap.changed_files.includes(prev.selectedFile)
            ? prev.selectedFile
            : null,
      }));
    } catch (err) {
      setError(String(err));
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => {
    refreshSnapshot();
  }, [refreshSnapshot]);

  useEffect(() => {
    const unlisten = listen("review_progress", () => {
      refreshSnapshot();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refreshSnapshot]);

  const setSelectedFile = (file: string | null) => {
    setState((prev) => (prev ? { ...prev, selectedFile: file } : prev));
  };

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
          Back to intake
        </button>
      </div>
    );
  }

  const isReady = state.status === "ready" || state.status === "submitted";
  const isRunning =
    state.status === "created" || state.status === "running_agents" || state.status === "cleaning";
  const activeCount = state.findings.filter((f) => f.status === "active").length;

  return (
    <ReviewContext.Provider value={{ state, setSelectedFile, refreshSnapshot }}>
      <div className="h-screen bg-zinc-950 text-zinc-100 flex flex-col">
        {/* Header */}
        <header className="flex items-center gap-3 px-4 py-3 border-b border-zinc-800 shrink-0">
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

            {/* Panel toggle */}
            <div className="flex border border-zinc-700 rounded-lg overflow-hidden ml-2">
              <button
                onClick={() => setActivePanel("signals")}
                className={`px-2 py-1 text-xs ${activePanel === "signals" ? "bg-zinc-700 text-zinc-100" : "text-zinc-400 hover:text-zinc-200"}`}
              >
                <List className="w-3 h-3" />
              </button>
              <button
                onClick={() => setActivePanel("diff")}
                className={`px-2 py-1 text-xs ${activePanel === "diff" ? "bg-zinc-700 text-zinc-100" : "text-zinc-400 hover:text-zinc-200"}`}
              >
                <FileCode className="w-3 h-3" />
              </button>
            </div>

            {isReady && state.status !== "submitted" && (
              <button
                onClick={() => setShowSubmit(true)}
                className="flex items-center gap-1 bg-emerald-600 text-white px-3 py-1.5 rounded-lg text-xs font-medium hover:bg-emerald-500"
              >
                <Send className="w-3 h-3" />
                Submit ({activeCount})
              </button>
            )}
          </div>
        </header>

        {/* Body */}
        <div className="flex flex-1 min-h-0">
          {/* File tree sidebar */}
          <aside className="w-56 border-r border-zinc-800 shrink-0 overflow-hidden">
            <FileTree />
          </aside>

          {/* Main panel */}
          <main className="flex-1 min-w-0 overflow-hidden">
            {activePanel === "signals" ? <SignalBoard /> : <DiffPanel />}
          </main>
        </div>

        {showSubmit && runId && (
          <SubmitDialog
            runId={runId}
            onClose={() => setShowSubmit(false)}
            onSubmitted={() => {
              setShowSubmit(false);
              refreshSnapshot();
            }}
          />
        )}
      </div>
    </ReviewContext.Provider>
  );
}
