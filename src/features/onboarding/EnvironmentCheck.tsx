import { useCallback, useEffect, useState } from "react";
import { CheckCircle, XCircle, AlertTriangle, Loader2, RotateCw } from "lucide-react";
import { inspectEnvironment, parseError } from "../../lib/ipc";
import type { ToolStatus } from "../../lib/types";

const statusConfig = {
  ready: { icon: CheckCircle, color: "text-(--color-state-ready)", label: "Ready" },
  degraded: { icon: AlertTriangle, color: "text-(--color-sev-warning)", label: "Degraded" },
  incomplete: { icon: AlertTriangle, color: "text-(--color-sev-warning)", label: "Incomplete" },
  missing: { icon: XCircle, color: "text-(--color-sev-blocker)", label: "Missing" },
  unauthenticated: {
    icon: AlertTriangle,
    color: "text-(--color-sev-warning)",
    label: "Not authenticated",
  },
} as const;

export function EnvironmentCheck({ onReady }: { onReady: (ready: boolean) => void }) {
  const [tools, setTools] = useState<ToolStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const runCheck = useCallback(() => {
    setLoading(true);
    setError(null);
    inspectEnvironment()
      .then((result) => {
        setTools(result);
        const hasSubmitPath = result.some(
          (t) =>
            (t.tool_name === "gh" ||
              t.tool_name === "gitlab_token" ||
              t.tool_name === "bitbucket_token") &&
            t.status === "ready",
        );
        onReady(hasSubmitPath);
      })
      .catch((err) => setError(parseError(err).message))
      .finally(() => setLoading(false));
  }, [onReady]);

  useEffect(() => {
    runCheck();
  }, [runCheck]);

  if (loading && tools.length === 0) {
    return (
      <div className="flex items-center gap-2 text-(--color-text-secondary) text-sm">
        <Loader2 className="w-4 h-4 animate-spin" />
        Checking environment...
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex items-center gap-3 text-(--color-sev-blocker) text-sm">
        <span>Environment check failed: {error}</span>
        <button
          onClick={runCheck}
          className="flex items-center gap-1 text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
        >
          <RotateCw className="w-3 h-3" /> Retry
        </button>
      </div>
    );
  }

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-(--color-text-tertiary) text-xs uppercase tracking-wide">
          Environment
        </span>
        <button
          onClick={runCheck}
          disabled={loading}
          title="Re-check environment"
          className="flex items-center gap-1 text-xs text-(--color-text-tertiary) hover:text-(--color-text-primary) transition-colors disabled:opacity-40"
        >
          <RotateCw className={`w-3 h-3 ${loading ? "animate-spin" : ""}`} /> Re-check
        </button>
      </div>
      {tools.map((tool) => {
        const config = statusConfig[tool.status] ?? statusConfig.missing;
        const Icon = config.icon;
        return (
          <div key={tool.tool_name} className="flex items-center gap-3 text-sm">
            <Icon className={`w-4 h-4 ${config.color}`} />
            <span className="font-medium text-(--color-text-primary) w-16">{tool.tool_name}</span>
            <span className={config.color}>{config.label}</span>
            {tool.version && <span className="text-(--color-text-tertiary)">v{tool.version}</span>}
            {tool.message && (
              <code className="text-(--color-text-tertiary) text-xs bg-(--color-elevated) px-2 py-0.5 rounded">
                {tool.message}
              </code>
            )}
          </div>
        );
      })}
    </div>
  );
}
