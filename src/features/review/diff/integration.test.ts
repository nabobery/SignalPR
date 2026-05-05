import { describe, it, expect } from "vitest";
import { parsePierrePatch } from "./parsePierrePatch";
import { mapFindingsToLineAnnotations } from "./mapFindingsToLineAnnotations";
import { normalizeFilePath } from "./normalizeFilePath";
import { computeDiffStats, shouldCollapseByDefault } from "./perfHeuristics";
import sampleDiff from "./__fixtures__/sample-diff.txt?raw";
import largeDiff from "./__fixtures__/large-diff.txt?raw";
import type { Finding } from "../../../lib/types";

function makeFinding(overrides: Partial<Finding>): Finding {
  return {
    id: "f1",
    review_run_id: "run1",
    agent_type: "security",
    file_path: null,
    line_start: null,
    line_end: null,
    severity: "warning",
    confidence: 0.9,
    title: "Issue",
    body: "Details",
    evidence: null,
    status: "active",
    user_edited_body: null,
    user_severity_override: null,
    is_anchored: true,
    created_at: "2026-04-30T00:00:00Z",
    cluster_id: null,
    lane_id: null,
    provider_name: null,
    diff_side: "RIGHT",
    diff_new_line: null,
    fix_search: null,
    fix_replace: null,
    fix_explanation: null,
    fix_status: null,
    source_kind: null,
    source_id: null,
    explain_json: null,
    ...overrides,
    fingerprint: overrides.fingerprint ?? null,
  };
}

describe("integration: small PR (few files)", () => {
  it("parses all files with correct types", () => {
    const files = parsePierrePatch(sampleDiff);
    expect(files).toHaveLength(4);
    expect(files.map((f) => f.type)).toEqual(["change", "new", "rename-changed", "deleted"]);
  });

  it("maps findings to the correct files", () => {
    const files = parsePierrePatch(sampleDiff);
    const findings = [
      makeFinding({
        id: "f-math",
        file_path: "src/utils/math.ts",
        diff_new_line: 5,
        line_start: 5,
      }),
      makeFinding({
        id: "f-string",
        file_path: "src/utils/string.ts",
        diff_new_line: 2,
        line_start: 2,
      }),
    ];

    const mathAnnotations = mapFindingsToLineAnnotations(findings, files[0].name);
    const stringAnnotations = mapFindingsToLineAnnotations(findings, files[1].name);

    expect(mathAnnotations).toHaveLength(1);
    expect(mathAnnotations[0].metadata!.findings[0].id).toBe("f-math");
    expect(stringAnnotations).toHaveLength(1);
    expect(stringAnnotations[0].metadata!.findings[0].id).toBe("f-string");
  });
});

describe("integration: large PR (many files)", () => {
  it("parses 16 files correctly", () => {
    const files = parsePierrePatch(largeDiff);
    expect(files).toHaveLength(16);
  });

  it("triggers collapse-by-default heuristic", () => {
    const files = parsePierrePatch(largeDiff);
    const stats = computeDiffStats(files);
    expect(stats.fileCount).toBe(16);
    expect(shouldCollapseByDefault(stats)).toBe(true);
  });
});

describe("integration: rename-heavy PR", () => {
  it("preserves prevName and allows finding matching via old path", () => {
    const files = parsePierrePatch(sampleDiff);
    const renamed = files.find((f) => f.prevName === "src/old-config.ts");
    expect(renamed).toBeDefined();
    expect(renamed!.name).toBe("src/config.ts");

    const findings = [
      makeFinding({
        id: "f-old",
        file_path: "src/old-config.ts",
        diff_new_line: 1,
        line_start: 1,
      }),
    ];

    const annotations = mapFindingsToLineAnnotations(findings, "src/old-config.ts");
    expect(annotations).toHaveLength(1);
  });
});

