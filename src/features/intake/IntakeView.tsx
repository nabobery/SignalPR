import { useState, useCallback, useEffect } from "react";
import { useNavigate } from "react-router";
import { Loader2, Settings } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { EnvironmentCheck } from "../onboarding/EnvironmentCheck";
import { openFromUrl, confirmWorkspace, startReview, parseError } from "../../lib/ipc";
import type { PrIntakeResult, ChannelEvent } from "../../lib/types";

export function IntakeView() {
  const navigate = useNavigate();
  const [url, setUrl] = useState("");
  const [envReady, setEnvReady] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [prResult, setPrResult] = useState<PrIntakeResult | null>(null);
  const [localPath, setLocalPath] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [starting, setStarting] = useState(false);
  const [channelToast, setChannelToast] = useState<string | null>(null);

  useEffect(() => {
    const unlisten = listen<ChannelEvent>("channel_review_requested", (event) => {
      const { source, pr_url, requester } = event.payload;
      setUrl(pr_url);
      setChannelToast(`Review requested via ${source}${requester ? ` from ${requester}` : ""}`);
      setTimeout(() => setChannelToast(null), 5000);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleFetch = async () => {
    setError(null);
    setLoading(true);
    try {
      const result = await openFromUrl(url);
      setPrResult(result);
      if (result.workspace_suggestion) {
        setLocalPath(result.workspace_suggestion);
      }
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  };

  const handleConfirmAndStart = async () => {
    if (!prResult || !localPath.trim()) return;

    setError(null);
    setConfirming(true);
    try {
      await confirmWorkspace(prResult.pr_id, localPath);
      setConfirming(false);
      setStarting(true);
      const runId = await startReview(prResult.pr_id);
      navigate(`/review/${runId}`);
    } catch (err) {
      setError(parseError(err).message);
      setConfirming(false);
      setStarting(false);
    }
  };

  const onEnvReady = useCallback((ready: boolean) => setEnvReady(ready), []);

  return (
    <main className="min-h-screen bg-zinc-950 text-zinc-100 flex flex-col items-center justify-center p-8 relative">
      <button
        onClick={() => navigate("/settings")}
        className="absolute top-4 right-4 text-zinc-500 hover:text-zinc-200 transition-colors"
        title="Settings"
      >
        <Settings className="w-5 h-5" />
      </button>
      <h1 className="text-3xl font-bold mb-2">SignalPR</h1>
      <p className="text-zinc-400 mb-6 text-center max-w-md">
        Paste a GitHub PR URL to start a reviewer-first, AI-assisted code review.
      </p>

      <div className="w-full max-w-lg mb-6">
        <EnvironmentCheck onReady={onEnvReady} />
      </div>

      <div className="w-full max-w-lg flex gap-2">
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && envReady && url.trim() && handleFetch()}
          placeholder="https://github.com/owner/repo/pull/123"
          className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-4 py-2 text-sm focus:outline-none focus:border-zinc-500 placeholder-zinc-600"
        />
        <button
          onClick={handleFetch}
          disabled={!envReady || !url.trim() || loading}
          className="bg-zinc-100 text-zinc-900 px-4 py-2 rounded-lg text-sm font-medium hover:bg-zinc-200 disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-2"
        >
          {loading && <Loader2 className="w-3 h-3 animate-spin" />}
          Fetch
        </button>
      </div>

      {error && <p className="text-red-400 text-sm mt-3 max-w-lg">{error}</p>}

      {prResult && (
        <div className="w-full max-w-lg mt-6 bg-zinc-900 border border-zinc-800 rounded-lg p-4 space-y-3">
          <div>
            <span className="text-zinc-400 text-xs">
              {prResult.owner}/{prResult.repo} #{prResult.pr_number}
            </span>
            <h2 className="text-lg font-semibold">{prResult.title}</h2>
            <div className="text-zinc-400 text-sm flex gap-3 mt-1">
              {prResult.author && <span>by {prResult.author}</span>}
              <span>{prResult.changed_file_count} files changed</span>
              {prResult.base_branch && (
                <span>
                  {prResult.head_branch} → {prResult.base_branch}
                </span>
              )}
            </div>
          </div>

          <div>
            <label className="text-zinc-400 text-xs block mb-1">Local repository path</label>
            <input
              type="text"
              value={localPath}
              onChange={(e) => setLocalPath(e.target.value)}
              placeholder="/path/to/local/repo"
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-zinc-500 placeholder-zinc-600"
            />
          </div>

          <button
            onClick={handleConfirmAndStart}
            disabled={!localPath.trim() || confirming || starting}
            className="w-full bg-emerald-600 text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-emerald-500 disabled:opacity-40 disabled:cursor-not-allowed flex items-center justify-center gap-2"
          >
            {(confirming || starting) && <Loader2 className="w-3 h-3 animate-spin" />}
            {confirming
              ? "Validating workspace..."
              : starting
                ? "Starting review..."
                : "Start Review"}
          </button>
        </div>
      )}

      {!envReady && (
        <p className="text-zinc-600 text-xs mt-4">
          gh CLI must be authenticated before you can start a review.
        </p>
      )}

      {channelToast && (
        <div className="fixed bottom-6 right-6 bg-zinc-800 border border-zinc-700 rounded-lg px-4 py-3 text-sm text-zinc-100 shadow-lg animate-in fade-in slide-in-from-bottom-2">
          {channelToast}
        </div>
      )}
    </main>
  );
}
