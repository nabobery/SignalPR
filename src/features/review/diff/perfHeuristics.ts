import type { FileDiffMetadata } from "@pierre/diffs";

export interface DiffStats {
  fileCount: number;
  totalLines: number;
}

const FILE_COUNT_THRESHOLD = 15;
const LINE_COUNT_THRESHOLD = 3000;

export function computeDiffStats(fileDiffs: FileDiffMetadata[]): DiffStats {
  let totalLines = 0;
  for (const fd of fileDiffs) {
    totalLines += fd.unifiedLineCount;
  }
  return { fileCount: fileDiffs.length, totalLines };
}

/**
 * Returns true when the diff is large enough that non-selected files
 * should be collapsed by default to prevent UI jank.
 */
export function shouldCollapseByDefault(stats: DiffStats): boolean {
  return stats.fileCount >= FILE_COUNT_THRESHOLD || stats.totalLines >= LINE_COUNT_THRESHOLD;
}
