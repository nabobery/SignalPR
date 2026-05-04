import { useState, useEffect, useRef } from "react";
import {
  AlertTriangle,
  ShieldAlert,
  Zap,
  Info,
  Sparkles,
  X,
  Check,
  Pencil,
  Wrench,
  ThumbsUp,
  Clock,
} from "lucide-react";
import { updateFinding, recordDecision } from "../../lib/ipc";
import type { Finding } from "../../lib/types";
import { FixPreview } from "./FixPreview";

const STANDARD_AGENT_TYPES = new Set(["security", "architecture", "performance"]);

const severityConfig: Record<string, { icon: typeof AlertTriangle; color: string; bg: string }> = {
  blocker: { icon: ShieldAlert, color: "text-red-400", bg: "bg-red-900/30" },
  critical: { icon: AlertTriangle, color: "text-orange-400", bg: "bg-orange-900/30" },
  warning: { icon: Zap, color: "text-yellow-400", bg: "bg-yellow-900/30" },
  info: { icon: Info, color: "text-blue-400", bg: "bg-blue-900/30" },
  nitpick: { icon: Sparkles, color: "text-zinc-400", bg: "bg-zinc-800/50" },
};

export function FindingCard({
  finding,
  onUpdated,
  focused = false,
  sessionDecision,
  onDecision,
}: {
  finding: Finding;
  onUpdated: () => void;
  focused?: boolean;
  sessionDecision?: string | null;
  onDecision?: (findingId: string, decision: string) => void;
}) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [editing, setEditing] = useState(false);
  const [editBody, setEditBody] = useState(finding.user_edited_body ?? finding.body);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (focused && cardRef.current) {
      cardRef.current.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }, [focused]);
  const [showFix, setShowFix] = useState(false);
  const [showEvidence, setShowEvidence] = useState(false);

  const hasPendingFix =
    finding.fix_status === "pending" && finding.fix_search !== null && finding.fix_replace !== null;
  const fixApplied = finding.fix_status === "applied" || finding.fix_status === "accepted";
  const fixRejected = finding.fix_status === "rejected";

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

  const handleRestore = async () => {
    await updateFinding(finding.id, undefined, undefined, "active");
    onUpdated();
  };

  const handleAccept = async () => {
    await recordDecision(finding.id, "accept");
    onDecision?.(finding.id, "accept");
  };

  const handleDefer = async () => {
    await recordDecision(finding.id, "skip");
    onDecision?.(finding.id, "skip");
  };

  if (finding.status === "suppressed") {
    return (
      <div
        ref={cardRef}
        className={`border rounded-lg p-3 bg-zinc-900/40 ${focused ? "border-blue-500 ring-1 ring-blue-500/50" : "border-zinc-800"}`}
      >
        <div className="flex items-start gap-2">
          <Info className="w-4 h-4 mt-0.5 shrink-0 text-zinc-500" />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="text-xs font-semibold uppercase text-zinc-500">suppressed</span>
              <span className="text-sm font-medium text-zinc-200 truncate">{finding.title}</span>
              <span className="text-xs text-zinc-600 ml-auto shrink-0">
                {Math.round(finding.confidence * 100)}%
              </span>
            </div>
            {finding.file_path && (
              <code className="text-xs text-zinc-500 block mb-2">{finding.file_path}</code>
            )}
            <p className="text-sm text-zinc-500 whitespace-pre-wrap">{displayBody}</p>
            <div className="flex gap-3 mt-2 items-center">
              <button
                onClick={handleRestore}
                className="flex items-center gap-1 text-xs text-zinc-300 hover:text-zinc-100"
              >
                <Check className="w-3 h-3" /> Restore
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      ref={cardRef}
      className={`border rounded-lg p-3 ${config.bg} ${focused ? "border-blue-500 ring-1 ring-blue-500/50" : "border-zinc-800"}`}
    >
      <div className="flex items-start gap-2">
        <Icon className={`w-4 h-4 mt-0.5 shrink-0 ${config.color}`} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className={`text-xs font-semibold uppercase ${config.color}`}>
              {effectiveSeverity}
            </span>
            {!STANDARD_AGENT_TYPES.has(finding.agent_type) && (
              <span className="bg-violet-900/40 text-violet-300 text-xs px-1.5 py-0.5 rounded capitalize">
                {finding.agent_type.replace(/_/g, " ")}
              </span>
            )}
            <span className="text-sm font-medium text-zinc-100 truncate">{finding.title}</span>
            {sessionDecision && (
              <span
                className={`text-xs px-1.5 py-0.5 rounded ${sessionDecision === "accept" ? "bg-emerald-900/30 text-emerald-400" : "bg-zinc-700 text-zinc-400"}`}
              >
                {sessionDecision === "accept" ? "Accepted" : "Deferred"}
              </span>
            )}
            <span className="text-xs text-zinc-500 ml-auto shrink-0">
              {Math.round(finding.confidence * 100)}%
            </span>
          </div>

          {/* Provenance chips */}
          {(finding.lane_id || finding.provider_name) && (
            <div className="flex gap-1.5 mb-1 flex-wrap">
              {finding.lane_id && (
                <span className="text-xs px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-400 capitalize">
                  {finding.lane_id}
                </span>
              )}
              {finding.provider_name && (
                <span className="text-xs px-1.5 py-0.5 rounded bg-zinc-800 text-zinc-400">
                  {finding.provider_name}
                </span>
              )}
            </div>
          )}

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
            <div className="flex gap-3 mt-2 items-center flex-wrap">
              {!sessionDecision && (
                <>
                  <button
                    onClick={handleAccept}
                    className="flex items-center gap-1 text-xs text-emerald-400 hover:text-emerald-300"
                  >
                    <ThumbsUp className="w-3 h-3" /> Accept
                  </button>
                  <button
                    onClick={handleDefer}
                    className="flex items-center gap-1 text-xs text-zinc-400 hover:text-zinc-200"
                  >
                    <Clock className="w-3 h-3" /> Defer
                  </button>
                </>
              )}
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
              {hasPendingFix && (
                <button
                  onClick={() => setShowFix((v) => !v)}
                  className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded bg-amber-900/30 text-amber-400 hover:bg-amber-900/50"
                >
                  <Wrench className="w-3 h-3" /> Fix available
                </button>
              )}
              {fixApplied && (
                <span className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded bg-emerald-900/30 text-emerald-400">
                  <Check className="w-3 h-3" /> Fix applied
                </span>
              )}
              {fixRejected && (
                <span className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded bg-zinc-700 text-zinc-500">
                  Fix rejected
                </span>
              )}
              {finding.evidence && (
                <button
                  onClick={() => setShowEvidence((v) => !v)}
                  className="text-xs text-zinc-400 hover:text-zinc-200"
                >
                  {showEvidence ? "Hide evidence" : "Evidence"}
                </button>
              )}
            </div>
          )}

          {showEvidence && finding.evidence && (
            <div className="mt-2 p-2 rounded bg-zinc-800/50 border border-zinc-700">
              <pre className="text-xs text-zinc-400 whitespace-pre-wrap break-words">
                {finding.evidence}
              </pre>
            </div>
          )}

          {showFix && hasPendingFix && (
            <FixPreview
              findingId={finding.id}
              fixSearch={finding.fix_search!}
              fixReplace={finding.fix_replace!}
              fixExplanation={finding.fix_explanation}
              onAccept={() => {
                setShowFix(false);
                onUpdated();
              }}
              onReject={() => {
                setShowFix(false);
                onUpdated();
              }}
            />
          )}
        </div>
      </div>
    </div>
  );
}
