import {
  Shield,
  Layers,
  Gauge,
  Sparkles,
  Loader2,
  CheckCircle2,
  XCircle,
  Clock,
} from "lucide-react";
import type { LaneSnapshot } from "../../lib/types";
import { StreamingActivity } from "./StreamingActivity";

interface LaneProgressProps {
  lanes: LaneSnapshot[];
}

const LANE_ICONS: Record<string, typeof Shield> = {
  security: Shield,
  architecture: Layers,
  performance: Gauge,
};

function laneStatusStyle(status: string): { color: string; bg: string; label: string } {
  switch (status) {
    case "pending":
      return {
        color: "text-(--color-text-tertiary)",
        bg: "bg-(--color-elevated)",
        label: "Pending",
      };
    case "running":
      return { color: "text-(--color-sev-info)", bg: "bg-(--color-sev-info-bg)", label: "Running" };
    case "completed":
      return {
        color: "text-(--color-state-ready)",
        bg: "bg-(--color-state-ready-bg)",
        label: "Done",
      };
    case "failed":
      return {
        color: "text-(--color-sev-blocker)",
        bg: "bg-(--color-sev-blocker-bg)",
        label: "Failed",
      };
    case "timed_out":
      return {
        color: "text-(--color-sev-critical)",
        bg: "bg-(--color-sev-critical-bg)",
        label: "Timed Out",
      };
    case "cancelled":
      return {
        color: "text-(--color-text-tertiary)",
        bg: "bg-(--color-elevated)",
        label: "Cancelled",
      };
    default:
      return { color: "text-(--color-text-tertiary)", bg: "bg-(--color-elevated)", label: status };
  }
}

function StatusIcon({ status }: { status: string }) {
  switch (status) {
    case "running":
      return <Loader2 className="w-3.5 h-3.5 animate-spin text-(--color-sev-info)" />;
    case "completed":
      return <CheckCircle2 className="w-3.5 h-3.5 text-(--color-state-ready)" />;
    case "failed":
      return <XCircle className="w-3.5 h-3.5 text-(--color-sev-blocker)" />;
    case "timed_out":
      return <Clock className="w-3.5 h-3.5 text-(--color-sev-critical)" />;
    default:
      return <div className="w-3.5 h-3.5 rounded-full bg-(--color-border)" />;
  }
}

export default function LaneProgress({ lanes }: LaneProgressProps) {
  if (lanes.length === 0) return null;

  const completed = lanes.filter((l) => l.status === "completed").length;
  const total = lanes.length;

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between">
        <span className="text-[11px] font-medium uppercase tracking-wider text-(--color-text-tertiary)">
          Analysis Lanes
        </span>
        <span className="text-[11px] text-(--color-text-tertiary)">
          {completed}/{total}
        </span>
      </div>
      <div className="flex gap-2 flex-wrap">
        {lanes.map((lane) => {
          const style = laneStatusStyle(lane.status);
          const Icon = LANE_ICONS[lane.lane_id] ?? Sparkles;
          return (
            <div
              key={lane.lane_id}
              className={`flex flex-col gap-0.5 px-3 py-1.5 rounded-md border border-(--color-border-subtle) min-w-0 ${style.bg}`}
            >
              <div className="flex items-center gap-2 min-w-0">
                <Icon className={`w-3.5 h-3.5 shrink-0 ${style.color}`} />
                <span className={`text-xs font-medium capitalize truncate ${style.color}`}>
                  {lane.lane_id.replace(/_/g, " ")}
                </span>
                <StatusIcon status={lane.status} />
                {lane.status === "completed" && lane.finding_count > 0 && (
                  <span className="text-[11px] text-(--color-text-tertiary)">
                    {lane.finding_count}
                  </span>
                )}
              </div>
              {lane.status === "running" && <StreamingActivity laneId={lane.lane_id} />}
            </div>
          );
        })}
      </div>
      {lanes.some((l) => l.error_message) && (
        <div className="text-[11px] text-(--color-sev-blocker)">
          {lanes
            .filter((l) => l.error_message)
            .map((l) => `${l.lane_id}: ${l.error_message}`)
            .join("; ")}
        </div>
      )}
    </div>
  );
}
