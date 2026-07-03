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
    const unlistenChannel = listen<ChannelEvent>("channel_review_requested", (event) => {
      const { source, pr_url, requester } = event.payload;
      setUrl(pr_url);
      setChannelToast(`Review requested via ${source}${requester ? ` from ${requester}` : ""}`);
      setTimeout(() => setChannelToast(null), 5000);
    });
    const unlistenGitHub = listen<ChannelEvent>("github_review_requested", (event) => {
      const { source, pr_url, requester } = event.payload;
      setUrl(pr_url);
      setChannelToast(
        `GitHub review requested${requester ? ` from ${requester}` : ""}${source ? ` (${source})` : ""}`,
      );
      setTimeout(() => setChannelToast(null), 5000);
    });
    return () => {
      unlistenChannel.then((fn) => fn());
      unlistenGitHub.then((fn) => fn());
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
    <main className="min-h-screen bg-(--color-base) text-(--color-text-primary) flex flex-col items-center justify-center p-8 relative">
      <button
        onClick={() => navigate("/settings")}
        className="absolute top-4 right-4 text-(--color-text-tertiary) hover:text-(--color-text-primary) transition-colors"
        title="Settings"
      >
        <Settings className="w-5 h-5" />
      </button>
      <h1 className="text-3xl font-bold mb-2">SignalPR</h1>
      <p className="text-(--color-text-secondary) mb-6 text-center max-w-md">
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
          onKeyDown={(e) => e.key === "Enter" && url.trim() && handleFetch()}
          placeholder="https://github.com/owner/repo/pull/123"
          className="flex-1 bg-(--color-surface) border border-(--color-border) rounded-lg px-4 py-2 text-sm focus:outline-none focus:border-(--color-border-strong) placeholder:text-(--color-text-tertiary)"
        />
        <button
          onClick={handleFetch}
          disabled={!url.trim() || loading}
          className="bg-(--color-accent) text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-(--color-elevated) disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-2"
        >
          {loading && <Loader2 className="w-3 h-3 animate-spin" />}
          Fetch
        </button>
      </div>

      {error && <p className="text-(--color-sev-blocker) text-sm mt-3 max-w-lg">{error}</p>}

      {prResult && (
        <div className="w-full max-w-lg mt-6 bg-(--color-surface) border border-(--color-border-subtle) rounded-lg p-4 space-y-3">
          <div>
            <span className="text-(--color-text-secondary) text-xs">
              {prResult.owner}/{prResult.repo} #{prResult.pr_number}
            </span>
            <h2 className="text-lg font-semibold">{prResult.title}</h2>
            <div className="text-(--color-text-secondary) text-sm flex gap-3 mt-1">
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
            <label className="text-(--color-text-secondary) text-xs block mb-1">
              Local repository path
            </label>
            <input
              type="text"
              value={localPath}
              onChange={(e) => setLocalPath(e.target.value)}
              placeholder="/path/to/local/repo"
              className="w-full bg-(--color-elevated) border border-(--color-border) rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-(--color-border-strong) placeholder:text-(--color-text-tertiary)"
            />
          </div>

          <button
            onClick={handleConfirmAndStart}
            disabled={!localPath.trim() || confirming || starting}
            className="w-full bg-(--color-accent) text-white px-4 py-2 rounded-lg text-sm font-medium hover:bg-(--color-accent-hover) disabled:opacity-40 disabled:cursor-not-allowed flex items-center justify-center gap-2"
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
        <p className="text-(--color-text-tertiary) text-xs mt-4">
          No submit path detected yet (gh CLI or a platform token) — you can still fetch a PR; fix
          the environment above before submitting.
        </p>
      )}

      {channelToast && (
        <div className="fixed bottom-6 right-6 bg-(--color-elevated) border border-(--color-border) rounded-lg px-4 py-3 text-sm text-(--color-text-primary) shadow-lg animate-in fade-in slide-in-from-bottom-2">
          {channelToast}
        </div>
      )}
    </main>
  );
}
