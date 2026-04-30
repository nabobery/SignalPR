import { describe, it, expect } from "vitest";
import { shouldCollapseByDefault, type DiffStats } from "./perfHeuristics";

describe("shouldCollapseByDefault", () => {
  it("returns false for small diffs", () => {
    const stats: DiffStats = { fileCount: 5, totalLines: 200 };
    expect(shouldCollapseByDefault(stats)).toBe(false);
  });

  it("returns true when file count exceeds threshold", () => {
    const stats: DiffStats = { fileCount: 20, totalLines: 500 };
    expect(shouldCollapseByDefault(stats)).toBe(true);
  });

  it("returns true when total lines exceed threshold", () => {
    const stats: DiffStats = { fileCount: 3, totalLines: 5000 };
    expect(shouldCollapseByDefault(stats)).toBe(true);
  });
});
