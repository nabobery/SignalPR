import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import { DiffPanel } from "../DiffPanel";
import { ReviewContext, type ReviewState, type ReviewContextType } from "../../../lib/store";
import "@testing-library/jest-dom";

vi.mock("@pierre/diffs/react", () => ({
  FileDiff: ({ fileDiff }: { fileDiff: { name: string } }) => (
    <div data-testid={`pierre-file-${fileDiff.name}`}>{fileDiff.name}</div>
  ),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

function makeState(overrides: Partial<ReviewState> = {}): ReviewState {
  return {
    runId: "run-1",
    prId: "pr-1",
    workspaceId: "ws-1",
    status: "ready",
    prTitle: "Test PR",
    prNumber: 1,
    prUrl: "https://github.com/test/pr/1",
    diffText: `diff --git a/file.ts b/file.ts
index abc..def 100644
--- a/file.ts
+++ b/file.ts
@@ -1,3 +1,4 @@
 line1
+added line
 line2
 line3
`,
    changedFiles: ["file.ts"],
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
    ...overrides,
  };
}

function renderWithContext(
  state: ReviewState,
  props: { onRevealFinding?: (id: string) => void } = {},
) {
  const ctx: ReviewContextType = {
    state,
    setSelectedFile: vi.fn(),
    setSessionDecision: vi.fn(),
    refreshSnapshot: vi.fn().mockResolvedValue(undefined),
    revealFinding: vi.fn(),
  };
  return render(
    <ReviewContext.Provider value={ctx}>
      <DiffPanel {...props} />
    </ReviewContext.Provider>,
  );
}

describe("DiffPanel", () => {
  it("renders 'No diff available.' when diffText is null", () => {
    renderWithContext(makeState({ diffText: null }));
    expect(screen.getByText("No diff available.")).toBeInTheDocument();
  });

  it("always renders Pierre diff (no toggle UI)", () => {
    renderWithContext(makeState());
    expect(screen.getByTestId("pierre-file-file.ts")).toBeInTheDocument();
    expect(screen.queryByText("Use legacy diff")).not.toBeInTheDocument();
    expect(screen.queryByText("Use Pierre diff")).not.toBeInTheDocument();
  });

  it("falls back to legacy diff when PierreDiffPanel throws", async () => {
    const spy = vi.spyOn(console, "error").mockImplementation(() => {});

    vi.resetModules();

    vi.doMock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
    vi.doMock("@tauri-apps/api/event", () => ({
      listen: vi.fn(() => Promise.resolve(() => {})),
    }));
    vi.doMock("@pierre/diffs", () => ({
      parsePatchFiles: () => {
        throw new Error("Parse crash");
      },
    }));
    vi.doMock("@pierre/diffs/react", () => ({
      FileDiff: () => null,
    }));

    const { DiffPanel: FreshDiffPanel } = await import("../DiffPanel");
    const { ReviewContext: FreshContext } = await import("../../../lib/store");

    const state = makeState();
    const ctx = {
      state,
      setSelectedFile: vi.fn(),
      setSessionDecision: vi.fn(),
      refreshSnapshot: vi.fn().mockResolvedValue(undefined),
      revealFinding: vi.fn(),
    };
    render(
      <FreshContext.Provider value={ctx}>
        <FreshDiffPanel />
      </FreshContext.Provider>,
    );

    expect(screen.getByText("+added line")).toBeInTheDocument();
    spy.mockRestore();
  });
});
