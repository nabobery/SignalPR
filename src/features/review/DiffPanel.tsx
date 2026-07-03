import { ErrorBoundary } from "react-error-boundary";
import { useReviewContext } from "../../lib/store";
import { PierreDiffPanel } from "./diff/PierreDiffPanel";
import { LegacyDiffPanel } from "./diff/LegacyDiffPanel";

export interface DiffPanelProps {
  onRevealFinding?: (findingId: string) => void;
}

export function DiffPanel({ onRevealFinding }: DiffPanelProps) {
  const { state } = useReviewContext();

  if (!state.diffText) {
    return (
      <div className="flex items-center justify-center h-full text-(--color-text-tertiary) text-sm">
        No diff available.
      </div>
    );
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex-1 min-h-0">
        <ErrorBoundary fallback={<LegacyDiffPanel state={state} />}>
          <PierreDiffPanel state={state} onRevealFinding={onRevealFinding} />
        </ErrorBoundary>
      </div>
    </div>
  );
}
