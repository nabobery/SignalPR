import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ReviewContext, type ReviewState, type ReviewContextType } from "../../lib/store";
import { DraftReviewTab } from "./DraftReviewTab";
import type { Finding } from "../../lib/types";

const mockGetReviewDraft = vi.fn();
const mockGetEnvironmentSummary = vi.fn();
const mockSaveReviewDraft = vi.fn();
const mockSubmitReview = vi.fn();

vi.mock("../../lib/ipc", () => ({
  getEnvironmentSummary: (...args: unknown[]) => mockGetEnvironmentSummary(...args),
  getReviewDraft: (...args: unknown[]) => mockGetReviewDraft(...args),
  saveReviewDraft: (...args: unknown[]) => mockSaveReviewDraft(...args),
  submitReview: (...args: unknown[]) => mockSubmitReview(...args),
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
    title: "Unused variable",
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
    diff_new_line: 10,
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

function renderDraft(stateOverrides: Partial<ReviewState> = {}, onSubmitted = vi.fn()) {
  const state: ReviewState = {
    runId: "run-1",
    prId: "pr-1",
    status: "ready",
    prTitle: "Test PR",
    prNumber: 42,
    prUrl: "",
    diffText: null,
    changedFiles: ["src/main.rs"],
    findings: [
      makeFinding({ id: "f-1" }),
      makeFinding({ id: "f-2", file_path: "src/lib.rs", diff_new_line: null, is_anchored: false }),
    ],
    errorMessage: null,
    laneStatuses: [
      {
        lane_id: "security",
        status: "completed",
        finding_count: 2,
        provider_name: "codex",
        error_message: null,
      },
    ],
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
    platformMetadata: {
      platform: "github",
      pr_body: null,
      head_sha: "abc123",
      base_sha: "def456",
      base_ref: "main",
      head_ref: "feature/auth",
      draft: false,
      labels: [],
      requested_reviewers: [],
      requested_teams: [],
      review_state_summary: [],
      linked_issue_numbers: [],
      text_issue_refs: [],
    },
    platformMetadataFetchedAt: "2026-05-16T10:00:00Z",
    platformCapabilities: {
      platform: "github",
      capabilities: [
        { key: "review_summary_comment", support: "full", constraints: [], fallback: null },
        { key: "approve_review", support: "full", constraints: [], fallback: null },
        { key: "request_changes_review", support: "full", constraints: [], fallback: null },
        { key: "pending_comment_batch", support: "full", constraints: [], fallback: null },
      ],
    },
    platformCapabilitiesFetchedAt: "2026-05-16T10:00:00Z",
    ...stateOverrides,
  };

  const ctx: ReviewContextType = {
    state,
    setSelectedFile: vi.fn(),
    setSessionDecision: vi.fn(),
    refreshSnapshot: vi.fn(),
    revealFinding: vi.fn(),
  };

  return render(
    <ReviewContext.Provider value={ctx}>
      <DraftReviewTab runId="run-1" onSubmitted={onSubmitted} />
    </ReviewContext.Provider>,
  );
}

describe("DraftReviewTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetEnvironmentSummary.mockResolvedValue({
      can_review: true,
      can_submit: true,
      available_providers: ["codex"],
      warnings: [],
      tools: [
        {
          tool_name: "github_token",
          status: "ready",
          version: null,
          message: "GITHUB_TOKEN set",
          checked_at: "2026-05-16T10:00:00Z",
        },
      ],
    });
    mockGetReviewDraft.mockResolvedValue(null);
    mockSaveReviewDraft.mockResolvedValue(undefined);
    mockSubmitReview.mockResolvedValue(undefined);
  });

  it("loads existing draft on mount", async () => {
    mockGetReviewDraft.mockResolvedValue({
      run_id: "run-1",
      summary_markdown: "Great PR!",
      review_action: "approve",
      updated_at: "",
    });

    renderDraft();

    await waitFor(() => {
      const textarea = screen.getByPlaceholderText(/Write an optional summary/);
      expect(textarea).toHaveValue("Great PR!");
    });
    const radios = screen.getAllByRole("radio") as HTMLInputElement[];
    const approveRadio = radios.find((r) => r.value === "approve");
    expect(approveRadio).toBeDefined();
    expect(approveRadio!.checked).toBe(true);
  });

  it("renders summary textarea and action radios", async () => {
    renderDraft();

    await waitFor(() => {
      expect(screen.getByPlaceholderText(/Write an optional summary/)).toBeInTheDocument();
    });
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(3);
    expect(screen.getByText("Comment")).toBeInTheDocument();
    expect(screen.getByText("Approve")).toBeInTheDocument();
    expect(screen.getByText("Request Changes")).toBeInTheDocument();
  });

  it("shows pending comments grouped by file", async () => {
    renderDraft();

    await waitFor(() => {
      expect(screen.getByText("src/main.rs")).toBeInTheDocument();
      expect(screen.getByText("src/lib.rs")).toBeInTheDocument();
    });
    expect(screen.getByText(/1 inline comment/)).toBeInTheDocument();
    expect(screen.getByText(/2 file/)).toBeInTheDocument();
  });

  it("shows stale anchor warning when findings are unanchored", async () => {
    renderDraft({
      findings: [makeFinding({ id: "f-1", is_anchored: false, file_path: "src/main.rs" })],
    });

    await waitFor(() => {
      expect(screen.getByText(/stale line anchors/)).toBeInTheDocument();
    });
  });

  it("shows partial lane warning when a lane failed", async () => {
    renderDraft({
      laneStatuses: [
        {
          lane_id: "security",
          status: "completed",
          finding_count: 2,
          provider_name: "codex",
          error_message: null,
        },
        {
          lane_id: "arch",
          status: "failed",
          finding_count: 0,
          provider_name: "mock",
          error_message: "timeout",
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText(/Some analysis lanes failed/)).toBeInTheDocument();
    });
  });

  it("disables unsupported review actions from platform capabilities", async () => {
    renderDraft({
      platformCapabilities: {
        platform: "bitbucket",
        capabilities: [
          {
            key: "review_summary_comment",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "approve_review",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "request_changes_review",
            support: "none",
            constraints: [
              {
                code: "unsupported_request_changes",
                message: "Request changes is not supported on this platform.",
              },
            ],
            fallback: null,
          },
          {
            key: "pending_comment_batch",
            support: "full",
            constraints: [],
            fallback: null,
          },
        ],
      },
    });

    const blockedRadio = (await screen.findAllByRole("radio")).find(
      (radio) => (radio as HTMLInputElement).value === "request-changes",
    ) as HTMLInputElement | undefined;
    expect(blockedRadio).toBeDefined();
    expect(blockedRadio).toBeDisabled();
    expect(
      screen.getByText(/Request changes is not supported on this platform/),
    ).toBeInTheDocument();
  });

  it("blocks submission when platform auth is not ready", async () => {
    mockGetEnvironmentSummary.mockResolvedValue({
      can_review: true,
      can_submit: false,
      available_providers: ["codex"],
      warnings: ["No submit path ready"],
      tools: [
        {
          tool_name: "github_token",
          status: "missing",
          version: null,
          message: "Set GITHUB_TOKEN or GH_TOKEN",
          checked_at: "2026-05-16T10:00:00Z",
        },
        {
          tool_name: "gh",
          status: "unauthenticated",
          version: "2.0.0",
          message: "Run: gh auth login",
          checked_at: "2026-05-16T10:00:00Z",
        },
      ],
    });

    renderDraft({
      platformCapabilities: {
        platform: "github",
        capabilities: [
          {
            key: "review_summary_comment",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "approve_review",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "request_changes_review",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "pending_comment_batch",
            support: "full",
            constraints: [],
            fallback: null,
          },
        ],
      },
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
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
    });

    expect(await screen.findAllByText(/Set GITHUB_TOKEN or GH_TOKEN/)).toHaveLength(2);
    expect(screen.getByRole("button", { name: /Submit review/ })).toBeDisabled();
  });

  it("blocks submission when capability metadata has not been loaded", async () => {
    renderDraft({
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
        requested_teams: [],
        review_state_summary: [],
        linked_issue_numbers: [],
        text_issue_refs: [],
      },
      platformCapabilities: null,
    });

    expect(
      await screen.findAllByText(/Refresh platform metadata to load the available review actions/),
    ).toHaveLength(2);
    expect(screen.getByRole("button", { name: /Submit review/ })).toBeDisabled();
  });

  it("surfaces degraded batch behavior when pending comment batching is partial", async () => {
    renderDraft({
      platformCapabilities: {
        platform: "gitlab",
        capabilities: [
          {
            key: "review_summary_comment",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "approve_review",
            support: "full",
            constraints: [],
            fallback: null,
          },
          {
            key: "request_changes_review",
            support: "partial",
            constraints: [
              {
                code: "maps_to_unapprove",
                message: "Request changes currently maps to an approval removal flow.",
              },
            ],
            fallback: null,
          },
          {
            key: "pending_comment_batch",
            support: "partial",
            constraints: [
              {
                code: "draft_notes_only",
                message: "Pending review batches are preserved as draft notes only.",
              },
            ],
            fallback: null,
          },
        ],
      },
    });

    await waitFor(() => {
      expect(
        screen.getByText(/Pending review batches are preserved as draft notes only/),
      ).toBeInTheDocument();
    });
  });

  it("shows trust chips in pending comment preview", async () => {
    renderDraft({
      findings: [
        makeFinding({
          source_kind: "local_check",
          source_id: "oxlint:no-unused-vars",
        }),
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("Local check")).toBeInTheDocument();
      expect(screen.getByText("Deterministic")).toBeInTheDocument();
    });
  });

  it("warns when findings rely on AI inference only", async () => {
    renderDraft({
      findings: [makeFinding({ source_kind: "ai_provider", explain_json: null, evidence: null })],
    });

    await waitFor(() => {
      expect(screen.getByText(/AI inference without deterministic support/i)).toBeInTheDocument();
    });
  });

  it("submits review with summary and action", async () => {
    const onSubmitted = vi.fn();
    const user = userEvent.setup();

    renderDraft({}, onSubmitted);

    await waitFor(() => {
      expect(screen.getByPlaceholderText(/Write an optional summary/)).toBeInTheDocument();
    });

    const textarea = screen.getByPlaceholderText(/Write an optional summary/);
    await user.type(textarea, "Looks good");

    const radios = screen.getAllByRole("radio") as HTMLInputElement[];
    const approveRadio = radios.find((r) => r.value === "approve")!;
    await user.click(approveRadio);

    const submitBtn = screen.getByRole("button", { name: /Submit review/ });
    await user.click(submitBtn);

    await waitFor(() => {
      expect(mockSaveReviewDraft).toHaveBeenCalled();
      expect(mockSubmitReview).toHaveBeenCalledWith("run-1", "approve", undefined, "Looks good");
      expect(onSubmitted).toHaveBeenCalled();
    });
  });

  it("shows Force resubmit when already submitted", async () => {
    renderDraft({ status: "submitted" });

    await waitFor(() => {
      expect(screen.getByText("Already submitted.")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /Force resubmit/ })).toBeInTheDocument();
    });
  });

  it("disables submit when no active findings and summary is empty", async () => {
    renderDraft({ findings: [] });

    await waitFor(() => {
      expect(screen.getByText(/No active findings/)).toBeInTheDocument();
    });
    const submitBtn = screen.getByRole("button", { name: /Submit review/ });
    expect(submitBtn).toBeDisabled();
  });

  it("allows summary-only submission when no active findings", async () => {
    const onSubmitted = vi.fn();
    const user = userEvent.setup();
    renderDraft({ findings: [] }, onSubmitted);

    await waitFor(() => {
      expect(screen.getByPlaceholderText(/Write an optional summary/)).toBeInTheDocument();
    });

    await user.type(
      screen.getByPlaceholderText(/Write an optional summary/),
      "Summary only review",
    );

    const submitBtn = screen.getByRole("button", { name: /Submit review/ });
    expect(submitBtn).not.toBeDisabled();
    await user.click(submitBtn);

    await waitFor(() => {
      expect(mockSubmitReview).toHaveBeenCalledWith(
        "run-1",
        "comment",
        undefined,
        "Summary only review",
      );
      expect(onSubmitted).toHaveBeenCalled();
    });
  });

  it("shows error when submit fails", async () => {
    mockSubmitReview.mockRejectedValue("Submit failed");

    const user = userEvent.setup();
    renderDraft();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /Submit review/ })).toBeInTheDocument();
    });

    await user.click(screen.getByRole("button", { name: /Submit review/ }));

    await waitFor(() => {
      expect(screen.getByText("Submit failed")).toBeInTheDocument();
    });
  });

  it("autosave uses latest action after action change", async () => {
    renderDraft();

    const textarea = await screen.findByPlaceholderText(/Write an optional summary/);
    vi.useFakeTimers();
    fireEvent.change(textarea, { target: { value: "Latest value" } });

    const radios = screen.getAllByRole("radio") as HTMLInputElement[];
    const approveRadio = radios.find((r) => r.value === "approve")!;
    fireEvent.click(approveRadio);

    await act(async () => {
      vi.advanceTimersByTime(2000);
      await Promise.resolve();
    });

    const lastCall = mockSaveReviewDraft.mock.calls[mockSaveReviewDraft.mock.calls.length - 1];
    expect(lastCall).toEqual(["run-1", "Latest value", "approve"]);

    vi.useRealTimers();
  });
});
