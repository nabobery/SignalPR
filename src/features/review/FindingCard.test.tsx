import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

vi.mock("./FixPreview", () => ({
  FixPreview: () => <div>FixPreview</div>,
}));

import { FindingCard } from "./FindingCard";
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
    ...overrides,
  };
}

describe("FindingCard", () => {
  const onUpdated = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders severity badge, title, and body", () => {
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);

    expect(screen.getByText("warning")).toBeInTheDocument();
    expect(screen.getByText("Unused variable")).toBeInTheDocument();
    expect(screen.getByText("The variable `x` is never read.")).toBeInTheDocument();
  });

  it("displays confidence percentage", () => {
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);
    expect(screen.getByText("85%")).toBeInTheDocument();
  });

  it("displays file path with line range", () => {
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);
    expect(screen.getByText(/src\/main\.rs/)).toBeInTheDocument();
    expect(screen.getByText(/10-15/)).toBeInTheDocument();
  });

  it("renders different severity levels correctly", () => {
    render(<FindingCard finding={makeFinding({ severity: "blocker" })} onUpdated={onUpdated} />);
    expect(screen.getByText("blocker")).toBeInTheDocument();
  });

  it("uses user_severity_override when present", () => {
    render(
      <FindingCard
        finding={makeFinding({
          severity: "warning",
          user_severity_override: "critical",
        })}
        onUpdated={onUpdated}
      />,
    );
    expect(screen.getByText("critical")).toBeInTheDocument();
    expect(screen.queryByText("warning")).not.toBeInTheDocument();
  });

  it("renders suppressed findings with restore action", () => {
    render(<FindingCard finding={makeFinding({ status: "suppressed" })} onUpdated={onUpdated} />);
    expect(screen.getByText("suppressed")).toBeInTheDocument();
    expect(screen.getByText("Restore")).toBeInTheDocument();
  });

  it("calls updateFinding with active status when Restore is clicked", async () => {
    const { updateFinding } = await import("../../lib/ipc");
    const user = userEvent.setup();
    render(<FindingCard finding={makeFinding({ status: "suppressed" })} onUpdated={onUpdated} />);

    await user.click(screen.getByText("Restore"));

    expect(updateFinding).toHaveBeenCalledWith("f-1", undefined, undefined, "active");
    expect(onUpdated).toHaveBeenCalled();
  });

  it("shows edit textarea when Edit is clicked", async () => {
    const user = userEvent.setup();
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);

    await user.click(screen.getByText("Edit"));
    expect(screen.getByRole("textbox")).toBeInTheDocument();
  });

  it("cancels editing and hides textarea", async () => {
    const user = userEvent.setup();
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);

    await user.click(screen.getByText("Edit"));
    expect(screen.getByRole("textbox")).toBeInTheDocument();

    await user.click(screen.getByText("Cancel"));
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
  });

  it("calls updateFinding and onUpdated when Save is clicked", async () => {
    const { updateFinding } = await import("../../lib/ipc");
    const user = userEvent.setup();
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);

    await user.click(screen.getByText("Edit"));
    const textarea = screen.getByRole("textbox");
    await user.clear(textarea);
    await user.type(textarea, "Updated body");
    await user.click(screen.getByText("Save"));

    expect(updateFinding).toHaveBeenCalledWith("f-1", "Updated body", undefined, undefined);
    expect(onUpdated).toHaveBeenCalled();
  });

  it("calls updateFinding with suppressed status when Suppress is clicked", async () => {
    const { updateFinding } = await import("../../lib/ipc");
    const user = userEvent.setup();
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} />);

    await user.click(screen.getByText("Suppress"));

    expect(updateFinding).toHaveBeenCalledWith("f-1", undefined, undefined, "suppressed");
    expect(onUpdated).toHaveBeenCalled();
  });

  it("displays user_edited_body when present", () => {
    render(
      <FindingCard
        finding={makeFinding({ user_edited_body: "Custom body text" })}
        onUpdated={onUpdated}
      />,
    );
    expect(screen.getByText("Custom body text")).toBeInTheDocument();
  });

  it("shows fix affordance when pending fix exists", () => {
    render(
      <FindingCard
        finding={makeFinding({
          fix_search: "old",
          fix_replace: "new",
          fix_status: "pending",
        })}
        onUpdated={onUpdated}
      />,
    );
    expect(screen.getByText("Fix available")).toBeInTheDocument();
  });

  it("does not show fix affordance when fix_replace is missing", () => {
    render(
      <FindingCard
        finding={makeFinding({
          fix_search: "old",
          fix_replace: null,
          fix_status: "pending",
        })}
        onUpdated={onUpdated}
      />,
    );
    expect(screen.queryByText("Fix available")).not.toBeInTheDocument();
  });

  it("renders FixPreview when fix affordance is toggled", async () => {
    const user = userEvent.setup();
    render(
      <FindingCard
        finding={makeFinding({
          fix_search: "old",
          fix_replace: "new",
          fix_status: "pending",
        })}
        onUpdated={onUpdated}
      />,
    );

    await user.click(screen.getByText("Fix available"));
    expect(screen.getByText("FixPreview")).toBeInTheDocument();
  });

  it("renders provenance chips when lane_id and provider_name are set", () => {
    render(
      <FindingCard
        finding={makeFinding({ lane_id: "security", provider_name: "codex" })}
        onUpdated={onUpdated}
      />,
    );
    expect(screen.getByText("security")).toBeInTheDocument();
    expect(screen.getByText("codex")).toBeInTheDocument();
  });

  it("renders evidence toggle and content when evidence is present", async () => {
    const user = userEvent.setup();
    render(
      <FindingCard finding={makeFinding({ evidence: "Stack trace here" })} onUpdated={onUpdated} />,
    );

    expect(screen.getByText("Evidence")).toBeInTheDocument();
    expect(screen.queryByText("Stack trace here")).not.toBeInTheDocument();

    await user.click(screen.getByText("Evidence"));
    expect(screen.getByText("Stack trace here")).toBeInTheDocument();
  });

  it("calls recordDecision on Accept and triggers onDecision", async () => {
    const { recordDecision } = await import("../../lib/ipc");
    const onDecision = vi.fn();
    const user = userEvent.setup();

    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} onDecision={onDecision} />);

    await user.click(screen.getByText("Accept"));
    expect(recordDecision).toHaveBeenCalledWith("f-1", "accept");
    expect(onDecision).toHaveBeenCalledWith("f-1", "accept");
  });

  it("calls recordDecision on Defer and triggers onDecision", async () => {
    const { recordDecision } = await import("../../lib/ipc");
    const onDecision = vi.fn();
    const user = userEvent.setup();

    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} onDecision={onDecision} />);

    await user.click(screen.getByText("Defer"));
    expect(recordDecision).toHaveBeenCalledWith("f-1", "skip");
    expect(onDecision).toHaveBeenCalledWith("f-1", "skip");
  });

  it("hides Accept/Defer when sessionDecision is set", () => {
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} sessionDecision="accept" />);

    expect(screen.queryByText("Accept")).not.toBeInTheDocument();
    expect(screen.queryByText("Defer")).not.toBeInTheDocument();
    expect(screen.getByText("Accepted")).toBeInTheDocument();
  });

  it("shows Deferred badge when sessionDecision is skip", () => {
    render(<FindingCard finding={makeFinding()} onUpdated={onUpdated} sessionDecision="skip" />);

    expect(screen.queryByText("Accept")).not.toBeInTheDocument();
    expect(screen.getByText("Deferred")).toBeInTheDocument();
  });
});
