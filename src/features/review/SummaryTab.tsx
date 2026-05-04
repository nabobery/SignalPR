import { useMemo } from "react";
import {
  AlertTriangle,
  ShieldAlert,
  Zap,
  Info,
  Sparkles,
  FileWarning,
  CheckCircle,
  XCircle,
  Loader2,
} from "lucide-react";
import { useReviewContext } from "../../lib/store";

const severityIcons: Record<string, typeof AlertTriangle> = {
  blocker: ShieldAlert,
  critical: AlertTriangle,
  warning: Zap,
  info: Info,
  nitpick: Sparkles,
};

const severityColors: Record<string, string> = {
  blocker: "text-red-400",
  critical: "text-orange-400",
  warning: "text-yellow-400",
  info: "text-blue-400",
  nitpick: "text-zinc-400",
};

export function SummaryTab() {
  const { state, setSelectedFile } = useReviewContext();
  const activeFindings = state.findings.filter((f) => f.status === "active");

  const severityBreakdown = useMemo(() => {
    const counts: Record<string, number> = {};
    for (const f of activeFindings) {
      const sev = f.user_severity_override ?? f.severity;
      counts[sev] = (counts[sev] ?? 0) + 1;
    }
    return counts;
  }, [activeFindings]);

  const hotspots = useMemo(() => {
    const fileCounts: Record<string, number> = {};
    for (const f of activeFindings) {
      if (f.file_path) {
        fileCounts[f.file_path] = (fileCounts[f.file_path] ?? 0) + 1;
      }
    }
    return Object.entries(fileCounts)
      .sort(([, a], [, b]) => b - a)
      .slice(0, 8);
  }, [activeFindings]);

  const hasBlocker = "blocker" in severityBreakdown;
  const hasCritical = "critical" in severityBreakdown;
  const isRunning =
    state.status === "created" || state.status === "running_agents" || state.status === "cleaning";
  const completedLanes = state.laneStatuses.filter((l) => l.status === "completed").length;
  const failedLanes = state.laneStatuses.filter(
    (l) => l.status === "failed" || l.status === "timed_out",
  ).length;

  return (
    <div className="overflow-y-auto p-4 space-y-5">
      {/* Status + risk */}
      <div className="flex items-center gap-3 flex-wrap">
        {isRunning && (
          <span className="flex items-center gap-1.5 text-xs text-yellow-400 bg-yellow-900/20 px-2 py-1 rounded">
            <Loader2 className="w-3 h-3 animate-spin" />
            {state.status === "running_agents" ? "Analyzing..." : "Cleaning..."}
          </span>
        )}
        {state.status === "ready" && (
          <span className="flex items-center gap-1.5 text-xs text-emerald-400 bg-emerald-900/20 px-2 py-1 rounded">
            <CheckCircle className="w-3 h-3" /> Review ready
          </span>
        )}
        {state.status === "submitted" && (
          <span className="flex items-center gap-1.5 text-xs text-blue-400 bg-blue-900/20 px-2 py-1 rounded">
            <CheckCircle className="w-3 h-3" /> Submitted
          </span>
        )}
        {state.status === "failed" && (
          <span className="flex items-center gap-1.5 text-xs text-red-400 bg-red-900/20 px-2 py-1 rounded">
            <XCircle className="w-3 h-3" /> Failed
          </span>
        )}
        {(hasBlocker || hasCritical) && (
          <span className="flex items-center gap-1.5 text-xs text-red-400 bg-red-900/20 px-2 py-1 rounded">
            <ShieldAlert className="w-3 h-3" /> High risk
          </span>
        )}
      </div>

      {/* Stats row */}
      <div className="grid grid-cols-3 gap-3">
        <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
          <div className="text-2xl font-bold text-zinc-100">{state.changedFiles.length}</div>
          <div className="text-xs text-zinc-500">files changed</div>
        </div>
        <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
          <div className="text-2xl font-bold text-zinc-100">{activeFindings.length}</div>
          <div className="text-xs text-zinc-500">active findings</div>
        </div>
        <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-3">
          <div className="text-2xl font-bold text-zinc-100">
            {completedLanes}/{state.laneStatuses.length}
          </div>
          <div className="text-xs text-zinc-500">
            lanes {failedLanes > 0 ? `(${failedLanes} failed)` : "completed"}
          </div>
        </div>
      </div>

      {/* Lane statuses */}
      {state.laneStatuses.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Analysis lanes
          </h3>
          <div className="space-y-1.5">
            {state.laneStatuses.map((lane) => (
              <div
                key={lane.lane_id}
                className="flex items-center gap-2 px-3 py-2 rounded-lg bg-zinc-900/50 border border-zinc-800/50"
              >
                {lane.status === "completed" && (
                  <CheckCircle className="w-3.5 h-3.5 text-emerald-400" />
                )}
                {(lane.status === "failed" || lane.status === "timed_out") && (
                  <XCircle className="w-3.5 h-3.5 text-red-400" />
                )}
                {lane.status !== "completed" &&
                  lane.status !== "failed" &&
                  lane.status !== "timed_out" && (
                    <Loader2 className="w-3.5 h-3.5 text-yellow-400 animate-spin" />
                  )}
                <span className="text-sm text-zinc-200 capitalize">{lane.lane_id}</span>
                <span className="text-xs text-zinc-500">{lane.provider_name}</span>
                {lane.finding_count > 0 && (
                  <span className="text-xs text-zinc-400 ml-auto">
                    {lane.finding_count} finding{lane.finding_count !== 1 ? "s" : ""}
                  </span>
                )}
                {lane.error_message && (
                  <span className="text-xs text-red-400 truncate ml-auto max-w-48">
                    {lane.error_message}
                  </span>
                )}
              </div>
            ))}
          </div>
        </section>
      )}

      {/* Severity breakdown */}
      {activeFindings.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Severity breakdown
          </h3>
          <div className="flex gap-3 flex-wrap">
            {["blocker", "critical", "warning", "info", "nitpick"].map((sev) => {
              const count = severityBreakdown[sev];
              if (!count) return null;
              const Icon = severityIcons[sev] ?? Info;
              const color = severityColors[sev] ?? "text-zinc-400";
              return (
                <div
                  key={sev}
                  className="flex items-center gap-1.5 bg-zinc-900/50 border border-zinc-800/50 rounded px-2.5 py-1.5"
                >
                  <Icon className={`w-3.5 h-3.5 ${color}`} />
                  <span className={`text-sm font-medium ${color}`}>{count}</span>
                  <span className="text-xs text-zinc-500 capitalize">{sev}</span>
                </div>
              );
            })}
          </div>
        </section>
      )}

      {/* Hotspots */}
      {hotspots.length > 0 && (
        <section>
          <h3 className="text-xs font-medium text-zinc-500 uppercase tracking-wider mb-2">
            Hotspots
          </h3>
          <div className="space-y-1">
            {hotspots.map(([file, count]) => (
              <button
                key={file}
                onClick={() => setSelectedFile(file)}
                className="flex items-center gap-2 w-full text-left px-3 py-2 rounded-lg bg-zinc-900/50 border border-zinc-800/50 hover:border-zinc-700 transition-colors"
              >
                <FileWarning className="w-3.5 h-3.5 text-yellow-400 shrink-0" />
                <code className="text-xs text-zinc-300 truncate flex-1">{file}</code>
                <span className="text-xs text-zinc-500 shrink-0">
                  {count} finding{count !== 1 ? "s" : ""}
                </span>
              </button>
            ))}
          </div>
        </section>
      )}
    </div>
  );
}
