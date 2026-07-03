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
    <div className="fixed bottom-4 left-1/2 -translate-x-1/2 z-50 flex items-center gap-4 bg-(--color-overlay) border border-(--color-border) rounded-xl shadow-xl px-5 py-3">
      <span className="text-sm text-(--color-text-secondary)">
        {fixCount} fix{fixCount !== 1 ? "es" : ""} available
      </span>
      <button
        onClick={handleAcceptAll}
        disabled={accepting}
        className="inline-flex items-center gap-1.5 text-xs px-3 py-1.5 rounded-md bg-(--color-accent) text-white font-medium hover:bg-(--color-accent-hover) disabled:opacity-50 transition-colors"
      >
        {accepting && <Loader2 className="w-3 h-3 animate-spin" />}
        Accept All
      </button>
      <button
        onClick={onDismiss}
        className="text-xs px-3 py-1.5 rounded-md bg-(--color-elevated) text-(--color-text-secondary) font-medium hover:text-(--color-text-primary) transition-colors"
      >
        Dismiss
      </button>
    </div>
  );
}
