import { useState } from "react";
import { Loader2, Send, X } from "lucide-react";
import { submitReview, parseError } from "../../lib/ipc";
import type { ReviewAction } from "../../lib/types";

export function SubmitDialog({
  runId,
  onClose,
  onSubmitted,
}: {
  runId: string;
  onClose: () => void;
  onSubmitted: () => void;
}) {
  const [action, setAction] = useState<ReviewAction>("comment");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async () => {
    setSubmitting(true);
    setError(null);
    try {
      await submitReview(runId, action);
      onSubmitted();
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setSubmitting(false);
    }
  };

  const actions: { value: ReviewAction; label: string; description: string }[] = [
    { value: "comment", label: "Comment", description: "Leave feedback without explicit approval" },
    { value: "approve", label: "Approve", description: "Approve the pull request" },
    {
      value: "request-changes",
      label: "Request Changes",
      description: "Block merging until addressed",
    },
  ];

  return (
    <div className="fixed inset-0 bg-black/60 flex items-center justify-center z-50">
      <div className="bg-zinc-900 border border-zinc-700 rounded-xl p-6 w-full max-w-md">
        <div className="flex items-center justify-between mb-4">
          <h3 className="text-lg font-semibold text-zinc-100">Submit Review</h3>
          <button onClick={onClose} className="text-zinc-400 hover:text-zinc-200">
            <X className="w-5 h-5" />
          </button>
        </div>

        <div className="space-y-2 mb-4">
          {actions.map((a) => (
            <label
              key={a.value}
              className={`flex items-start gap-3 p-3 rounded-lg border cursor-pointer ${
                action === a.value
                  ? "border-emerald-500 bg-emerald-900/10"
                  : "border-zinc-700 hover:border-zinc-600"
              }`}
            >
              <input
                type="radio"
                name="action"
                value={a.value}
                checked={action === a.value}
                onChange={() => setAction(a.value)}
                className="mt-1"
              />
              <div>
                <div className="text-sm font-medium text-zinc-100">{a.label}</div>
                <div className="text-xs text-zinc-400">{a.description}</div>
              </div>
            </label>
          ))}
        </div>

        {error && <p className="text-red-400 text-sm mb-3">{error}</p>}

        <div className="flex gap-2">
          <button
            onClick={onClose}
            className="flex-1 px-4 py-2 text-sm border border-zinc-700 rounded-lg text-zinc-300 hover:bg-zinc-800"
          >
            Cancel
          </button>
          <button
            onClick={handleSubmit}
            disabled={submitting}
            className="flex-1 px-4 py-2 text-sm bg-emerald-600 text-white rounded-lg font-medium hover:bg-emerald-500 disabled:opacity-50 flex items-center justify-center gap-2"
          >
            {submitting ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <Send className="w-3 h-3" />
            )}
            Submit
          </button>
        </div>
      </div>
    </div>
  );
}
