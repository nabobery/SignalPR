import { useState, useEffect, useRef } from "react";
import {
  ChevronDown,
  ChevronRight,
  AlertTriangle,
  AlertOctagon,
  Info,
  ShieldAlert,
  Pencil,
  EyeOff,
  Check,
  X,
  Layers,
} from "lucide-react";
import type { Finding } from "../../lib/types";
import { updateFinding } from "../../lib/ipc";

interface HydratedCluster {
  id: string;
  label: string | null;
  member_count: number;
  representative: Finding;
  members: Finding[];
}

interface ClusterCardProps {
  cluster: HydratedCluster;
  onUpdate: () => void;
  focused?: boolean;
}

const SEVERITY_CONFIG: Record<string, { icon: typeof AlertTriangle; color: string }> = {
  blocker: { icon: AlertOctagon, color: "text-[--color-sev-blocker]" },
  critical: { icon: ShieldAlert, color: "text-[--color-sev-critical]" },
  warning: { icon: AlertTriangle, color: "text-[--color-sev-warning]" },
  info: { icon: Info, color: "text-[--color-sev-info]" },
  nitpick: { icon: Info, color: "text-[--color-sev-nitpick]" },
};

function FindingSubItem({ finding }: { finding: Finding }) {
  const config = SEVERITY_CONFIG[finding.severity] ?? SEVERITY_CONFIG.info;
  const Icon = config.icon;

  return (
    <div className="flex items-start gap-2 py-2 px-3 bg-[--color-base] rounded-md border border-[--color-border-subtle]">
      <Icon className={`w-3.5 h-3.5 mt-0.5 shrink-0 ${config.color}`} />
      <div className="min-w-0">
        <p className="text-xs text-[--color-text-secondary]">{finding.title}</p>
        {finding.file_path && (
          <p className="text-[11px] text-[--color-text-tertiary] font-mono mt-0.5">
            {finding.file_path}
            {finding.line_start != null && `:${finding.line_start}`}
            {finding.line_end != null &&
              finding.line_end !== finding.line_start &&
              `-${finding.line_end}`}
          </p>
        )}
      </div>
    </div>
  );
}

export default function ClusterCard({ cluster, onUpdate, focused = false }: ClusterCardProps) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [editing, setEditing] = useState(false);
  const [editBody, setEditBody] = useState(cluster.representative.body);

  useEffect(() => {
    if (focused && cardRef.current) {
      cardRef.current.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }, [focused]);

  const rep = cluster.representative;
  const config = SEVERITY_CONFIG[rep.severity] ?? SEVERITY_CONFIG.info;
  const Icon = config.icon;
  const isMulti = cluster.member_count > 1;

  const handleSave = async () => {
    await updateFinding(rep.id, editBody, undefined, undefined);
    setEditing(false);
    onUpdate();
  };

  const handleSuppress = async () => {
    await updateFinding(rep.id, undefined, undefined, "suppressed");
    onUpdate();
  };

  return (
    <div
      ref={cardRef}
      className={`bg-[--color-surface] border rounded-lg overflow-hidden ${
        focused
          ? "border-[--color-accent] ring-1 ring-[--color-accent]/40"
          : "border-[--color-border-subtle]"
      }`}
    >
      {/* Header */}
      <div className="flex items-start gap-3 p-4">
        <Icon className={`w-4 h-4 mt-0.5 shrink-0 ${config.color}`} />

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-medium text-[--color-text-primary] truncate">
              {rep.title}
            </h3>
            {isMulti && (
              <span className="flex items-center gap-1 px-1.5 py-0.5 bg-[--color-elevated] rounded text-xs text-[--color-text-secondary]">
                <Layers className="w-3 h-3" />
                {cluster.member_count}
              </span>
            )}
          </div>

          <div className="flex items-center gap-2 mt-1">
            <span className={`text-xs font-medium capitalize ${config.color}`}>
              {rep.user_severity_override ?? rep.severity}
            </span>
            <span className="text-xs text-[--color-text-tertiary]">
              {Math.round(rep.confidence * 100)}%
            </span>
            {rep.lane_id && (
              <span className="text-xs text-[--color-text-tertiary]">via {rep.lane_id}</span>
            )}
          </div>

          {rep.file_path && (
            <p className="text-[11px] text-[--color-text-tertiary] font-mono mt-1">
              {rep.file_path}
              {rep.line_start != null && `:${rep.line_start}`}
              {rep.line_end != null && rep.line_end !== rep.line_start && `-${rep.line_end}`}
            </p>
          )}

          {editing ? (
            <div className="mt-2">
              <textarea
                className="w-full bg-[--color-elevated] border border-[--color-border] rounded px-2 py-1.5 text-xs text-[--color-text-primary] resize-y min-h-[60px] focus:outline-none focus:border-[--color-border-strong]"
                value={editBody}
                onChange={(e) => setEditBody(e.target.value)}
                rows={3}
              />
              <div className="flex gap-1 mt-1">
                <button
                  onClick={handleSave}
                  className="flex items-center gap-1 px-2 py-1 text-xs bg-[--color-accent] hover:bg-[--color-accent-hover] rounded text-white transition-colors"
                >
                  <Check className="w-3 h-3" /> Save
                </button>
                <button
                  onClick={() => {
                    setEditing(false);
                    setEditBody(rep.body);
                  }}
                  className="flex items-center gap-1 px-2 py-1 text-xs bg-[--color-elevated] hover:bg-[--color-overlay] rounded text-[--color-text-secondary] transition-colors"
                >
                  <X className="w-3 h-3" /> Cancel
                </button>
              </div>
            </div>
          ) : (
            <p className="text-xs text-[--color-text-secondary] mt-2 whitespace-pre-wrap">
              {rep.user_edited_body ?? rep.body}
            </p>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-1 shrink-0">
          <button
            onClick={() => setEditing(true)}
            className="p-1 text-[--color-text-tertiary] hover:text-[--color-text-secondary] rounded transition-colors"
            title="Edit"
          >
            <Pencil className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleSuppress}
            className="p-1 text-[--color-text-tertiary] hover:text-[--color-sev-blocker] rounded transition-colors"
            title="Suppress"
          >
            <EyeOff className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Expandable members */}
      {isMulti && (
        <div className="border-t border-[--color-border-subtle]">
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1 px-4 py-2 text-xs text-[--color-text-tertiary] hover:text-[--color-text-secondary] w-full transition-colors"
          >
            {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            {expanded ? "Hide" : "Show"} {cluster.member_count} member findings
          </button>
          {expanded && (
            <div className="flex flex-col gap-1 px-4 pb-3">
              {cluster.members.map((m) => (
                <FindingSubItem key={m.id} finding={m} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
