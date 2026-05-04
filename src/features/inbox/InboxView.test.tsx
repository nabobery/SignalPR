import { describe, it, expect, vi, beforeEach } from "vitest";
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
vi.mock("../../lib/ipc", () => ({
  getInboxOverview: (...args: unknown[]) => mockGetInboxOverview(...args),
  resumeReview: (...args: unknown[]) => mockResumeReview(...args),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

import { InboxView } from "./InboxView";
import type { InboxOverview } from "../../lib/types";

function makeOverview(overrides: Partial<InboxOverview> = {}): InboxOverview {
  return {
    environment_summary: {
      can_review: true,
      can_submit: true,
      available_providers: ["codex"],
      warnings: [],
      tools: [{ tool_name: "gh", status: "ready", version: "2.0", message: null, checked_at: "" }],
    },
    incomplete_reviews: [],
    recent_reviews: [],
    recent_workspaces: [],
    ...overrides,
  };
}

describe("InboxView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows loading spinner then renders overview", async () => {
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
  });

  it("shows error when fetch fails", async () => {
    mockGetInboxOverview.mockRejectedValue("network error");

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("network error")).toBeInTheDocument();
    });
  });

  it("renders incomplete reviews with Restart and Open buttons", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        incomplete_reviews: [
          {
            run_id: "run-1",
            pr_id: "pr-1",
            pr_number: 42,
            title: "Fix auth bug",
            author: "alice",
            pr_url: "https://github.com/org/repo/pull/42",
            status: "running_agents",
            last_updated: "2026-05-04T00:00:00Z",
            active_finding_count: 3,
            providers_used: ["codex"],
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
      expect(screen.getByText("Fix auth bug")).toBeInTheDocument();
    });
    expect(screen.getByText("#42")).toBeInTheDocument();
    expect(screen.getByText("alice")).toBeInTheDocument();
    expect(screen.getByText("3 findings")).toBeInTheDocument();
    expect(screen.getByText("Restart")).toBeInTheDocument();
    expect(screen.getByText("Open")).toBeInTheDocument();
  });

  it("renders recent reviews without Restart button", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        recent_reviews: [
          {
            run_id: "run-2",
            pr_id: "pr-2",
            pr_number: 99,
            title: "Add caching layer",
            author: "bob",
            pr_url: "https://github.com/org/repo/pull/99",
            status: "submitted",
            last_updated: "2026-05-03T00:00:00Z",
            active_finding_count: 0,
            providers_used: [],
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
      expect(screen.getByText("Add caching layer")).toBeInTheDocument();
    });
    expect(screen.queryByText("Restart")).not.toBeInTheDocument();
    expect(screen.getByText("Open")).toBeInTheDocument();
  });

  it("clicking Restart calls resumeReview and navigates", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        incomplete_reviews: [
          {
            run_id: "run-1",
            pr_id: "pr-1",
            pr_number: 42,
            title: "Fix auth bug",
            author: "alice",
            pr_url: "",
            status: "running_agents",
            last_updated: "",
            active_finding_count: 0,
            providers_used: [],
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
      expect(screen.getByText("Restart")).toBeInTheDocument();
    });

    await user.click(screen.getByText("Restart"));

    await waitFor(() => {
      expect(mockResumeReview).toHaveBeenCalledWith("run-1");
      expect(mockNavigate).toHaveBeenCalledWith("/review/new-run-id");
    });
  });

  it("clicking Open navigates to review", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        recent_reviews: [
          {
            run_id: "run-2",
            pr_id: "pr-2",
            pr_number: 99,
            title: "Add caching layer",
            author: null,
            pr_url: "",
            status: "ready",
            last_updated: "",
            active_finding_count: 0,
            providers_used: [],
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
      expect(screen.getByText("Open")).toBeInTheDocument();
    });

    await user.click(screen.getByText("Open"));
    expect(mockNavigate).toHaveBeenCalledWith("/review/run-2");
  });

  it("shows empty state when no reviews exist", async () => {
    mockGetInboxOverview.mockResolvedValue(makeOverview());

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText(/No reviews yet/)).toBeInTheDocument();
    });
  });

  it("renders recent workspaces when present", async () => {
    mockGetInboxOverview.mockResolvedValue(
      makeOverview({
        recent_workspaces: [
          {
            workspace_id: "ws-1",
            local_path: "/Users/test/repo",
            remote_owner: "org",
            remote_repo: "repo",
            last_reviewed_at: "2026-05-03T00:00:00Z",
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
      expect(screen.getByText("org/repo")).toBeInTheDocument();
      expect(screen.getByText("/Users/test/repo")).toBeInTheDocument();
    });
  });

  it("shows readiness warnings when tools are missing", async () => {
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
      }),
    );

    render(
      <MemoryRouter>
        <InboxView />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("Setup needed")).toBeInTheDocument();
      expect(screen.getByText("gh CLI not found")).toBeInTheDocument();
    });
  });
});
