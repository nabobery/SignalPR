import { useState } from "react";
import { AlertTriangle, ShieldAlert, Zap, Info, Sparkles, X, Check, Pencil } from "lucide-react";
import { updateFinding } from "../../lib/ipc";
import type { Finding } from "../../lib/types";

const severityConfig: Record<string, { icon: typeof AlertTriangle; color: string; bg: string }> = {
  blocker: { icon: ShieldAlert, color: "text-red-400", bg: "bg-red-900/30" },
  critical: { icon: AlertTriangle, color: "text-orange-400", bg: "bg-orange-900/30" },
  warning: { icon: Zap, color: "text-yellow-400", bg: "bg-yellow-900/30" },
  info: { icon: Info, color: "text-blue-400", bg: "bg-blue-900/30" },
  nitpick: { icon: Sparkles, color: "text-zinc-400", bg: "bg-zinc-800/50" },
};

export function FindingCard({ finding, onUpdated }: { finding: Finding; onUpdated: () => void }) {
  const [editing, setEditing] = useState(false);
  const [editBody, setEditBody] = useState(finding.user_edited_body ?? finding.body);
  const [saving, setSaving] = useState(false);

  const effectiveSeverity = finding.user_severity_override ?? finding.severity;
  const config = severityConfig[effectiveSeverity] ?? severityConfig.info;
  const Icon = config.icon;
  const displayBody = finding.user_edited_body ?? finding.body;

  const handleSave = async () => {
    setSaving(true);
    try {
      await updateFinding(finding.id, editBody, undefined, undefined);
      setEditing(false);
      onUpdated();
    } finally {
      setSaving(false);
    }
  };

  const handleSuppress = async () => {
    await updateFinding(finding.id, undefined, undefined, "suppressed");
    onUpdated();
  };

  if (finding.status === "suppressed") return null;

  return (
    <div className={`border border-zinc-800 rounded-lg p-3 ${config.bg}`}>
      <div className="flex items-start gap-2">
        <Icon className={`w-4 h-4 mt-0.5 shrink-0 ${config.color}`} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className={`text-xs font-semibold uppercase ${config.color}`}>
              {effectiveSeverity}
            </span>
            <span className="text-sm font-medium text-zinc-100 truncate">{finding.title}</span>
            <span className="text-xs text-zinc-500 ml-auto shrink-0">
              {Math.round(finding.confidence * 100)}%
            </span>
          </div>

          {finding.file_path && (
            <code className="text-xs text-zinc-400 block mb-2">
              {finding.file_path}
              {finding.is_anchored && finding.line_start && (
                <>
                  :{finding.line_start}
                  {finding.line_end &&
                    finding.line_end !== finding.line_start &&
                    `-${finding.line_end}`}
                </>
              )}
            </code>
          )}

          {editing ? (
            <div className="space-y-2">
              <textarea
                value={editBody}
                onChange={(e) => setEditBody(e.target.value)}
                rows={3}
                className="w-full bg-zinc-800 border border-zinc-600 rounded px-2 py-1 text-sm text-zinc-200 focus:outline-none focus:border-zinc-400 resize-y"
              />
              <div className="flex gap-2">
                <button
                  onClick={handleSave}
                  disabled={saving}
                  className="flex items-center gap-1 text-xs text-emerald-400 hover:text-emerald-300"
                >
                  <Check className="w-3 h-3" /> Save
                </button>
                <button
                  onClick={() => {
                    setEditing(false);
                    setEditBody(displayBody);
                  }}
                  className="flex items-center gap-1 text-xs text-zinc-400 hover:text-zinc-300"
                >
                  <X className="w-3 h-3" /> Cancel
                </button>
              </div>
            </div>
          ) : (
            <p className="text-sm text-zinc-300 whitespace-pre-wrap">{displayBody}</p>
          )}

          {!editing && (
            <div className="flex gap-3 mt-2">
              <button
                onClick={() => setEditing(true)}
                className="flex items-center gap-1 text-xs text-zinc-400 hover:text-zinc-200"
              >
                <Pencil className="w-3 h-3" /> Edit
              </button>
              <button
                onClick={handleSuppress}
                className="flex items-center gap-1 text-xs text-zinc-400 hover:text-red-400"
              >
                <X className="w-3 h-3" /> Suppress
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
