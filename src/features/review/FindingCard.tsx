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
  HelpCircle,
} from "lucide-react";
import { updateFinding, recordDecision } from "../../lib/ipc";
import type { Finding, PlatformMetadata } from "../../lib/types";
import { buildFindingTrustViewModel } from "../../lib/trust";
import { FixPreview } from "./FixPreview";
import { severityBadge } from "../../ui/badge";

const STANDARD_AGENT_TYPES = new Set(["security", "architecture", "performance"]);

const severityIcons: Record<string, typeof AlertTriangle> = {
  blocker: ShieldAlert,
  critical: AlertTriangle,
  warning: Zap,
  info: Info,
  nitpick: Sparkles,
};

const severityConfig: Record<string, { icon: typeof AlertTriangle; color: string; bg: string }> = {
  blocker: {
    icon: severityIcons.blocker,
    color: severityBadge("blocker").text,
    bg: severityBadge("blocker").bg,
  },
  critical: {
    icon: severityIcons.critical,
    color: severityBadge("critical").text,
    bg: severityBadge("critical").bg,
  },
  warning: {
    icon: severityIcons.warning,
    color: severityBadge("warning").text,
    bg: severityBadge("warning").bg,
  },
  info: {
    icon: severityIcons.info,
    color: severityBadge("info").text,
    bg: severityBadge("info").bg,
  },
  nitpick: {
    icon: severityIcons.nitpick,
    color: severityBadge("nitpick").text,
    bg: severityBadge("nitpick").bg,
  },
};

