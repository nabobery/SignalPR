import { describe, expect, it } from "vitest";
import type { Finding, ReviewFreshnessSummary } from "./types";
import {
  buildFindingTrustViewModel,
  buildRunTrustOverview,
  parseFindingExplanation,
} from "./trust";

function makeFinding(overrides: Partial<Finding> = {}): Finding {
  return {
    id: "f-1",
    review_run_id: "run-1",
    agent_type: "security",
    file_path: "src/main.rs",
    line_start: 10,
    line_end: 12,
    severity: "warning",
    confidence: 0.9,
    title: "Potential issue",
    body: "Details",
    evidence: null,
    status: "active",
    user_edited_body: null,
    user_severity_override: null,
    is_anchored: true,
    created_at: "2026-05-01T00:00:00Z",
    cluster_id: null,
    lane_id: "security",
    provider_name: "codex",
    diff_side: "RIGHT",
    diff_new_line: 10,
    fix_search: null,
    fix_replace: null,
    fix_explanation: null,
    fix_status: null,
    fingerprint: null,
    source_kind: "ai_provider",
    source_id: null,
    explain_json: null,
    ...overrides,
  };
}

const defaultFreshness: ReviewFreshnessSummary = {
  is_rerun: false,
  baseline_run_id: null,
  reviewed_head_sha: null,
  current_head_sha: null,
  head_changed_since_review: false,
  rerun_trigger_source: null,
  rerun_reason: null,
  rerun_scope: null,
};

describe("parseFindingExplanation", () => {
  it("parses versioned explanation payloads", () => {
    const parsed = parseFindingExplanation(
      JSON.stringify({
        schema_version: 1,
        origin: {
          source_kind: "ai_provider",
          source_id: null,
          lane_id: "security",
          provider_name: "codex",
        },
        ranking: { confidence_raw: 0.85, severity_raw: "warning" },
      }),
    );

    expect(parsed?.schema_version).toBe(1);
    expect(parsed?.origin.provider_name).toBe("codex");
  });

  it("defaults legacy payloads to schema version 1", () => {
    const parsed = parseFindingExplanation(
      JSON.stringify({
        origin: {
          source_kind: "local_check",
          source_id: "oxlint:no-unused-vars",
          lane_id: "security",
          provider_name: null,
        },
      }),
    );

    expect(parsed?.schema_version).toBe(1);
    expect(parsed?.origin.source_kind).toBe("local_check");
  });

  it("keeps parsing older origin payloads by using finding-level fallbacks", () => {
    const parsed = parseFindingExplanation(
      JSON.stringify({
        origin: {
          source_kind: "local_check",
        },
      }),
      {
        source_kind: "local_check",
        source_id: "oxlint:no-unused-vars",
        lane_id: "security",
        provider_name: null,
      },
    );

    expect(parsed?.schema_version).toBe(1);
    expect(parsed?.origin.source_id).toBe("oxlint:no-unused-vars");
    expect(parsed?.origin.lane_id).toBe("security");
  });

  it("fails soft on malformed JSON", () => {
    expect(parseFindingExplanation("nope")).toBeNull();
  });
});

describe("buildFindingTrustViewModel", () => {
  it("marks local checks as deterministic support", () => {
    const trust = buildFindingTrustViewModel(
      makeFinding({
        source_kind: "local_check",
        source_id: "oxlint:no-unused-vars",
      }),
    );

    expect(trust.supportBadges.some((badge) => badge.label === "Deterministic")).toBe(true);
    expect(trust.warningBadges.some((badge) => badge.label === "AI inference only")).toBe(false);
  });

  it("marks ai-only findings with a caution badge", () => {
    const trust = buildFindingTrustViewModel(makeFinding());
    expect(trust.warningBadges.some((badge) => badge.label === "AI inference only")).toBe(true);
  });

  it("does not treat run-level platform metadata as deterministic support by itself", () => {
    const trust = buildFindingTrustViewModel(makeFinding(), {
      platformMetadata: {
        platform: "github",
        pr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: false,
        labels: ["backend"],
        requested_reviewers: [],
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
    });

    expect(trust.hasPlatformContext).toBe(true);
    expect(trust.hasDeterministicSupport).toBe(false);
    expect(trust.warningBadges.some((badge) => badge.label === "AI inference only")).toBe(true);
  });
});

describe("buildRunTrustOverview", () => {
  it("summarizes trust inputs across active findings", () => {
    const findings = [
      makeFinding({
        evidence: "stack trace",
        explain_json: JSON.stringify({
          schema_version: 1,
          origin: {
            source_kind: "ai_provider",
            source_id: null,
            lane_id: "security",
            provider_name: "codex",
          },
          issue_context: {
            included_count: 2,
            sources: ["github:issue:#1", "jira:AUTH-42"],
          },
          ownership: {
            owners: ["@backend"],
          },
        }),
      }),
      makeFinding({
        id: "f-2",
        source_kind: "local_check",
        source_id: "oxlint:no-unused-vars",
      }),
    ];

    const overview = buildRunTrustOverview({
      findings,
      localChecksSummary: {
        total_errors: 3,
        included_count: 2,
        tools_run: ["oxlint"],
        items: [],
      },
      platformMetadata: {
        platform: "github",
        pr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: false,
        labels: ["backend"],
        requested_reviewers: [],
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
      platformMetadataFetchedAt: new Date().toISOString(),
      reviewFreshness: {
        ...defaultFreshness,
        is_rerun: true,
        head_changed_since_review: true,
      },
    });

    expect(overview.sourceCounts[0]?.count).toBeGreaterThan(0);
    expect(overview.findingsWithEvidence).toBe(1);
    expect(overview.findingsWithIssueContext).toBe(1);
    expect(overview.findingsWithOwnership).toBe(1);
    expect(overview.localChecksIncluded).toBe(2);
    expect(overview.hasPlatformMetadata).toBe(true);
    expect(overview.reviewFreshnessLabel).toBe("Rerun against newer head");
  });
});
