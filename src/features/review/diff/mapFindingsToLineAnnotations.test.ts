import { describe, it, expect } from "vitest";
import { mapFindingsToLineAnnotations } from "./mapFindingsToLineAnnotations";
import type { Finding, FindingCluster } from "../../../lib/types";

function makeFinding(overrides: Partial<Finding> = {}): Finding {
  return {
    id: "f1",
    review_run_id: "run1",
    agent_type: "security",
    file_path: "src/utils/math.ts",
    line_start: 5,
    line_end: 7,
    severity: "warning",
    confidence: 0.9,
    title: "Potential issue",
    body: "Details here",
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
    diff_new_line: 5,
    fix_search: null,
    fix_replace: null,
    fix_explanation: null,
    fix_status: null,
    ...overrides,
    fingerprint: overrides.fingerprint ?? null,
  };
}

describe("mapFindingsToLineAnnotations", () => {
  it("maps anchored findings to additions side at diff_new_line", () => {
    const findings = [makeFinding({ diff_new_line: 5, diff_side: "RIGHT" })];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toHaveLength(1);
    expect(result[0].side).toBe("additions");
    expect(result[0].lineNumber).toBe(5);
  });

  it("uses line_start as fallback when diff_new_line is null", () => {
    const findings = [makeFinding({ diff_new_line: null, diff_side: null, line_start: 3 })];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toHaveLength(1);
    expect(result[0].side).toBe("additions");
    expect(result[0].lineNumber).toBe(3);
  });

  it("excludes file-level findings (no line anchor) from line annotations", () => {
    const findings = [
      makeFinding({
        line_start: null,
        line_end: null,
        diff_new_line: null,
        is_anchored: false,
      }),
    ];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toHaveLength(0);
  });

  it("excludes suppressed findings", () => {
    const findings = [makeFinding({ status: "suppressed" })];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toHaveLength(0);
  });

  it("only annotates cluster representatives (not duplicate members)", () => {
    const cluster: FindingCluster = {
      id: "c1",
      review_run_id: "run1",
      label: "Similar issues",
      representative_finding_id: "f-rep",
      member_count: 3,
      created_at: "2026-04-30T00:00:00Z",
    };

    const findings = [
      makeFinding({ id: "f-rep", cluster_id: "c1", diff_new_line: 10 }),
      makeFinding({ id: "f-member1", cluster_id: "c1", diff_new_line: 12 }),
      makeFinding({ id: "f-member2", cluster_id: "c1", diff_new_line: 14 }),
    ];

    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts", [cluster]);

    expect(result).toHaveLength(1);
    expect(result[0].lineNumber).toBe(10);
  });

  it("groups multiple findings on the same line into one annotation", () => {
    const findings = [
      makeFinding({ id: "f1", diff_new_line: 5, title: "Issue A" }),
      makeFinding({ id: "f2", diff_new_line: 5, title: "Issue B" }),
    ];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toHaveLength(1);
    expect(result[0].lineNumber).toBe(5);
    expect(result[0].metadata!.findings).toHaveLength(2);
  });

  it("filters findings by file path", () => {
    const findings = [
      makeFinding({
        id: "f1",
        file_path: "src/utils/math.ts",
        diff_new_line: 5,
      }),
      makeFinding({
        id: "f2",
        file_path: "src/utils/string.ts",
        diff_new_line: 3,
      }),
    ];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toHaveLength(1);
    expect(result[0].metadata!.findings[0].id).toBe("f1");
  });

  it("returns empty array when no findings match the file", () => {
    const findings = [makeFinding({ file_path: "src/other.ts", diff_new_line: 1 })];
    const result = mapFindingsToLineAnnotations(findings, "src/utils/math.ts");

    expect(result).toEqual([]);
  });
});