describe("integration: deleted-file PR", () => {
  it("correctly identifies deleted files", () => {
    const files = parsePierrePatch(sampleDiff);
    const deleted = files.find((f) => f.type === "deleted");
    expect(deleted).toBeDefined();
    expect(deleted!.name).toBe("src/deprecated.ts");
  });

  it("does not crash when mapping findings to a deleted file", () => {
    const findings = [
      makeFinding({
        id: "f-dep",
        file_path: "src/deprecated.ts",
        diff_new_line: 1,
        line_start: 1,
      }),
    ];
    const annotations = mapFindingsToLineAnnotations(findings, "src/deprecated.ts");
    expect(annotations).toHaveLength(1);
  });
});

describe("integration: parse edge cases", () => {
  it("returns empty for non-diff text (Pierre is lenient)", () => {
    expect(parsePierrePatch("not a valid diff")).toEqual([]);
  });

  it("returns empty array for empty/whitespace-only input", () => {
    expect(parsePierrePatch("")).toEqual([]);
    expect(parsePierrePatch("   \n  ")).toEqual([]);
  });
});

describe("integration: path normalization", () => {
  it("strips a/ prefix when file is in known set", () => {
    const known = new Set(["src/utils/math.ts"]);
    expect(normalizeFilePath("a/src/utils/math.ts", known)).toBe("src/utils/math.ts");
  });

  it("strips b/ prefix when file is in known set", () => {
    const known = new Set(["src/utils/math.ts"]);
    expect(normalizeFilePath("b/src/utils/math.ts", known)).toBe("src/utils/math.ts");
  });

  it("does not strip prefix when stripped path is not in known set", () => {
    const known = new Set(["other/file.ts"]);
    expect(normalizeFilePath("a/src/utils/math.ts", known)).toBe("a/src/utils/math.ts");
  });

  it("strips unconditionally when no known set provided", () => {
    expect(normalizeFilePath("b/src/file.ts")).toBe("src/file.ts");
  });

  it("leaves normal paths untouched", () => {
    expect(normalizeFilePath("src/file.ts")).toBe("src/file.ts");
  });

  it("matches findings with a/ prefix to the correct file annotations", () => {
    const known = new Set(["src/utils/math.ts"]);
    const findings = [
      makeFinding({
        id: "f-prefixed",
        file_path: "a/src/utils/math.ts",
        diff_new_line: 3,
        line_start: 3,
      }),
    ];
    const annotations = mapFindingsToLineAnnotations(
      findings,
      "src/utils/math.ts",
      undefined,
      known,
    );
    expect(annotations).toHaveLength(1);
    expect(annotations[0].metadata!.findings[0].id).toBe("f-prefixed");
  });
});

describe("integration: annotation ordering", () => {
  it("returns annotations sorted by lineNumber ascending", () => {
    const findings = [
      makeFinding({ id: "f-line-20", file_path: "src/file.ts", diff_new_line: 20, line_start: 20 }),
      makeFinding({ id: "f-line-5", file_path: "src/file.ts", diff_new_line: 5, line_start: 5 }),
      makeFinding({ id: "f-line-12", file_path: "src/file.ts", diff_new_line: 12, line_start: 12 }),
    ];
    const annotations = mapFindingsToLineAnnotations(findings, "src/file.ts");
    const lines = annotations.map((a) => a.lineNumber);
    expect(lines).toEqual([5, 12, 20]);
  });

  it("groups multiple findings on the same line sorted by severity", () => {
    const findings = [
      makeFinding({
        id: "f-info",
        file_path: "src/file.ts",
        diff_new_line: 10,
        line_start: 10,
        severity: "info",
      }),
      makeFinding({
        id: "f-blocker",
        file_path: "src/file.ts",
        diff_new_line: 10,
        line_start: 10,
        severity: "blocker",
      }),
    ];
    const annotations = mapFindingsToLineAnnotations(findings, "src/file.ts");
    expect(annotations).toHaveLength(1);
    expect(annotations[0].metadata!.highestSeverity).toBe("blocker");
    expect(annotations[0].metadata!.findings[0].id).toBe("f-blocker");
    expect(annotations[0].metadata!.findings[1].id).toBe("f-info");
  });
});
