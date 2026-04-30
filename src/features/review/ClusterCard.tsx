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
  blocker: { icon: AlertOctagon, color: "text-red-500" },
  critical: { icon: ShieldAlert, color: "text-orange-500" },
  warning: { icon: AlertTriangle, color: "text-yellow-500" },
  info: { icon: Info, color: "text-blue-400" },
  nitpick: { icon: Info, color: "text-zinc-400" },
};

function FindingSubItem({ finding }: { finding: Finding }) {
  const config = SEVERITY_CONFIG[finding.severity] ?? SEVERITY_CONFIG.info;
  const Icon = config.icon;

  return (
    <div className="flex items-start gap-2 py-2 px-3 bg-zinc-900/30 rounded border border-zinc-800/50">
      <Icon className={`w-3.5 h-3.5 mt-0.5 ${config.color} shrink-0`} />
      <div className="min-w-0">
        <p className="text-xs text-zinc-300">{finding.title}</p>
        {finding.file_path && (
          <p className="text-xs text-zinc-500 mt-0.5">
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

  useEffect(() => {
    if (focused && cardRef.current) {
      cardRef.current.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  }, [focused]);
  const [editBody, setEditBody] = useState(cluster.representative.body);
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
    // Suppress entire cluster by suppressing the representative
    await updateFinding(rep.id, undefined, undefined, "suppressed");
    onUpdate();
  };

  return (
    <div
      ref={cardRef}
      className={`bg-zinc-900 border rounded-lg overflow-hidden ${focused ? "border-blue-500 ring-1 ring-blue-500/50" : "border-zinc-800"}`}
    >
      {/* Header */}
      <div className="flex items-start gap-3 p-4">
        <Icon className={`w-4 h-4 mt-0.5 ${config.color} shrink-0`} />

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <h3 className="text-sm font-medium text-zinc-100 truncate">
              {rep.user_edited_body ? rep.title : rep.title}
            </h3>
            {isMulti && (
              <span className="flex items-center gap-1 px-1.5 py-0.5 bg-zinc-800 rounded text-xs text-zinc-400">
                <Layers className="w-3 h-3" />
                {cluster.member_count}
              </span>
            )}
          </div>

          <div className="flex items-center gap-2 mt-1">
            <span className={`text-xs font-medium ${config.color} capitalize`}>
              {rep.user_severity_override ?? rep.severity}
            </span>
            <span className="text-xs text-zinc-500">
              {Math.round(rep.confidence * 100)}% confidence
            </span>
            {rep.lane_id && <span className="text-xs text-zinc-600">via {rep.lane_id}</span>}
          </div>

          {rep.file_path && (
            <p className="text-xs text-zinc-500 mt-1 font-mono">
              {rep.file_path}
              {rep.line_start != null && `:${rep.line_start}`}
              {rep.line_end != null && rep.line_end !== rep.line_start && `-${rep.line_end}`}
            </p>
          )}

          {/* Body */}
          {editing ? (
            <div className="mt-2">
              <textarea
                className="w-full bg-zinc-800 border border-zinc-700 rounded px-2 py-1.5 text-xs text-zinc-200 resize-y min-h-[60px]"
                value={editBody}
                onChange={(e) => setEditBody(e.target.value)}
                rows={3}
              />
              <div className="flex gap-1 mt-1">
                <button
                  onClick={handleSave}
                  className="flex items-center gap-1 px-2 py-1 text-xs bg-blue-600 hover:bg-blue-500 rounded text-white"
                >
                  <Check className="w-3 h-3" /> Save
                </button>
                <button
                  onClick={() => {
                    setEditing(false);
                    setEditBody(rep.body);
                  }}
                  className="flex items-center gap-1 px-2 py-1 text-xs bg-zinc-700 hover:bg-zinc-600 rounded text-zinc-300"
                >
                  <X className="w-3 h-3" /> Cancel
                </button>
              </div>
            </div>
          ) : (
            <p className="text-xs text-zinc-400 mt-2 whitespace-pre-wrap">
              {rep.user_edited_body ?? rep.body}
            </p>
          )}
        </div>

        {/* Actions */}
        <div className="flex items-center gap-1 shrink-0">
          <button
            onClick={() => setEditing(true)}
            className="p-1 text-zinc-500 hover:text-zinc-300 rounded"
            title="Edit"
          >
            <Pencil className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleSuppress}
            className="p-1 text-zinc-500 hover:text-red-400 rounded"
            title="Suppress"
          >
            <EyeOff className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Expandable members */}
      {isMulti && (
        <div className="border-t border-zinc-800">
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1 px-4 py-2 text-xs text-zinc-500 hover:text-zinc-300 w-full"
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
