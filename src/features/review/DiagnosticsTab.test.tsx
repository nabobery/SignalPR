import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DiagnosticsTab } from "./DiagnosticsTab";

vi.mock("../../lib/ipc", () => ({
  getEventLog: vi.fn(),
  parseError: (err: unknown) => ({ code: "unknown", message: String(err) }),
}));

describe("DiagnosticsTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("renders event entries after loading", async () => {
    const { getEventLog } = await import("../../lib/ipc");
    (getEventLog as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        timestamp: "2026-05-01T10:00:00Z",
        event_type: "lane_call_started",
        payload: { lane_id: "security", provider: "codex" },
      },
      {
        timestamp: "2026-05-01T10:00:05Z",
        event_type: "lane_call_completed",
        payload: { lane_id: "security", duration_ms: 5000 },
      },
    ]);

    render(<DiagnosticsTab runId="run-1" />);

    expect(await screen.findByText("lane_call_started")).toBeInTheDocument();
    expect(screen.getByText("lane_call_completed")).toBeInTheDocument();
    expect(screen.getByText("(2 events)")).toBeInTheDocument();
  });

  it("shows loading state initially", async () => {
    const { getEventLog } = await import("../../lib/ipc");
    (getEventLog as ReturnType<typeof vi.fn>).mockReturnValue(new Promise(() => {}));

    render(<DiagnosticsTab runId="run-1" />);
    expect(document.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("shows error when event log fetch fails", async () => {
    const { getEventLog } = await import("../../lib/ipc");
    (getEventLog as ReturnType<typeof vi.fn>).mockRejectedValue("Network error");

    render(<DiagnosticsTab runId="run-1" />);
    expect(await screen.findByText("Network error")).toBeInTheDocument();
  });

  it("filters events by event_type", async () => {
    const { getEventLog } = await import("../../lib/ipc");
    (getEventLog as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        timestamp: "2026-05-01T10:00:00Z",
        event_type: "lane_call_started",
        payload: {},
      },
      {
        timestamp: "2026-05-01T10:00:05Z",
        event_type: "review_ready",
        payload: { surfaced_count: 3 },
      },
    ]);

    const user = userEvent.setup();
    render(<DiagnosticsTab runId="run-1" />);

    await screen.findByText("lane_call_started");

    const filter = screen.getByPlaceholderText(/filter/i);
    await user.type(filter, "ready");

    expect(screen.queryByText("lane_call_started")).not.toBeInTheDocument();
    expect(screen.getByText("review_ready")).toBeInTheDocument();
    expect(screen.getByText("(1 events)")).toBeInTheDocument();
  });

  it("shows empty state when no events exist", async () => {
    const { getEventLog } = await import("../../lib/ipc");
    (getEventLog as ReturnType<typeof vi.fn>).mockResolvedValue([]);

    render(<DiagnosticsTab runId="run-1" />);
    expect(await screen.findByText("No events recorded for this run.")).toBeInTheDocument();
  });

  it("expands event payload on click", async () => {
    const { getEventLog } = await import("../../lib/ipc");
    (getEventLog as ReturnType<typeof vi.fn>).mockResolvedValue([
      {
        timestamp: "2026-05-01T10:00:00Z",
        event_type: "lane_call_completed",
        payload: { duration_ms: 5000, lane_id: "security" },
      },
    ]);

    const user = userEvent.setup();
    render(<DiagnosticsTab runId="run-1" />);

    await screen.findByText("lane_call_completed");
    await user.click(screen.getByText("lane_call_completed"));

    expect(screen.getByText(/"duration_ms": 5000/)).toBeInTheDocument();
  });
});
