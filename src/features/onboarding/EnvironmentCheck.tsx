import { useEffect, useState } from "react";
import { CheckCircle, XCircle, AlertTriangle, Loader2 } from "lucide-react";
import { inspectEnvironment, parseError } from "../../lib/ipc";
import type { ToolStatus } from "../../lib/types";

const statusConfig = {
  ready: { icon: CheckCircle, color: "text-[--color-state-ready]", label: "Ready" },
  degraded: { icon: AlertTriangle, color: "text-[--color-sev-warning]", label: "Degraded" },
  incomplete: { icon: AlertTriangle, color: "text-[--color-sev-warning]", label: "Incomplete" },
  missing: { icon: XCircle, color: "text-[--color-sev-blocker]", label: "Missing" },
  unauthenticated: {
    icon: AlertTriangle,
    color: "text-[--color-sev-warning]",
    label: "Not authenticated",
  },
} as const;

export function EnvironmentCheck({ onReady }: { onReady: (ready: boolean) => void }) {
  const [tools, setTools] = useState<ToolStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
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

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-[--color-text-secondary] text-sm">
        <Loader2 className="w-4 h-4 animate-spin" />
        Checking environment...
      </div>
    );
  }

  if (error) {
    return (
      <div className="text-[--color-sev-blocker] text-sm">Environment check failed: {error}</div>
    );
  }

  return (
    <div className="space-y-2">
      {tools.map((tool) => {
        const config = statusConfig[tool.status] ?? statusConfig.missing;
        const Icon = config.icon;
        return (
          <div key={tool.tool_name} className="flex items-center gap-3 text-sm">
            <Icon className={`w-4 h-4 ${config.color}`} />
            <span className="font-medium text-[--color-text-primary] w-16">{tool.tool_name}</span>
            <span className={config.color}>{config.label}</span>
            {tool.version && <span className="text-[--color-text-tertiary]">v{tool.version}</span>}
            {tool.message && (
              <code className="text-[--color-text-tertiary] text-xs bg-[--color-elevated] px-2 py-0.5 rounded">
                {tool.message}
              </code>
            )}
          </div>
        );
      })}
    </div>
  );
}
