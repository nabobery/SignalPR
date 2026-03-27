import { Shield, Layers, Gauge, Loader2, CheckCircle2, XCircle, Clock } from "lucide-react";
import type { LaneSnapshot } from "../../lib/types";

interface LaneProgressProps {
  lanes: LaneSnapshot[];
}

const LANE_ICONS: Record<string, typeof Shield> = {
  security: Shield,
  architecture: Layers,
  performance: Gauge,
};

function statusBadge(status: string) {
  switch (status) {
    case "pending":
      return { color: "text-zinc-400", bg: "bg-zinc-800", label: "Pending" };
    case "running":
      return { color: "text-blue-400", bg: "bg-blue-900/40", label: "Running" };
    case "completed":
      return { color: "text-green-400", bg: "bg-green-900/40", label: "Done" };
    case "failed":
      return { color: "text-red-400", bg: "bg-red-900/40", label: "Failed" };
    case "timed_out":
      return { color: "text-orange-400", bg: "bg-orange-900/40", label: "Timed Out" };
    case "cancelled":
      return { color: "text-zinc-400", bg: "bg-zinc-800", label: "Cancelled" };
    default:
      return { color: "text-zinc-400", bg: "bg-zinc-800", label: status };
  }
}

function StatusIcon({ status }: { status: string }) {
  switch (status) {
    case "running":
      return <Loader2 className="w-3.5 h-3.5 animate-spin text-blue-400" />;
    case "completed":
      return <CheckCircle2 className="w-3.5 h-3.5 text-green-400" />;
    case "failed":
      return <XCircle className="w-3.5 h-3.5 text-red-400" />;
    case "timed_out":
      return <Clock className="w-3.5 h-3.5 text-orange-400" />;
    default:
      return <div className="w-3.5 h-3.5 rounded-full bg-zinc-600" />;
  }
}

export default function LaneProgress({ lanes }: LaneProgressProps) {
  if (lanes.length === 0) return null;

  const completed = lanes.filter((l) => l.status === "completed").length;
  const total = lanes.length;

  return (
    <div className="flex flex-col gap-2 px-4 py-3 bg-zinc-900/50 rounded-lg border border-zinc-800">
      <div className="flex items-center justify-between">
        <span className="text-xs text-zinc-400 font-medium uppercase tracking-wide">
          Analysis Lanes
        </span>
        <span className="text-xs text-zinc-500">
          {completed} of {total} completed
        </span>
      </div>
      <div className="flex gap-2">
        {lanes.map((lane) => {
          const badge = statusBadge(lane.status);
          const Icon = LANE_ICONS[lane.lane_id] ?? Shield;
          return (
            <div
              key={lane.lane_id}
              className={`flex items-center gap-2 px-3 py-1.5 rounded-md ${badge.bg} border border-zinc-700/50`}
            >
              <Icon className={`w-3.5 h-3.5 ${badge.color}`} />
              <span className={`text-xs font-medium ${badge.color} capitalize`}>
                {lane.lane_id}
              </span>
              <StatusIcon status={lane.status} />
              {lane.status === "completed" && lane.finding_count > 0 && (
                <span className="text-xs text-zinc-400">{lane.finding_count}</span>
              )}
            </div>
          );
        })}
      </div>
      {lanes.some((l) => l.error_message) && (
        <div className="text-xs text-red-400/80 mt-1">
          {lanes
            .filter((l) => l.error_message)
            .map((l) => `${l.lane_id}: ${l.error_message}`)
            .join("; ")}
        </div>
      )}
    </div>
  );
}
