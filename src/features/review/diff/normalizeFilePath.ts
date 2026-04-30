/**
 * Strips git diff `a/` / `b/` synthetic prefixes from a file path.
 * When a set of known changed files is provided, strips only if the
 * result matches a known file (avoids mangling paths in repos that
 * genuinely have top-level `a/` or `b/` directories).
 */
export function normalizeFilePath(path: string, knownFiles?: ReadonlySet<string>): string {
  if (path.startsWith("a/") || path.startsWith("b/")) {
    const stripped = path.slice(2);
    if (!knownFiles || knownFiles.has(stripped)) {
      return stripped;
    }
  }
  return path;
}
