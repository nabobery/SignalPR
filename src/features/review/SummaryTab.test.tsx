import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { ReviewContext, type ReviewState, type ReviewContextType } from "../../lib/store";
import { SummaryTab } from "./SummaryTab";
import type { Finding, LaneSnapshot } from "../../lib/types";

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
    <ReviewContext.Provider value={ctx}>
      <SummaryTab />
    </ReviewContext.Provider>,
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
      },
      setSelectedFile,
      setSessionDecision: vi.fn(),
      refreshSnapshot: vi.fn(),
      revealFinding: vi.fn(),
    };

    const user = userEvent.setup();
    render(
      <ReviewContext.Provider value={ctx}>
        <SummaryTab />
      </ReviewContext.Provider>,
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
});
