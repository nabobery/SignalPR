import { Loader2 } from "lucide-react";
import { useState } from "react";

interface FixBatchBarProps {
  fixCount: number;
  onAcceptAll: () => Promise<void>;
  onDismiss: () => void;
}

export function FixBatchBar({ fixCount, onAcceptAll, onDismiss }: FixBatchBarProps) {
  const [accepting, setAccepting] = useState(false);

  if (fixCount <= 0) return null;

  const handleAcceptAll = async () => {
    setAccepting(true);
    try {
      await onAcceptAll();
    } finally {
      setAccepting(false);
    }
  };

  return (
    <div className="fixed bottom-4 left-1/2 -translate-x-1/2 z-50 flex items-center gap-4 bg-zinc-900 border border-zinc-700 rounded-xl shadow-lg px-5 py-3">
      <span className="text-sm text-zinc-300">
        {fixCount} fix{fixCount !== 1 ? "es" : ""} available
      </span>
      <button
        onClick={handleAcceptAll}
        disabled={accepting}
        className="flex items-center gap-1 text-xs px-3 py-1.5 rounded-lg bg-emerald-600 text-white font-medium hover:bg-emerald-500 disabled:opacity-50"
      >
        {accepting && <Loader2 className="w-3 h-3 animate-spin" />}
        Accept All
      </button>
      <button
        onClick={onDismiss}
        className="text-xs px-3 py-1.5 rounded-lg bg-zinc-700 text-zinc-300 font-medium hover:bg-zinc-600"
      >
        Dismiss
      </button>
    </div>
  );
}
