import { useEffect, useState } from "react";
import { CheckCircle, XCircle, AlertTriangle, Loader2 } from "lucide-react";
import { inspectEnvironment, parseError } from "../../lib/ipc";
import type { ToolStatus } from "../../lib/types";

const statusConfig = {
  ready: { icon: CheckCircle, color: "text-green-400", label: "Ready" },
  degraded: { icon: AlertTriangle, color: "text-yellow-400", label: "Degraded" },
  missing: { icon: XCircle, color: "text-red-400", label: "Missing" },
  unauthenticated: { icon: AlertTriangle, color: "text-yellow-400", label: "Not authenticated" },
} as const;

export function EnvironmentCheck({ onReady }: { onReady: (ready: boolean) => void }) {
  const [tools, setTools] = useState<ToolStatus[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    inspectEnvironment()
      .then((result) => {
        setTools(result);
        const ghReady = result.some((t) => t.tool_name === "gh" && t.status === "ready");
        onReady(ghReady);
      })
      .catch((err) => setError(parseError(err).message))
      .finally(() => setLoading(false));
  }, [onReady]);

  if (loading) {
    return (
      <div className="flex items-center gap-2 text-zinc-400 text-sm">
        <Loader2 className="w-4 h-4 animate-spin" />
        Checking environment...
      </div>
    );
  }

  if (error) {
    return <div className="text-red-400 text-sm">Environment check failed: {error}</div>;
  }

  return (
    <div className="space-y-2">
      {tools.map((tool) => {
        const config = statusConfig[tool.status] ?? statusConfig.missing;
        const Icon = config.icon;
        return (
          <div key={tool.tool_name} className="flex items-center gap-3 text-sm">
            <Icon className={`w-4 h-4 ${config.color}`} />
            <span className="font-medium text-zinc-200 w-16">{tool.tool_name}</span>
            <span className={config.color}>{config.label}</span>
            {tool.version && <span className="text-zinc-500">v{tool.version}</span>}
            {tool.message && (
              <code className="text-zinc-500 text-xs bg-zinc-800 px-2 py-0.5 rounded">
                {tool.message}
              </code>
            )}
          </div>
        );
      })}
    </div>
  );
}
