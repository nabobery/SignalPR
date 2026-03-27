import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { FindingCard } from "./FindingCard";
import type { Finding } from "../../lib/types";

vi.mock("../../lib/ipc", () => ({
  updateFinding: vi.fn(() => Promise.resolve()),
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

  it("returns null for suppressed findings", () => {
    const { container } = render(
      <FindingCard finding={makeFinding({ status: "suppressed" })} onUpdated={onUpdated} />,
    );
    expect(container.innerHTML).toBe("");
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
});
