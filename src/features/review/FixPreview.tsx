import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import { previewFix, acceptFix, rejectFix, parseError } from "../../lib/ipc";

interface FixPreviewProps {
  findingId: string;
  fixSearch: string;
  fixReplace: string;
  fixExplanation: string | null;
  onAccept: () => void;
  onReject: () => void;
}

export function FixPreview({
  findingId,
  fixSearch: _fixSearch,
  fixReplace: _fixReplace,
  fixExplanation,
  onAccept,
  onReject,
}: FixPreviewProps) {
  // fixSearch and fixReplace are available for future inline display;
  // the rendered diff comes from the previewFix IPC call.
  void _fixSearch;
  void _fixReplace;
  const [diff, setDiff] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [acting, setActing] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    previewFix(findingId)
      .then((result) => {
        if (!cancelled) setDiff(result);
      })
      .catch((err) => {
        if (!cancelled) setError(parseError(err).message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [findingId]);

  const handleAccept = async () => {
    setActing(true);
    try {
      await acceptFix(findingId);
      onAccept();
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setActing(false);
    }
  };

  const handleReject = async () => {
    setActing(true);
    try {
      await rejectFix(findingId);
      onReject();
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setActing(false);
    }
  };

  const renderDiffLine = (line: string, idx: number) => {
    let className = "whitespace-pre text-zinc-400";
    if (line.startsWith("-")) {
      className = "whitespace-pre bg-red-900/30 text-red-300";
    } else if (line.startsWith("+")) {
      className = "whitespace-pre bg-emerald-900/30 text-emerald-300";
    } else if (line.startsWith("@@")) {
      className = "whitespace-pre text-blue-400";
    }
    return (
      <div key={idx} className={className}>
        {line}
      </div>
    );
  };

  return (
    <div className="mt-2 space-y-2">
      {fixExplanation && <p className="text-xs text-zinc-400 italic">{fixExplanation}</p>}

      {loading && (
        <div className="flex items-center gap-2 text-xs text-zinc-500">
          <Loader2 className="w-3 h-3 animate-spin" />
          Loading diff preview...
        </div>
      )}

      {error && <p className="text-xs text-red-400">{error}</p>}

      {diff && !loading && (
        <div className="font-mono text-sm bg-zinc-950 rounded-lg border border-zinc-800 p-3 overflow-x-auto">
          {diff.split("\n").map(renderDiffLine)}
        </div>
      )}

      {!loading && !error && (
        <div className="flex gap-2">
          <button
            onClick={handleAccept}
            disabled={acting}
            className="flex items-center gap-1 text-xs px-3 py-1 rounded bg-emerald-600 text-white hover:bg-emerald-500 disabled:opacity-50"
          >
            {acting ? <Loader2 className="w-3 h-3 animate-spin" /> : null}
            Accept Fix
          </button>
          <button
            onClick={handleReject}
            disabled={acting}
            className="flex items-center gap-1 text-xs px-3 py-1 rounded bg-red-600 text-white hover:bg-red-500 disabled:opacity-50"
          >
            Reject Fix
          </button>
        </div>
      )}
    </div>
  );
}
