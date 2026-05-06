import { describe, it, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useMemo, useState } from "react";
import { SignalBoard } from "./SignalBoard";
import { ReviewContext, type ReviewContextType, type ReviewState } from "../../lib/store";
import type { Finding } from "../../lib/types";

vi.mock("../../lib/ipc", () => ({
  updateFinding: vi.fn(() => Promise.resolve()),
  recordDecision: vi.fn(() => Promise.resolve()),
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
    body: "The variable `x` is never read.",
    evidence: null,
    status: "active",
    user_edited_body: null,
    user_severity_override: null,
    is_anchored: true,
    created_at: "2026-03-27T00:00:00Z",
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

function SignalBoardHarness() {
  const [findings, setFindings] = useState<Finding[]>([
    makeFinding({ id: "active-1", title: "Active finding", status: "active" }),
    makeFinding({ id: "supp-1", title: "Suppressed finding", status: "suppressed" }),
  ]);

  const state: ReviewState = useMemo(
    () => ({
      runId: "run-1",
      prId: "pr-1",
      status: "ready",
      prTitle: "Test PR",
      prNumber: 42,
      prUrl: "",
      diffText: null,
      changedFiles: ["src/main.rs"],
      findings,
      errorMessage: null,
      laneStatuses: [],
      clusters: [],
      selectedFile: null,
      focusedFindingId: null,
      sessionDecisions: {},
      baselineRunId: null,
      metrics: null,
      delta: null,
      contextPackSummary: null,
      localChecksSummary: null,
      platformMetadata: null,
      platformMetadataFetchedAt: null,
    }),
    [findings],
  );

  const ctx: ReviewContextType = {
    state,
    setSelectedFile: vi.fn(),
    setSessionDecision: vi.fn(),
    refreshSnapshot: async () => {
      setFindings((prev) => prev.map((f) => (f.id === "supp-1" ? { ...f, status: "active" } : f)));
    },
    revealFinding: vi.fn(),
  };

  return (
    <ReviewContext.Provider value={ctx}>
      <SignalBoard />
    </ReviewContext.Provider>
  );
}

describe("SignalBoard", () => {
  it("shows suppressed findings in Suppressed preset and restores to Active", async () => {
    const user = userEvent.setup();
    render(<SignalBoardHarness />);

    expect(screen.getByText("Active finding")).toBeInTheDocument();
    expect(screen.queryByText("Suppressed finding")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Suppressed" }));
    expect(screen.getByText("Suppressed finding")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restore" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Restore" }));
    await user.click(screen.getByRole("button", { name: "Active" }));

    await waitFor(() => {
      expect(screen.getByText("Suppressed finding")).toBeInTheDocument();
    });
  });
});
