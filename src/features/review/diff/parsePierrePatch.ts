import { parsePatchFiles } from "@pierre/diffs";
import type { FileDiffMetadata } from "@pierre/diffs";

export type { FileDiffMetadata };

/**
 * Parses a unified diff string into a flat list of FileDiffMetadata using
 * Pierre's parsePatchFiles under the hood. Handles multi-commit patches by
 * flattening all parsed patches into a single ordered file list.
 *
 * Returns an empty array for empty input. Throws on malformed input so
 * callers (e.g. ErrorBoundary) can detect and handle the failure.
 */
export function parsePierrePatch(diffText: string): FileDiffMetadata[] {
  if (!diffText || !diffText.trim()) {
    return [];
  }

  const patches = parsePatchFiles(diffText, undefined, true);
  const files: FileDiffMetadata[] = [];

  for (const patch of patches) {
    for (const file of patch.files) {
      files.push(file);
    }
  }

  return files;
}
