import type { DiffLineAnnotation } from "@pierre/diffs";
import type { Finding, FindingCluster } from "../../../lib/types";
import { normalizeFilePath } from "./normalizeFilePath";

export interface AnnotationPayload {
  findings: Finding[];
  highestSeverity: Finding["severity"];
}

const SEVERITY_RANK: Record<Finding["severity"], number> = {
  blocker: 0,
  critical: 1,
  warning: 2,
  info: 3,
  nitpick: 4,
};

/**
 * Maps SignalPR findings to Pierre DiffLineAnnotation entries for a given file.
 *
 * Filters out suppressed findings, non-representative cluster members, and
 * file-level (unanchored) findings. Groups multiple findings on the same line
 * into a single annotation payload. Results are sorted by lineNumber ascending.
 */
export function mapFindingsToLineAnnotations(
  findings: Finding[],
  filePath: string,
  clusters?: FindingCluster[],
  knownFiles?: ReadonlySet<string>,
): DiffLineAnnotation<AnnotationPayload>[] {
  const representativeIds = new Set(
    (clusters ?? [])
      .map((c) => c.representative_finding_id)
      .filter((id): id is string => id != null),
  );

  const clusterIds = new Set((clusters ?? []).map((c) => c.id));

  const eligible = findings.filter((f) => {
    const normalized = f.file_path ? normalizeFilePath(f.file_path, knownFiles) : null;
    if (normalized !== filePath) return false;
    if (f.status === "suppressed") return false;

    const lineNumber = f.diff_new_line ?? f.line_start;
    if (lineNumber == null) return false;

    if (f.cluster_id && clusterIds.has(f.cluster_id)) {
      return representativeIds.has(f.id);
    }

    return true;
  });

  const grouped = new Map<number, Finding[]>();
  for (const f of eligible) {
    const line = (f.diff_new_line ?? f.line_start)!;
    const existing = grouped.get(line);
    if (existing) {
      existing.push(f);
    } else {
      grouped.set(line, [f]);
    }
  }

  const annotations: DiffLineAnnotation<AnnotationPayload>[] = [];

  for (const [lineNumber, group] of grouped) {
    const sorted = [...group].sort((a, b) => SEVERITY_RANK[a.severity] - SEVERITY_RANK[b.severity]);

    annotations.push({
      side: "additions",
      lineNumber,
      metadata: {
        findings: sorted,
        highestSeverity: sorted[0].severity,
      },
    });
  }

  annotations.sort((a, b) => a.lineNumber - b.lineNumber);

  return annotations;
}