export function FindingCard({
  finding,
  onUpdated,
  focused = false,
  sessionDecision,
  onDecision,
  platformMetadata = null,
}: {
  finding: Finding;
  onUpdated: () => void;
  focused?: boolean;
  sessionDecision?: string | null;
  onDecision?: (findingId: string, decision: string) => void;
  platformMetadata?: PlatformMetadata | null;
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
  const [showWhy, setShowWhy] = useState(false);

  const hasPendingFix =
    finding.fix_status === "pending" && finding.fix_search !== null && finding.fix_replace !== null;
  const fixApplied = finding.fix_status === "applied" || finding.fix_status === "accepted";
  const fixRejected = finding.fix_status === "rejected";

  const effectiveSeverity = finding.user_severity_override ?? finding.severity;
  const config = severityConfig[effectiveSeverity] ?? severityConfig.info;
  const Icon = config.icon;
  const displayBody = finding.user_edited_body ?? finding.body;
  const trust = buildFindingTrustViewModel(finding, { platformMetadata });

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
        className={`border rounded-lg p-3 bg-(--color-surface) ${focused ? "border-(--color-sev-info) ring-1 ring-(--color-sev-info)/40" : "border-(--color-border-subtle)"}`}
      >
        <div className="flex items-start gap-2">
          <Info className="w-4 h-4 mt-0.5 shrink-0 text-(--color-text-tertiary)" />
          <div className="flex-1 min-w-0">
            <div className="flex items-center gap-2 mb-1">
              <span className="text-xs font-semibold uppercase text-(--color-text-tertiary)">
                suppressed
              </span>
              <span className="text-sm font-medium text-(--color-text-secondary) truncate">
                {finding.title}
              </span>
              <span className="text-xs text-(--color-text-tertiary) ml-auto shrink-0">
                {Math.round(finding.confidence * 100)}%
              </span>
            </div>
            {finding.file_path && (
              <code className="text-xs text-(--color-text-tertiary) font-mono block mb-2">
                {finding.file_path}
              </code>
            )}
            <p className="text-sm text-(--color-text-tertiary) whitespace-pre-wrap">
              {displayBody}
            </p>
            <div className="flex gap-3 mt-2 items-center">
              <button
                onClick={handleRestore}
                className="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
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
      className={`border rounded-lg p-3 ${config.bg} ${focused ? "border-(--color-accent) ring-1 ring-(--color-accent)/40" : "border-(--color-border-subtle)"}`}
    >
      <div className="flex items-start gap-2">
        <Icon className={`w-4 h-4 mt-0.5 shrink-0 ${config.color}`} />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <span className={`text-xs font-semibold uppercase ${config.color}`}>
              {effectiveSeverity}
            </span>
            {!STANDARD_AGENT_TYPES.has(finding.agent_type) && (
              <span className="bg-(--color-state-waiting-bg) text-(--color-state-waiting) text-xs px-1.5 py-0.5 rounded capitalize">
                {finding.agent_type.replace(/_/g, " ")}
              </span>
            )}
            <span className="text-sm font-medium text-(--color-text-primary) truncate">
              {finding.title}
            </span>
            {sessionDecision && (
              <span
                className={`text-xs px-1.5 py-0.5 rounded ${sessionDecision === "accept" ? "bg-(--color-state-ready-bg) text-(--color-state-ready)" : "bg-(--color-elevated) text-(--color-text-secondary)"}`}
              >
                {sessionDecision === "accept" ? "Accepted" : "Deferred"}
              </span>
            )}
            {finding.baseline_decision && !sessionDecision && (
              <span className="text-xs px-1.5 py-0.5 rounded bg-(--color-elevated) text-(--color-text-tertiary) italic">
                Prev: {finding.baseline_decision}
              </span>
            )}
            {finding.delta_state && (
              <span
                className={`text-xs px-1.5 py-0.5 rounded ${
                  finding.delta_state === "new"
                    ? "bg-(--color-state-ready-bg) text-(--color-state-ready)"
                    : finding.delta_state === "stale"
                      ? "bg-(--color-sev-warning-bg) text-(--color-sev-warning)"
                      : "bg-(--color-elevated) text-(--color-text-tertiary)"
                }`}
              >
                {finding.delta_state}
              </span>
            )}
            <span className="text-xs text-(--color-text-tertiary) ml-auto shrink-0">
              {Math.round(finding.confidence * 100)}%
            </span>
          </div>

          {finding.file_path && (
            <code className="text-xs text-(--color-text-secondary) font-mono block mb-2">
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

          <div className="space-y-1.5 mb-2">
            <div className="flex gap-1.5 flex-wrap">
              {trust.provenanceBadges.map((badge) => (
                <span
                  key={badge.key}
                  className={`text-xs px-1.5 py-0.5 rounded ${
                    badge.tone === "support"
                      ? "bg-(--color-state-ready-bg) text-(--color-state-ready)"
                      : "bg-(--color-elevated) text-(--color-text-secondary)"
                  }`}
                >
                  {badge.label}
                </span>
              ))}
            </div>
            {(trust.supportBadges.length > 0 || trust.warningBadges.length > 0) && (
              <div className="flex gap-1.5 flex-wrap">
                {trust.supportBadges.map((badge) => (
                  <span
                    key={badge.key}
                    className="text-xs px-1.5 py-0.5 rounded bg-(--color-sev-info-bg) text-(--color-sev-info)"
                  >
                    {badge.label}
                  </span>
                ))}
                {trust.warningBadges.map((badge) => (
                  <span
                    key={badge.key}
                    className="text-xs px-1.5 py-0.5 rounded bg-(--color-sev-warning-bg) text-(--color-sev-warning)"
                  >
                    {badge.label}
                  </span>
                ))}
              </div>
            )}
          </div>

          {editing ? (
            <div className="space-y-2">
              <textarea
                value={editBody}
                onChange={(e) => setEditBody(e.target.value)}
                rows={3}
                className="w-full bg-(--color-elevated) border border-(--color-border) rounded px-2 py-1 text-sm text-(--color-text-primary) focus:outline-none focus:border-(--color-border-strong) resize-y"
              />
              <div className="flex gap-2">
                <button
                  onClick={handleSave}
                  disabled={saving}
                  className="flex items-center gap-1 text-xs text-(--color-state-ready) hover:opacity-80 transition-opacity"
                >
                  <Check className="w-3 h-3" /> Save
                </button>
                <button
                  onClick={() => {
                    setEditing(false);
                    setEditBody(displayBody);
                  }}
                  className="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
                >
                  <X className="w-3 h-3" /> Cancel
                </button>
              </div>
            </div>
          ) : (
            <p className="text-sm text-(--color-text-secondary) whitespace-pre-wrap">
              {displayBody}
            </p>
          )}

          {!editing && (
            <div className="flex gap-3 mt-2 items-center flex-wrap">
              {!sessionDecision && (
                <>
                  <button
                    onClick={handleAccept}
                    className="flex items-center gap-1 text-xs text-(--color-accent) hover:opacity-80 transition-opacity"
                  >
                    <ThumbsUp className="w-3 h-3" /> Accept
                  </button>
                  <button
                    onClick={handleDefer}
                    className="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
                  >
                    <Clock className="w-3 h-3" /> Defer
                  </button>
                </>
              )}
              <button
                onClick={() => setEditing(true)}
                className="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
              >
                <Pencil className="w-3 h-3" /> Edit
              </button>
              <button
                onClick={handleSuppress}
                className="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-sev-blocker) transition-colors"
              >
                <X className="w-3 h-3" /> Suppress
              </button>
              {hasPendingFix && (
                <button
                  onClick={() => setShowFix((v) => !v)}
                  className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded bg-(--color-sev-warning-bg) text-(--color-sev-warning) hover:opacity-80 transition-opacity"
                >
                  <Wrench className="w-3 h-3" /> Fix available
                </button>
              )}
              {fixApplied && (
                <span className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded bg-(--color-state-ready-bg) text-(--color-state-ready)">
                  <Check className="w-3 h-3" /> Fix applied
                </span>
              )}
              {fixRejected && (
                <span className="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded bg-(--color-elevated) text-(--color-text-tertiary)">
                  Fix rejected
                </span>
              )}
              {finding.evidence && (
                <button
                  onClick={() => setShowEvidence((v) => !v)}
                  className="text-xs text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
                >
                  {showEvidence ? "Hide evidence" : "Evidence"}
                </button>
              )}
              {trust.explanation && (
                <button
                  onClick={() => setShowWhy((v) => !v)}
                  className="flex items-center gap-1 text-xs text-(--color-text-secondary) hover:text-(--color-text-primary) transition-colors"
                >
                  <HelpCircle className="w-3 h-3" /> {showWhy ? "Hide why" : "Why?"}
                </button>
              )}
            </div>
          )}

          {showEvidence && finding.evidence && (
            <div className="mt-2 p-2 rounded bg-(--color-elevated) border border-(--color-border)">
              <pre className="text-xs text-(--color-text-secondary) font-mono whitespace-pre-wrap break-words">
                {finding.evidence}
              </pre>
            </div>
          )}

          {showWhy && trust.explanation && (
            <div className="mt-2 p-2 rounded bg-(--color-elevated) border border-(--color-border) space-y-1.5">
              <div className="text-xs text-(--color-text-tertiary) font-medium uppercase tracking-wider">
                Why this finding was surfaced
              </div>
              <TrustSection
                label="Origin"
                lines={[
                  `Source: ${trust.explanation.origin.source_kind}`,
                  trust.explanation.origin.lane_id
                    ? `Lane: ${trust.explanation.origin.lane_id}`
                    : null,
                  trust.explanation.origin.provider_name
                    ? `Provider: ${trust.explanation.origin.provider_name}`
                    : null,
                  trust.explanation.origin.source_id
                    ? `Source ID: ${trust.explanation.origin.source_id}`
                    : null,
                ]}
              />
              {finding.evidence && (
                <TrustSection label="Evidence" lines={["Attached evidence is available above."]} />
              )}
              {(trust.hasDeterministicSupport || trust.warningBadges.length > 0) && (
                <TrustSection
                  label="Deterministic inputs"
                  lines={[
                    trust.explanation.issue_context &&
                    trust.explanation.issue_context.included_count > 0
                      ? `${trust.explanation.issue_context.included_count} issue context item${trust.explanation.issue_context.included_count === 1 ? "" : "s"} included`
                      : null,
                    trust.explanation.issue_context?.sources?.length
                      ? `Issue sources: ${trust.explanation.issue_context.sources.join(", ")}`
                      : null,
                    trust.hasOwnership ? "Owners mapped from repository context" : null,
                    trust.hasPlatformContext ? "Platform context was available for this run" : null,
                    trust.warningBadges.some((badge) => badge.key === "ai-only")
                      ? "No deterministic support beyond model inference"
                      : null,
                  ]}
                />
              )}
              {trust.explanation.preferences && (
                <TrustSection
                  label="Reviewer history"
                  lines={[
                    trust.explanation.preferences.category_tag
                      ? `Category: ${trust.explanation.preferences.category_tag}`
                      : null,
                    trust.explanation.preferences.accept_rate != null
                      ? `Accept rate: ${Math.round(trust.explanation.preferences.accept_rate * 100)}%`
                      : null,
                    trust.explanation.preferences.total_decisions != null
                      ? `Decisions observed: ${trust.explanation.preferences.total_decisions}`
                      : null,
                    trust.explanation.preferences.override_action
                      ? `Override: ${trust.explanation.preferences.override_action}`
                      : null,
                  ]}
                />
              )}
              {trust.explanation.ownership && trust.explanation.ownership.owners.length > 0 && (
                <TrustSection
                  label="Ownership"
                  lines={[`Owners: ${trust.explanation.ownership.owners.join(", ")}`]}
                />
              )}
              {trust.explanation.ranking && (
                <div className="text-xs text-(--color-text-secondary)">
                  Confidence: {Math.round(trust.explanation.ranking.confidence_raw * 100)}%
                  {trust.explanation.ranking.suppressed_reason && (
                    <span className="ml-2 text-(--color-sev-warning)">
                      Suppressed: {trust.explanation.ranking.suppressed_reason}
                    </span>
                  )}
                </div>
              )}
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

function TrustSection({ label, lines }: { label: string; lines: Array<string | null> }) {
  const visibleLines = lines.filter((line): line is string => Boolean(line));
  if (visibleLines.length === 0) return null;

  return (
    <div className="space-y-0.5">
      <div className="text-xs text-(--color-text-tertiary)">{label}</div>
      {visibleLines.map((line) => (
        <div key={line} className="text-xs text-(--color-text-secondary)">
          {line}
        </div>
      ))}
    </div>
  );
}
