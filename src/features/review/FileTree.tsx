import { FileText, ShieldAlert, AlertTriangle, Zap, Info } from "lucide-react";
import { useReviewContext } from "../../lib/store";

const severityIcon: Record<string, { icon: typeof FileText; color: string }> = {
  blocker: { icon: ShieldAlert, color: "text-red-400" },
  critical: { icon: AlertTriangle, color: "text-orange-400" },
  warning: { icon: Zap, color: "text-yellow-400" },
  info: { icon: Info, color: "text-blue-400" },
};

const severityOrder = ["blocker", "critical", "warning", "info", "nitpick"];

export function FileTree() {
  const { state, setSelectedFile } = useReviewContext();

  // Build file → highest severity map
  const fileSeverity = new Map<string, string>();
  for (const f of state.findings.filter((f) => f.status === "active" && f.file_path)) {
    const current = fileSeverity.get(f.file_path!);
    if (!current || severityOrder.indexOf(f.severity) < severityOrder.indexOf(current)) {
      fileSeverity.set(f.file_path!, f.severity);
    }
  }

  return (
    <div className="overflow-y-auto p-3 space-y-0.5">
      <div className="text-xs text-zinc-400 mb-2 font-medium">
        Changed Files ({state.changedFiles.length})
      </div>
      <button
        onClick={() => setSelectedFile(null)}
        className={`w-full text-left px-2 py-1 rounded text-xs hover:bg-zinc-800 ${
          state.selectedFile === null ? "bg-zinc-800 text-zinc-100" : "text-zinc-400"
        }`}
      >
        All files
      </button>
      {state.changedFiles.map((file) => {
        const sev = fileSeverity.get(file);
        const config = sev ? severityIcon[sev] : null;
        const Icon = config?.icon ?? FileText;
        const color = config?.color ?? "text-zinc-500";
        const isSelected = state.selectedFile === file;

        return (
          <button
            key={file}
            onClick={() => setSelectedFile(file)}
            className={`w-full text-left px-2 py-1 rounded text-xs flex items-center gap-2 hover:bg-zinc-800 ${
              isSelected ? "bg-zinc-800 text-zinc-100" : "text-zinc-400"
            }`}
          >
            <Icon className={`w-3 h-3 shrink-0 ${color}`} />
            <span className="truncate">{file}</span>
          </button>
        );
      })}
    </div>
  );
}
