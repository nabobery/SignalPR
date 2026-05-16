import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router";

const mockNavigate = vi.fn();
vi.mock("react-router", async () => {
  const actual = await vi.importActual("react-router");
  return { ...actual, useNavigate: () => mockNavigate };
});

vi.mock("../intake/IntakeQuickAction", () => ({
  IntakeQuickAction: () => <div data-testid="intake-quick-action">QuickAction</div>,
}));

const mockGetInboxOverview = vi.fn();
const mockResumeReview = vi.fn();
const mockRefreshPrMetadata = vi.fn();
const mockRerunReview = vi.fn();
vi.mock("../../lib/ipc", () => ({
  getInboxOverview: (...args: unknown[]) => mockGetInboxOverview(...args),
  resumeReview: (...args: unknown[]) => mockResumeReview(...args),
  refreshPrMetadata: (...args: unknown[]) => mockRefreshPrMetadata(...args),
  rerunReview: (...args: unknown[]) => mockRerunReview(...args),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

import { InboxView } from "./InboxView";
import type { InboxOverview, InboxReviewRow } from "../../lib/types";

function makeRow(overrides: Partial<InboxReviewRow> = {}): InboxReviewRow {
  return {
    run_id: "run-1",
    pr_id: "pr-1",
    pr_number: 42,
    title: "Tighten auth guards",
    author: "alice",
    pr_url: "https://github.com/octo/signal/pull/42",
    status: "ready",
    last_updated: "2026-01-15T12:00:00Z",
    active_finding_count: 3,
    providers_used: ["codex"],
    queue_state: "ready_to_submit",
    platform: "github",
    repo_owner: "octo",
    repo_name: "signal",
    remote_host: "github.com",
    workspace_id: "ws-1",
    workspace_path: "/Users/test/signal",
    draft: false,
    has_saved_review_draft: false,
    metadata_freshness: { fetched_at: "2026-01-15T12:00:00Z", is_stale: false },
    review_freshness: {
      state: "current",
      reviewed_at: "2026-01-15T12:00:00Z",
      reviewed_head_sha: "sha-1",
      current_head_sha: "sha-1",
      has_unreviewed_updates: false,
    },
    reviewer_signal: {
      has_signal: true,
      label: "Needs your review",
      precision: "exact",
      requested_reviewers: ["mona"],
      requested_teams: [],
    },
    lane_health: {
      state: "healthy",
      failed_count: 0,
      timed_out_count: 0,
      running_count: 0,
      completed_count: 3,
    },
    submission_health: {
      state: "none",
      submitted_at: null,
      review_action: null,
      commit_id: null,
      error_message: null,
    },
    attention_reasons: [],
    allowed_actions: ["open", "refresh_metadata"],
    ...overrides,
  };
}

function makeOverview(overrides: Partial<InboxOverview> = {}): InboxOverview {
  return {
    environment_summary: {
      can_review: true,
      can_submit: true,
      available_providers: ["codex"],
      warnings: [],
      tools: [{ tool_name: "gh", status: "ready", version: "2.0", message: null, checked_at: "" }],
    },
    attention_summary: {
      total_items: 0,
      failed_runs: 0,
      failed_submissions: 0,
      stale_metadata: 0,
      degraded_runs: 0,
    },
    sections: [
      {
        id: "ready_to_submit",
        title: "Ready to submit",
        items: [makeRow()],
      },
    ],
    recent_workspaces: [],
    ...overrides,
  };
}

describe("InboxView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders readiness banner and queue sections", async () => {
    mockGetInboxOverview.mockResolvedValue(makeOverview());

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    expect(screen.getByTestId("intake-quick-action")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByText("Ready to review")).toBeInTheDocument();
    });
    expect(screen.getAllByText("Ready to submit").length).toBeGreaterThan(0);
    expect(screen.getByText("Tighten auth guards")).toBeInTheDocument();
  });

  it("distinguishes PR draft from saved review draft badges", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        sections: [
          {
            id: "review_requested",
            title: "Review requested",
            items: [
              makeRow({
                pr_id: "pr-1",
                draft: true,
                has_saved_review_draft: false,
              }),
              makeRow({
                pr_id: "pr-2",
                run_id: "run-2",
                title: "Saved review only",
                draft: false,
                has_saved_review_draft: true,
              }),
            ],
          },
        ],
      }),
    );

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("PR draft")).toBeInTheDocument();
    });
    expect(screen.getByText("Review draft")).toBeInTheDocument();
  });

  it("renders updated-since-review rows with rerun action", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        sections: [
          {
            id: "updated_since_review",
            title: "Updated since review",
            items: [
              makeRow({
                queue_state: "updated_since_review",
                reviewer_signal: {
                  has_signal: false,
                  label: "No reviewer requested",
                  precision: "none",
                  requested_reviewers: [],
                  requested_teams: [],
                },
                review_freshness: {
                  state: "stale",
                  reviewed_at: "2026-01-14T12:00:00Z",
                  reviewed_head_sha: "sha-1",
                  current_head_sha: "sha-2",
                  has_unreviewed_updates: true,
                },
                allowed_actions: ["open", "refresh_metadata", "rerun"],
              }),
            ],
          },
        ],
      }),
    );

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getAllByText("Updated since review").length).toBeGreaterThan(0);
    });
    expect(screen.getByText(/Review freshness: stale/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Rerun" })).toBeInTheDocument();
  });

  it("shows queue/setup attention banner details", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        environment_summary: {
          can_review: false,
          can_submit: false,
          available_providers: [],
          warnings: ["gh CLI not found"],
          tools: [
            { tool_name: "gh", status: "missing", version: null, message: null, checked_at: "" },
          ],
        },
        attention_summary: {
          total_items: 2,
          failed_runs: 1,
          failed_submissions: 1,
          stale_metadata: 0,
          degraded_runs: 0,
        },
      }),
    );

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Queue or setup needs attention")).toBeInTheDocument();
    });
    expect(screen.getByText("gh CLI not found")).toBeInTheDocument();
    expect(screen.getByText("2 PRs need attention")).toBeInTheDocument();
  });

  it("filters queue rows by search text", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        sections: [
          {
            id: "review_queue",
            title: "Review requested",
            items: [
              makeRow({ pr_id: "pr-1", title: "Tighten auth guards" }),
              makeRow({
                pr_id: "pr-2",
                run_id: "run-2",
                pr_number: 7,
                title: "Add caching layer",
                repo_name: "api",
                repo_owner: "octo",
              }),
            ],
          },
        ],
      }),
    );

    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Add caching layer")).toBeInTheDocument();
    });

    await user.type(
      screen.getByPlaceholderText("Search PR, author, repo, workspace..."),
      "caching",
    );

    expect(screen.queryByText("Tighten auth guards")).not.toBeInTheDocument();
    expect(screen.getByText("Add caching layer")).toBeInTheDocument();
  });

  it("filters queue rows by repo and provider", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        sections: [
          {
            id: "review_queue",
            title: "Review requested",
            items: [
              makeRow({ pr_id: "pr-1", repo_name: "signal", providers_used: ["codex"] }),
              makeRow({
                pr_id: "pr-2",
                run_id: "run-2",
                repo_name: "api",
                title: "Add billing hooks",
                providers_used: ["copilot"],
              }),
            ],
          },
        ],
      }),
    );

    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Add billing hooks")).toBeInTheDocument();
    });

    const selects = screen.getAllByRole("combobox");
    await user.selectOptions(selects[0], "octo/api");
    await user.selectOptions(selects[2], "copilot");

    expect(screen.queryByText("Tighten auth guards")).not.toBeInTheDocument();
    expect(screen.getByText("Add billing hooks")).toBeInTheDocument();
  });

  it("clicking Resume calls resumeReview and navigates", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        sections: [
          {
            id: "in_progress",
            title: "In progress",
            items: [
              makeRow({
                queue_state: "in_progress",
                status: "running_agents",
                allowed_actions: ["open", "resume"],
              }),
            ],
          },
        ],
      }),
    );
    mockResumeReview.mockResolvedValue("new-run-id");

    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Resume")).toBeInTheDocument();
    });

    await user.click(screen.getByText("Resume"));

    await waitFor(() => {
      expect(mockResumeReview).toHaveBeenCalledWith("run-1");
      expect(mockNavigate).toHaveBeenCalledWith("/review/new-run-id");
    });
  });

  it("clicking Refresh metadata refreshes the row data", async () => {
    mockGetInboxOverview
      .mockResolvedValueOnce(
        makeOverview({
          sections: [
            {
              id: "attention_needed",
              title: "Attention needed",
              items: [
                makeRow({
                  metadata_freshness: { fetched_at: "2026-05-14T12:00:00Z", is_stale: true },
                  attention_reasons: ["Platform metadata is stale"],
                  queue_state: "attention_needed",
                }),
              ],
            },
          ],
        }),
      )
      .mockResolvedValueOnce(makeOverview());
    mockRefreshPrMetadata.mockResolvedValue(undefined);

    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Metadata stale")).toBeInTheDocument();
    });

    await user.click(screen.getByText("Refresh"));

    await waitFor(() => {
      expect(mockRefreshPrMetadata).toHaveBeenCalledWith("pr-1");
      expect(mockGetInboxOverview).toHaveBeenCalledTimes(2);
    });
  });

  it("shows empty filtered state when nothing matches", async () => {
    mockGetInboxOverview.mockResolvedValue(makeOverview());

    const user = userEvent.setup();
    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Tighten auth guards")).toBeInTheDocument();
    });

    await user.type(screen.getByPlaceholderText("Search PR, author, repo, workspace..."), "nope");

    expect(screen.getByText("No queue items match the current filters.")).toBeInTheDocument();
  });
});
