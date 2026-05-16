import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";
import { ReviewContext, type ReviewState, type ReviewContextType } from "../../lib/store";
import { SummaryTab } from "./SummaryTab";
import type { Finding, LaneSnapshot, RunScorecard } from "../../lib/types";

vi.mock("../../lib/ipc", () => ({
  rerunReview: vi.fn(),
  refreshPrMetadata: vi.fn(),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

function makeFinding(overrides: Partial<Finding> = {}): Finding {
  return {
    id: "f-1",
    review_run_id: "run-1",
    agent_type: "security",
    file_path: "src/main.rs",
    line_start: 10,
    line_end: 15,
    severity: "warning",
    confidence: 0.85,
    title: "Test finding",
    body: "body",
    evidence: null,
    status: "active",
    user_edited_body: null,
    user_severity_override: null,
    is_anchored: true,
    created_at: "",
    cluster_id: null,
    lane_id: null,
    provider_name: null,
    diff_side: null,
    diff_new_line: null,
    fix_search: null,
    fix_replace: null,
    fix_explanation: null,
    fix_status: null,
    fingerprint: null,
    source_kind: null,
    source_id: null,
    explain_json: null,
    ...overrides,
  };
}

function makeLane(overrides: Partial<LaneSnapshot> = {}): LaneSnapshot {
  return {
    lane_id: "security",
    status: "completed",
    finding_count: 2,
    provider_name: "codex",
    error_message: null,
    ...overrides,
  };
}

function renderWithContext(state: Partial<ReviewState>) {
  const fullState: ReviewState = {
    runId: "run-1",
    prId: "pr-1",
    workspaceId: "ws-1",
    status: "ready",
    prTitle: "Test PR",
    prNumber: 42,
    prUrl: "",
    diffText: null,
    changedFiles: ["src/main.rs", "src/lib.rs"],
    findings: [],
    errorMessage: null,
    laneStatuses: [],
    clusters: [],
    selectedFile: null,
    focusedFindingId: null,
    sessionDecisions: {},
    baselineRunId: null,
    metrics: null,
    delta: null,
    reviewFreshness: {
      is_rerun: false,
      baseline_run_id: null,
      reviewed_head_sha: null,
      current_head_sha: null,
      head_changed_since_review: false,
      rerun_trigger_source: null,
      rerun_reason: null,
      rerun_scope: null,
    },
    contextPackSummary: null,
    localChecksSummary: null,
    platformMetadata: null,
    platformMetadataFetchedAt: null,
    platformCapabilities: null,
    platformCapabilitiesFetchedAt: null,
    providerSelection: null,
    providerControl: null,
    ...state,
  };

  const ctx: ReviewContextType = {
    state: fullState,
    setSelectedFile: vi.fn(),
    setSessionDecision: vi.fn(),
    refreshSnapshot: vi.fn(),
    revealFinding: vi.fn(),
  };

  return render(
    <MemoryRouter>
      <ReviewContext.Provider value={ctx}>
        <SummaryTab />
      </ReviewContext.Provider>
    </MemoryRouter>,
  );
}

describe("SummaryTab", () => {
  it("shows 'Review ready' when status is ready", () => {
    renderWithContext({ status: "ready" });
    expect(screen.getByText("Review ready")).toBeInTheDocument();
  });

  it("shows 'Submitted' when status is submitted", () => {
    renderWithContext({ status: "submitted" });
    expect(screen.getByText("Submitted")).toBeInTheDocument();
  });

  it("shows 'Analyzing...' when running_agents", () => {
    renderWithContext({ status: "running_agents" });
    expect(screen.getByText("Analyzing...")).toBeInTheDocument();
  });

  it("renders file count and active findings count", () => {
    renderWithContext({
      changedFiles: ["a.ts", "b.ts", "c.ts"],
      findings: [
        makeFinding({ id: "f-1", file_path: "a.ts" }),
        makeFinding({ id: "f-2", file_path: "a.ts" }),
      ],
    });
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("files changed")).toBeInTheDocument();
    expect(screen.getByText("active findings")).toBeInTheDocument();
    const statsCards = screen.getAllByText("2");
    expect(statsCards.length).toBeGreaterThanOrEqual(1);
  });

  it("renders lane statuses", () => {
    renderWithContext({
      laneStatuses: [
        makeLane({ lane_id: "security", status: "completed", provider_name: "codex" }),
        makeLane({
          lane_id: "performance",
          status: "failed",
          provider_name: "mock",
          error_message: "timeout",
        }),
      ],
    });

    expect(screen.getByText("security")).toBeInTheDocument();
    expect(screen.getByText("performance")).toBeInTheDocument();
    expect(screen.getByText("timeout")).toBeInTheDocument();
  });

  it("renders severity breakdown for active findings", () => {
    renderWithContext({
      findings: [
        makeFinding({ id: "f-1", severity: "blocker" }),
        makeFinding({ id: "f-2", severity: "warning" }),
        makeFinding({ id: "f-3", severity: "warning" }),
      ],
    });

    expect(screen.getByText("blocker")).toBeInTheDocument();
    expect(screen.getByText("warning")).toBeInTheDocument();
  });

  it("shows 'High risk' when blocker findings exist", () => {
    renderWithContext({
      findings: [makeFinding({ severity: "blocker" })],
    });
    expect(screen.getByText("High risk")).toBeInTheDocument();
  });

  it("renders hotspots and allows clicking to filter by file", async () => {
    const setSelectedFile = vi.fn();
    const ctx: ReviewContextType = {
      state: {
        runId: "run-1",
        prId: "pr-1",
        workspaceId: "ws-1",
        status: "ready",
        prTitle: "Test PR",
        prNumber: 42,
        prUrl: "",
        diffText: null,
        changedFiles: ["src/main.rs"],
        findings: [
          makeFinding({ id: "f-1", file_path: "src/main.rs" }),
          makeFinding({ id: "f-2", file_path: "src/main.rs" }),
          makeFinding({ id: "f-3", file_path: "src/lib.rs" }),
        ],
        errorMessage: null,
        laneStatuses: [],
        clusters: [],
        selectedFile: null,
        focusedFindingId: null,
        sessionDecisions: {},
        baselineRunId: null,
        metrics: null,
        delta: null,
        reviewFreshness: {
          is_rerun: false,
          baseline_run_id: null,
          reviewed_head_sha: null,
          current_head_sha: null,
          head_changed_since_review: false,
          rerun_trigger_source: null,
          rerun_reason: null,
          rerun_scope: null,
        },
        contextPackSummary: null,
        localChecksSummary: null,
        platformMetadata: null,
        platformMetadataFetchedAt: null,
        platformCapabilities: null,
        platformCapabilitiesFetchedAt: null,
        providerSelection: null,
        providerControl: null,
      },
      setSelectedFile,
      setSessionDecision: vi.fn(),
      refreshSnapshot: vi.fn(),
      revealFinding: vi.fn(),
    };

    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <ReviewContext.Provider value={ctx}>
          <SummaryTab />
        </ReviewContext.Provider>
      </MemoryRouter>,
    );

    expect(screen.getByText("src/main.rs")).toBeInTheDocument();
    expect(screen.getByText("src/lib.rs")).toBeInTheDocument();

    await user.click(screen.getByText("src/main.rs"));
    expect(setSelectedFile).toHaveBeenCalledWith("src/main.rs");
  });

  it("does not show severity section when no active findings", () => {
    renderWithContext({ findings: [] });
    expect(screen.queryByText("Severity breakdown")).not.toBeInTheDocument();
  });

  it("shows completed/total lanes in stats row", () => {
    renderWithContext({
      laneStatuses: [
        makeLane({ lane_id: "security", status: "completed" }),
        makeLane({ lane_id: "arch", status: "failed" }),
      ],
    });
    expect(screen.getByText("1/2")).toBeInTheDocument();
    expect(screen.getByText("lanes (1 failed)")).toBeInTheDocument();
  });

  it("renders Rerun button when status is ready", () => {
    renderWithContext({ status: "ready" });
    expect(screen.getByRole("button", { name: /rerun review/i })).toBeInTheDocument();
  });

  it("renders Rerun button when status is submitted", () => {
    renderWithContext({ status: "submitted" });
    expect(screen.getByRole("button", { name: /rerun review/i })).toBeInTheDocument();
  });

  it("does not render Rerun button when running", () => {
    renderWithContext({ status: "running_agents" });
    expect(screen.queryByRole("button", { name: /rerun/i })).not.toBeInTheDocument();
  });

  it("calls rerunReview and navigates on click", async () => {
    const { rerunReview } = await import("../../lib/ipc");
    (rerunReview as ReturnType<typeof vi.fn>).mockResolvedValue("new-run-id");

    const user = userEvent.setup();
    renderWithContext({ status: "ready" });

    await user.click(screen.getByRole("button", { name: /rerun/i }));
    expect(rerunReview).toHaveBeenCalledWith("run-1", {
      triggerSource: "workspace",
      reason: "manual",
    });
  });

  it("renders provider scorecard when metrics are present", () => {
    const scorecard: RunScorecard = {
      lanes: [
        {
          lane_id: "security",
          provider_name: "codex",
          lane_latency_ms: 12500,
          raw_findings_count: 10,
          surfaced_findings_count: 5,
          reviewer_accept_rate: 0.8,
          reviewer_edit_rate: 0.1,
          suppress_rate: 0.1,
          anchor_validity: 0.9,
          submission_inclusion_rate: 0.7,
          cost_usd: 0.0023,
        },
      ],
      overall_surfaced: 5,
      overall_accept_rate: 0.8,
      overall_edit_rate: 0.1,
      overall_suppress_rate: 0.1,
    };
    renderWithContext({ metrics: scorecard });
    expect(screen.getByText("Provider scorecard")).toBeInTheDocument();
    expect(screen.getByText("security")).toBeInTheDocument();
    expect(screen.getAllByText("80%").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Overall")).toBeInTheDocument();
    expect(screen.getByText("12.5s")).toBeInTheDocument();
  });

  it("renders provider-choice explanation when control data is present", () => {
    renderWithContext({
      providerSelection: {
        requested_provider: "claude",
        selected_provider: "codex",
        selection_mode: "fallback",
        checks: [],
        warnings: ["Requested provider 'claude' was unavailable, so SignalPR selected 'codex'."],
      },
      providerControl: {
        providers: [
          {
            provider_id: "codex",
            display_name: "Codex CLI",
            status: "ready",
            status_reason: "Ready for review runs",
            credential_source: null,
            capabilities: {
              provider_id: "codex",
              display_name: "Codex CLI",
              provider_family: "local_cli",
              fit_tags: ["balanced"],
              billing_risk: "included",
              setup_complexity: "moderate",
              opt_in_only: false,
              in_auto_fallback: true,
              credential_fields: [],
              interactive_permissions: false,
              default_governance_tier: "read_only",
              supports_session_resume: false,
              supports_checkpointing: false,
              paid_eval_eligible: false,
            },
            recent_metrics: {
              sample_count: 2,
              avg_latency_ms: 950,
              avg_accept_rate: 0.7,
              avg_edit_rate: 0.1,
              avg_suppress_rate: 0.1,
              avg_anchor_validity: 1,
              avg_cost_usd: null,
            },
            fit_narrative: "Ready now.",
            recommended_default: true,
            warnings: [],
          },
        ],
        recommended_provider_id: "codex",
        recommendation_reason: "Codex CLI is ready now.",
        preferred_provider: "auto",
        workspace_id: "ws-1",
        recent_window_size: 20,
        generated_at: "2026-05-16T10:00:00Z",
      },
    });

    expect(screen.getByText("Why this provider was chosen")).toBeInTheDocument();
    expect(
      screen.getAllByText((content) => content.includes(", selected "))[0],
    ).toBeInTheDocument();
    expect(screen.getByText("Recommended default")).toBeInTheDocument();
  });

  it("renders review trust overview", () => {
    renderWithContext({
      findings: [
        makeFinding({
          source_kind: "local_check",
          source_id: "oxlint:no-unused-vars",
          evidence: "unused variable",
          explain_json: JSON.stringify({
            schema_version: 1,
            origin: {
              source_kind: "local_check",
              source_id: "oxlint:no-unused-vars",
              lane_id: "security",
              provider_name: null,
            },
            issue_context: {
              included_count: 1,
              sources: ["github:issue:#42"],
            },
            ownership: {
              owners: ["@team-core"],
            },
          }),
        }),
      ],
      localChecksSummary: {
        total_errors: 2,
        included_count: 1,
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
    });

    expect(screen.getByText("Review trust overview")).toBeInTheDocument();
    expect(screen.getByText(/Local check: 1/)).toBeInTheDocument();
    expect(screen.getByText(/Evidence: 1/)).toBeInTheDocument();
    expect(screen.getByText(/Issue context: 1/)).toBeInTheDocument();
    expect(screen.getByText(/Owners: 1/)).toBeInTheDocument();
    expect(screen.getByText(/Local checks: 1 via oxlint/)).toBeInTheDocument();
  });

  it("renders delta summary for reruns", () => {
    renderWithContext({
      baselineRunId: "run-0",
      reviewFreshness: {
        is_rerun: true,
        baseline_run_id: "run-0",
        reviewed_head_sha: "sha-1",
        current_head_sha: "sha-2",
        head_changed_since_review: true,
        rerun_trigger_source: "workspace",
        rerun_reason: "manual",
        rerun_scope: "full_pr",
      },
      delta: {
        changed_files: ["src/main.rs"],
        changed_hunks_by_file: {},
        counts: { new: 3, unchanged: 2, stale: 1, resolved: 4 },
        resolved: [
          {
            id: "resolved-1",
            title: "Resolved issue",
            file_path: "src/main.rs",
            agent_type: "security",
            severity: "warning",
          },
        ],
      },
    });
    expect(screen.getByText("Changes since last run")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("new")).toBeInTheDocument();
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getAllByText("resolved").length).toBeGreaterThan(0);
    expect(screen.getByText("Resolved issue")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Rerun review/i })).toBeInTheDocument();
    expect(screen.getByText(/latest changes/i)).toBeInTheDocument();
  });

  it("does not show scorecard when metrics is null", () => {
    renderWithContext({ metrics: null });
    expect(screen.queryByText("Provider scorecard")).not.toBeInTheDocument();
  });

  // ---- Platform metadata tests ----

  it("renders GitHub metadata section with requested reviewers", () => {
    renderWithContext({
      platformMetadata: {
        platform: "github",
        pr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: false,
        labels: [],
        requested_reviewers: ["alice", "bob"],
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
      platformMetadataFetchedAt: "2026-05-06T00:00:00Z",
    });
    expect(screen.getByText("GitHub metadata")).toBeInTheDocument();
    expect(screen.getByText(/Requested reviewers/)).toBeInTheDocument();
    expect(screen.getByText(/alice, bob/)).toBeInTheDocument();
  });

  it("renders draft badge when PR is draft", () => {
    renderWithContext({
      platformMetadata: {
        platform: "github",
        pr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: true,
        labels: [],
        requested_reviewers: [],
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
    });
    expect(screen.getByText("Draft PR")).toBeInTheDocument();
  });

  it("renders labels from platform metadata", () => {
    renderWithContext({
      platformMetadata: {
        platform: "github",
        pr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: false,
        labels: ["bug", "security"],
        requested_reviewers: [],
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
    });
    expect(screen.getByText("bug")).toBeInTheDocument();
    expect(screen.getByText("security")).toBeInTheDocument();
  });

  it("renders requested teams from platform metadata", () => {
    renderWithContext({
      platformMetadata: {
        platform: "github",
        pr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: false,
        labels: [],
        requested_reviewers: [],
        requested_teams: ["security-team", "docs-team"],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
    });
    expect(screen.getByText(/Requested teams/)).toBeInTheDocument();
    expect(screen.getByText(/security-team, docs-team/)).toBeInTheDocument();
  });

  it("renders GitLab metadata with reviewers and approvals", () => {
    renderWithContext({
      platformMetadata: {
        platform: "gitlab",
        mr_body: null,
        head_sha: "abc",
        base_sha: "def",
        base_ref: "main",
        head_ref: "feature",
        draft: false,
        labels: ["fix"],
        reviewers: ["alice"],
        reviewer_statuses: [],
        approval_status: {
          approved: true,
          approved_by: ["alice"],
          approvals_required: 1,
          approvals_left: 0,
        },
        closes_issues: [42],
      },
    });
    expect(screen.getByText("GitLab metadata")).toBeInTheDocument();
    expect(screen.getAllByText(/alice/).length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Approved")).toBeInTheDocument();
  });

  it("renders Bitbucket metadata with reviewers, approvals, and Jira keys", () => {
    renderWithContext({
      platformMetadata: {
        platform: "bitbucket",
        pr_body: "Fix login flow",
        head_sha: "abc123",
        base_sha: "def456",
        head_ref: "feature/JIRA-42-login",
        base_ref: "main",
        draft: false,
        labels: [],
        reviewers: ["alice", "bob"],
        reviewer_statuses: [],
        approval_status: {
          approved: true,
          approved_by: ["alice"],
          approvals_required: null,
          approvals_left: null,
        },
        default_reviewers: ["teamlead"],
        jira_issue_keys: ["JIRA-42", "AUTH-7"],
      },
    });
    expect(screen.getByText("Bitbucket metadata")).toBeInTheDocument();
    expect(screen.getByText(/alice, bob/)).toBeInTheDocument();
    expect(screen.getByText("Approved")).toBeInTheDocument();
    expect(screen.getByText(/JIRA-42, AUTH-7/)).toBeInTheDocument();
    expect(screen.getByText(/teamlead/)).toBeInTheDocument();
    expect(
      screen.getByText(/Bitbucket does not support pending review groups/),
    ).toBeInTheDocument();
  });

  it("does not render metadata section when null", () => {
    renderWithContext({ platformMetadata: null });
    expect(screen.queryByText("GitHub metadata")).not.toBeInTheDocument();
    expect(screen.queryByText("GitLab metadata")).not.toBeInTheDocument();
    expect(screen.queryByText("Bitbucket metadata")).not.toBeInTheDocument();
  });
});
