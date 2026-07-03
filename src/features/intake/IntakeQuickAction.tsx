import { useState } from "react";
import { useNavigate } from "react-router";
import { Loader2 } from "lucide-react";
import { openFromUrl, confirmWorkspace, startReview, parseError } from "../../lib/ipc";
import type { PrIntakeResult } from "../../lib/types";

export function IntakeQuickAction() {
  const navigate = useNavigate();
  const [url, setUrl] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [prResult, setPrResult] = useState<PrIntakeResult | null>(null);
  const [localPath, setLocalPath] = useState("");
  const [confirming, setConfirming] = useState(false);
  const [starting, setStarting] = useState(false);

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

  const inputCls =
    "w-full rounded-md border border-(--color-border) bg-(--color-elevated) px-3 py-2 text-sm text-(--color-text-primary) placeholder:text-(--color-text-tertiary) focus:border-(--color-border-strong) focus:outline-none";

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && url.trim() && handleFetch()}
          placeholder="https://github.com/owner/repo/pull/123"
          className={inputCls}
        />
        <button
          onClick={handleFetch}
          disabled={!url.trim() || loading}
          className="inline-flex items-center gap-2 rounded-md bg-(--color-accent) px-4 py-2 text-sm font-medium text-white hover:bg-(--color-accent-hover) disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
        >
          {loading && <Loader2 className="w-3 h-3 animate-spin" />}
          Fetch
        </button>
      </div>

      {error && <p className="text-xs text-(--color-sev-blocker)">{error}</p>}

      {prResult && (
        <div className="rounded-lg border border-(--color-border) bg-(--color-surface) p-3 space-y-3">
          <div>
            <span className="text-[11px] font-mono text-(--color-text-tertiary)">
              {prResult.owner}/{prResult.repo} #{prResult.pr_number}
            </span>
            <h3 className="text-sm font-semibold text-(--color-text-primary) mt-0.5">
              {prResult.title}
            </h3>
            <div className="text-xs text-(--color-text-tertiary) flex gap-3 mt-1">
              {prResult.author && <span>by {prResult.author}</span>}
              <span>{prResult.changed_file_count} files changed</span>
            </div>
          </div>
          <div>
            <label className="text-xs text-(--color-text-tertiary) block mb-1">
              Local repository path
            </label>
            <input
              type="text"
              value={localPath}
              onChange={(e) => setLocalPath(e.target.value)}
              placeholder="/path/to/local/repo"
              className={inputCls}
            />
          </div>
          <button
            onClick={handleConfirmAndStart}
            disabled={!localPath.trim() || confirming || starting}
            className="w-full inline-flex items-center justify-center gap-2 rounded-md bg-(--color-accent) px-3 py-2 text-sm font-medium text-white hover:bg-(--color-accent-hover) disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            {(confirming || starting) && <Loader2 className="w-3 h-3 animate-spin" />}
            {confirming ? "Validating..." : starting ? "Starting..." : "Start Review"}
          </button>
        </div>
      )}
    </div>
  );
}
