import { FileText, ShieldAlert, AlertTriangle, Zap, Info } from "lucide-react";
import { useReviewContext } from "../../lib/store";

const severityIcon: Record<string, { icon: typeof FileText; color: string }> = {
  blocker: { icon: ShieldAlert, color: "text-[--color-sev-blocker]" },
  critical: { icon: AlertTriangle, color: "text-[--color-sev-critical]" },
  warning: { icon: Zap, color: "text-[--color-sev-warning]" },
  info: { icon: Info, color: "text-[--color-sev-info]" },
};

const severityOrder = ["blocker", "critical", "warning", "info", "nitpick"];

export function FileTree() {
  const { state, setSelectedFile } = useReviewContext();

  const fileSeverity = new Map<string, string>();
  for (const f of state.findings.filter((f) => f.status === "active" && f.file_path)) {
    const current = fileSeverity.get(f.file_path!);
    if (!current || severityOrder.indexOf(f.severity) < severityOrder.indexOf(current)) {
      fileSeverity.set(f.file_path!, f.severity);
    }
  }

  return (
    <div className="overflow-y-auto p-2 space-y-0.5 h-full">
      <div className="px-2 py-1 text-[11px] font-medium text-[--color-text-tertiary] uppercase tracking-wider">
        Files ({state.changedFiles.length})
      </div>
      <button
        onClick={() => setSelectedFile(null)}
        className={`w-full text-left px-2 py-1.5 rounded-md text-xs transition-colors ${
          state.selectedFile === null
            ? "bg-[--color-elevated] text-[--color-text-primary]"
            : "text-[--color-text-secondary] hover:bg-[--color-elevated]/60 hover:text-[--color-text-primary]"
        }`}
      >
        All files
      </button>
      {state.changedFiles.map((file) => {
        const sev = fileSeverity.get(file);
        const config = sev ? severityIcon[sev] : null;
        const Icon = config?.icon ?? FileText;
        const color = config?.color ?? "text-[--color-text-tertiary]";
        const isSelected = state.selectedFile === file;

        return (
          <button
            key={file}
            onClick={() => setSelectedFile(file)}
            className={`w-full text-left px-2 py-1.5 rounded-md text-xs flex items-center gap-2 transition-colors ${
              isSelected
                ? "bg-[--color-elevated] text-[--color-text-primary]"
                : "text-[--color-text-secondary] hover:bg-[--color-elevated]/60 hover:text-[--color-text-primary]"
            }`}
          >
            <Icon className={`w-3 h-3 shrink-0 ${color}`} />
            <span className="truncate font-mono">{file}</span>
          </button>
        );
      })}
    </div>
  );
}
