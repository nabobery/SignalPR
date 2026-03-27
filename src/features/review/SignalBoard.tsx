import { FindingCard } from "./FindingCard";
import { useReviewContext } from "../../lib/store";

export function SignalBoard() {
  const { state, refreshSnapshot } = useReviewContext();
  const activeFindings = state.findings.filter((f) => f.status === "active");
  const selectedFile = state.selectedFile;

  const displayFindings = selectedFile
    ? activeFindings.filter((f) => f.file_path === selectedFile)
    : activeFindings;

  if (displayFindings.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-zinc-500 text-sm">
        {activeFindings.length === 0
          ? "No findings to display."
          : `No findings for ${selectedFile}`}
      </div>
    );
  }

  return (
    <div className="space-y-3 overflow-y-auto p-4">
      <div className="text-xs text-zinc-400 mb-2">
        {displayFindings.length} finding{displayFindings.length !== 1 ? "s" : ""}
        {selectedFile && <span> in {selectedFile}</span>}
      </div>
      {displayFindings.map((finding) => (
        <FindingCard key={finding.id} finding={finding} onUpdated={refreshSnapshot} />
      ))}
    </div>
  );
}
