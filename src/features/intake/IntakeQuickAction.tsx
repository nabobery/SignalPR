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

  return (
    <div className="space-y-3">
      <div className="flex gap-2">
        <input
          type="text"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && url.trim() && handleFetch()}
          placeholder="https://github.com/owner/repo/pull/123"
          className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-zinc-500 placeholder-zinc-600"
        />
        <button
          onClick={handleFetch}
          disabled={!url.trim() || loading}
          className="bg-zinc-100 text-zinc-900 px-4 py-2 rounded-lg text-sm font-medium hover:bg-zinc-200 disabled:opacity-40 disabled:cursor-not-allowed flex items-center gap-2"
        >
          {loading && <Loader2 className="w-3 h-3 animate-spin" />}
          Fetch
        </button>
      </div>

      {error && <p className="text-red-400 text-sm">{error}</p>}

      {prResult && (
        <div className="bg-zinc-900 border border-zinc-800 rounded-lg p-3 space-y-3">
          <div>
            <span className="text-zinc-400 text-xs">
              {prResult.owner}/{prResult.repo} #{prResult.pr_number}
            </span>
            <h3 className="text-sm font-semibold">{prResult.title}</h3>
            <div className="text-zinc-400 text-xs flex gap-3 mt-1">
              {prResult.author && <span>by {prResult.author}</span>}
              <span>{prResult.changed_file_count} files changed</span>
            </div>
          </div>
          <div>
            <label className="text-zinc-400 text-xs block mb-1">Local repository path</label>
            <input
              type="text"
              value={localPath}
              onChange={(e) => setLocalPath(e.target.value)}
              placeholder="/path/to/local/repo"
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-zinc-500 placeholder-zinc-600"
            />
          </div>
          <button
            onClick={handleConfirmAndStart}
            disabled={!localPath.trim() || confirming || starting}
            className="w-full bg-emerald-600 text-white px-3 py-1.5 rounded-lg text-sm font-medium hover:bg-emerald-500 disabled:opacity-40 disabled:cursor-not-allowed flex items-center justify-center gap-2"
          >
            {(confirming || starting) && <Loader2 className="w-3 h-3 animate-spin" />}
            {confirming ? "Validating..." : starting ? "Starting..." : "Start Review"}
          </button>
        </div>
      )}
    </div>
  );
}
