import { useMemo, useCallback } from "react";
import { FileDiff } from "@pierre/diffs/react";
import type { DiffLineAnnotation, FileDiffMetadata } from "@pierre/diffs";
import { parsePierrePatch } from "./parsePierrePatch";
import {
  mapFindingsToLineAnnotations,
  type AnnotationPayload,
} from "./mapFindingsToLineAnnotations";
import { normalizeFilePath } from "./normalizeFilePath";
import { FindingAnnotation } from "./FindingAnnotation";
import { computeDiffStats, shouldCollapseByDefault } from "./perfHeuristics";
import type { ReviewState } from "../../../lib/store";
import type { Finding } from "../../../lib/types";

const SEVERITY_RANK: Record<Finding["severity"], number> = {
  blocker: 0,
  critical: 1,
  warning: 2,
  info: 3,
  nitpick: 4,
};

interface PierreDiffPanelProps {
  state: ReviewState;
  onRevealFinding?: (findingId: string) => void;
}

export function PierreDiffPanel({ state, onRevealFinding }: PierreDiffPanelProps) {
  const fileDiffs = useMemo(() => parsePierrePatch(state.diffText ?? ""), [state.diffText]);

  const knownFiles = useMemo(() => new Set(state.changedFiles), [state.changedFiles]);

  /** Precomputed map: normalizedFilePath → Finding[] (computed once per render) */
  const findingsByFile = useMemo(() => {
    const map = new Map<string, Finding[]>();
    for (const f of state.findings) {
      if (!f.file_path) continue;
      const key = normalizeFilePath(f.file_path, knownFiles);
      const bucket = map.get(key);
      if (bucket) {
        bucket.push(f);
      } else {
        map.set(key, [f]);
      }
    }
    return map;
  }, [state.findings, knownFiles]);

  const fileAnnotationsMap = useMemo(() => {
    const map = new Map<string, DiffLineAnnotation<AnnotationPayload>[]>();
    for (const fd of fileDiffs) {
      const fileFindingsForName = findingsByFile.get(fd.name) ?? [];
      const fileFindingsForPrev = fd.prevName ? (findingsByFile.get(fd.prevName) ?? []) : [];
      const combined =
        fileFindingsForPrev.length > 0
          ? [...fileFindingsForName, ...fileFindingsForPrev]
          : fileFindingsForName;

      if (combined.length === 0) continue;

      const annotations = mapFindingsToLineAnnotations(
        combined,
        fd.name,
        state.clusters,
        knownFiles,
        state.platformMetadata,
      );
      if (fd.prevName) {
        const prevAnnotations = mapFindingsToLineAnnotations(
          combined,
          fd.prevName,
          state.clusters,
          knownFiles,
          state.platformMetadata,
        );
        annotations.push(...prevAnnotations);
        annotations.sort((a, b) => a.lineNumber - b.lineNumber);
      }
      if (annotations.length > 0) {
        map.set(fd.name, annotations);
      }
    }
    return map;
  }, [fileDiffs, findingsByFile, state.clusters, knownFiles, state.platformMetadata]);

  /** Precomputed map: filePath → file-level (unanchored) findings */
  const fileLevelFindingsMap = useMemo(() => {
    const map = new Map<string, Finding[]>();
    for (const fd of fileDiffs) {
      const candidates = [
        ...(findingsByFile.get(fd.name) ?? []),
        ...(fd.prevName ? (findingsByFile.get(fd.prevName) ?? []) : []),
      ];
      const fileLevelFindings = candidates.filter(
        (f) => f.status === "active" && f.line_start == null && f.diff_new_line == null,
      );
      if (fileLevelFindings.length > 0) {
        map.set(fd.name, fileLevelFindings);
      }
    }
    return map;
  }, [fileDiffs, findingsByFile]);

  const renderAnnotation = useCallback(
    (annotation: DiffLineAnnotation<AnnotationPayload>) => {
      if (!annotation.metadata) return null;
      return <FindingAnnotation payload={annotation.metadata} onReveal={onRevealFinding} />;
    },
    [onRevealFinding],
  );

  const renderHeaderMetadata = useCallback(
    (fileDiff: FileDiffMetadata) => {
      const fileFindings = fileLevelFindingsMap.get(fileDiff.name);
      if (!fileFindings || fileFindings.length === 0) return null;
      const highest = fileFindings.reduce((a, b) =>
        (SEVERITY_RANK[a.severity] ?? 4) <= (SEVERITY_RANK[b.severity] ?? 4) ? a : b,
      );
      return (
        <span className="text-xs px-1.5 py-0.5 rounded bg-[--color-elevated] text-[--color-text-secondary]">
          {fileFindings.length} file-level &middot; {highest.severity}
        </span>
      );
    },
    [fileLevelFindingsMap],
  );

  const diffStats = useMemo(() => computeDiffStats(fileDiffs), [fileDiffs]);
  const collapseByDefault = shouldCollapseByDefault(diffStats);

  const isFileExpanded = useCallback(
    (fd: FileDiffMetadata) => {
      if (state.selectedFile) {
        return fd.name === state.selectedFile || fd.prevName === state.selectedFile;
      }
      return !collapseByDefault;
    },
    [state.selectedFile, collapseByDefault],
  );

  if (fileDiffs.length === 0) {
    return (
      <div className="flex items-center justify-center h-full text-[--color-text-tertiary] text-sm">
        No diff available.
      </div>
    );
  }

  return (
    <div className="overflow-auto h-full">
      {fileDiffs.map((fd) => (
        <FileDiff
          key={fd.name}
          fileDiff={fd}
          options={{
            diffStyle: "unified",
            collapsed: !isFileExpanded(fd),
          }}
          lineAnnotations={fileAnnotationsMap.get(fd.name)}
          renderAnnotation={renderAnnotation}
          renderHeaderMetadata={renderHeaderMetadata}
        />
      ))}
    </div>
  );
}
