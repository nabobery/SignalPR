import type { AnnotationPayload } from "./mapFindingsToLineAnnotations";

const SEVERITY_COLORS: Record<string, string> = {
  blocker: "bg-red-500 text-white",
  critical: "bg-orange-500 text-white",
  warning: "bg-yellow-500 text-black",
  info: "bg-blue-500 text-white",
  nitpick: "bg-zinc-500 text-white",
};

interface FindingAnnotationProps {
  payload: AnnotationPayload;
  onReveal?: (findingId: string) => void;
}

export function FindingAnnotation({ payload, onReveal }: FindingAnnotationProps) {
  const { findings, highestSeverity, provenanceLabel, supportLabels, warningLabels } = payload;
  const colorClass = SEVERITY_COLORS[highestSeverity] ?? SEVERITY_COLORS.info;
  const label = findings.length === 1 ? findings[0].title : `${findings.length} findings`;

  const handleClick = () => {
    if (onReveal && findings.length > 0) {
      onReveal(findings[0].id);
    }
  };

  return (
    <button
      type="button"
      onClick={handleClick}
      className={`inline-flex items-center gap-1.5 px-2 py-0.5 rounded text-xs font-medium cursor-pointer hover:opacity-80 transition-opacity ${colorClass}`}
      title={[
        findings.map((f) => `[${f.severity}] ${f.title}`).join("\n"),
        provenanceLabel ? `Provenance: ${provenanceLabel}` : "",
        supportLabels.length > 0 ? `Signals: ${supportLabels.join(", ")}` : "",
        warningLabels.length > 0 ? `Warnings: ${warningLabels.join(", ")}` : "",
      ]
        .filter(Boolean)
        .join("\n")}
    >
      <span className="capitalize">{highestSeverity}</span>
      <span className="opacity-80">{label}</span>
      {supportLabels.length > 0 && <span className="opacity-70">+</span>}
    </button>
  );
}
