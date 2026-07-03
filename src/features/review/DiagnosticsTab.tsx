import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Activity,
  BookOpen,
  Filter,
  GitBranch,
  Loader2,
  Package,
  RefreshCw,
  ShieldCheck,
  ShieldAlert,
} from "lucide-react";
import { getEnvironmentSummary, getEventLog, refreshPrMetadata, parseError } from "../../lib/ipc";
import type {
  CapabilitySupport,
  ContextPackItem,
  ContextPackSummary,
  EnvironmentSummary,
  LocalChecksSummary,
  PlatformCapabilities,
  PlatformMetadata,
  ProviderControlPlaneSnapshot,
  ProviderSelectionTrace,
} from "../../lib/types";
import { getPlatformAuthDiagnostic, isPlatformAuthReady } from "../../lib/types";

interface EventEntry {
  timestamp: string;
  event_type: string;
  payload: Record<string, unknown>;
}

interface DiagnosticsProps {
  runId: string;
  prId?: string | null;
  onMetadataRefreshed?: () => Promise<void> | void;
  contextPackSummary?: ContextPackSummary | null;
  localChecksSummary?: LocalChecksSummary | null;
  platformMetadata?: PlatformMetadata | null;
  platformMetadataFetchedAt?: string | null;
  platformCapabilities?: PlatformCapabilities | null;
  platformCapabilitiesFetchedAt?: string | null;
  providerSelection?: ProviderSelectionTrace | null;
  providerControl?: ProviderControlPlaneSnapshot | null;
}

export function DiagnosticsTab({
  runId,
  prId,
  onMetadataRefreshed,
  contextPackSummary,
  localChecksSummary,
  platformMetadata,
  platformMetadataFetchedAt,
  platformCapabilities,
  platformCapabilitiesFetchedAt,
  providerSelection,
  providerControl,
}: DiagnosticsProps) {
  const [events, setEvents] = useState<EventEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [filterText, setFilterText] = useState("");
  const [refreshingMetadata, setRefreshingMetadata] = useState(false);
  const [refreshMetadataError, setRefreshMetadataError] = useState<string | null>(null);
  const [environmentSummary, setEnvironmentSummary] = useState<EnvironmentSummary | null>(null);

  const load = useCallback(async () => {
    try {
      const data = (await getEventLog(runId)) as EventEntry[];
      setEvents(data);
    } catch (err) {
      setError(parseError(err).message);
    } finally {
      setLoading(false);
    }
  }, [runId]);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    let cancelled = false;
    getEnvironmentSummary()
      .then((summary) => {
        if (!cancelled) {
          setEnvironmentSummary(summary);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnvironmentSummary(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const refreshMetadata = useCallback(async () => {
    if (!prId) return;
    setRefreshingMetadata(true);
    setRefreshMetadataError(null);
    try {
      await refreshPrMetadata(prId);
      await Promise.resolve(onMetadataRefreshed?.());
    } catch (err) {
      setRefreshMetadataError(parseError(err).message);
    } finally {
      setRefreshingMetadata(false);
    }
  }, [onMetadataRefreshed, prId]);

  const filtered = useMemo(() => {
    if (!filterText.trim()) return events;
    const lower = filterText.toLowerCase();
    return events.filter(
      (e) =>
        e.event_type.toLowerCase().includes(lower) ||
        JSON.stringify(e.payload).toLowerCase().includes(lower),
    );
  }, [events, filterText]);

  if (loading) {
    return (
      <div className="flex items-center justify-center p-8">
        <Loader2 className="w-5 h-5 animate-spin text-(--color-text-secondary)" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-4">
        <p className="text-sm text-(--color-sev-blocker)">{error}</p>
      </div>
    );
  }

  return (
    <div className="overflow-y-auto p-4 space-y-4">
      <div className="rounded-lg border border-(--color-border-subtle) bg-(--color-surface) px-4 py-3">
        <div className="text-sm font-medium text-(--color-text-primary)">Evidence trail</div>
        <p className="mt-1 text-xs text-(--color-text-tertiary)">
          Audit the deterministic inputs, platform context, and event history behind this review.
        </p>
      </div>
      {platformMetadata && (
        <PlatformMetadataSection
          data={platformMetadata}
          fetchedAt={platformMetadataFetchedAt ?? null}
          onRefresh={refreshMetadata}
          isRefreshing={refreshingMetadata}
          refreshError={refreshMetadataError}
          canRefresh={Boolean(prId)}
        />
      )}
      {platformCapabilities && (
        <PlatformCapabilitiesSection
          data={platformCapabilities}
          fetchedAt={platformCapabilitiesFetchedAt ?? null}
          environmentSummary={environmentSummary}
        />
      )}
      {providerControl && (
        <ProviderControlSection
          providerControl={providerControl}
          providerSelection={providerSelection ?? null}
        />
      )}
      {contextPackSummary && <ContextPackSection data={contextPackSummary} />}
      {contextPackSummary && <IssueContextSection items={contextPackSummary.items ?? []} />}
      {localChecksSummary && <LocalChecksSection data={localChecksSummary} />}

      <div className="flex items-center gap-2">
        <Activity className="w-4 h-4 text-(--color-text-secondary)" />
        <h3 className="text-sm font-medium text-(--color-text-primary)">Event Timeline</h3>
        <span className="text-xs text-(--color-text-tertiary)">({filtered.length} events)</span>
      </div>

      <div className="relative">
        <Filter className="absolute left-2.5 top-2 w-3.5 h-3.5 text-(--color-text-tertiary)" />
        <input
          type="text"
          value={filterText}
          onChange={(e) => setFilterText(e.target.value)}
          placeholder="Filter by event type or payload..."
          className="w-full pl-8 pr-3 py-1.5 text-xs bg-(--color-surface) border border-(--color-border-subtle) rounded-md text-(--color-text-secondary) placeholder:text-(--color-text-tertiary) focus:outline-none focus:border-(--color-border)"
        />
      </div>

      {filtered.length === 0 && (
        <p className="text-xs text-(--color-text-tertiary) py-4 text-center">
          {events.length === 0 ? "No events recorded for this run." : "No events match filter."}
        </p>
      )}

      <div className="space-y-1">
        {filtered.map((event, i) => (
          <EventRow key={i} event={event} />
        ))}
      </div>
    </div>
  );
}

function ProviderControlSection({
  providerControl,
  providerSelection,
}: {
  providerControl: ProviderControlPlaneSnapshot;
  providerSelection: ProviderSelectionTrace | null;
}) {
  const noteworthySelection =
    providerSelection &&
    (providerSelection.selection_mode === "fallback" || providerSelection.warnings.length > 0);
  const degradedProviders = providerControl.providers.filter(
    (provider) => provider.status !== "ready",
  );

  if (!noteworthySelection && degradedProviders.length === 0) {
    return null;
  }

  return (
    <div className="border border-(--color-border-subtle) rounded-lg bg-(--color-surface)">
      <div className="px-4 py-3 border-b border-(--color-border-subtle)">
        <div className="text-sm font-medium text-(--color-text-primary)">Provider control</div>
        <p className="mt-1 text-xs text-(--color-text-tertiary)">
          Routing caveats, degraded providers, and fallback details for this environment.
        </p>
      </div>
      <div className="px-4 py-3 space-y-3">
        {providerSelection && (
          <div className="rounded-md border border-(--color-border-subtle) bg-(--color-base) px-3 py-2">
            <div className="text-xs text-(--color-text-secondary)">
              Selected{" "}
              <span className="text-(--color-text-primary)">
                {providerSelection.selected_provider}
              </span>{" "}
              via{" "}
              <span className="text-(--color-text-primary)">
                {providerSelection.selection_mode}
              </span>
            </div>
            {providerSelection.warnings.map((warning) => (
              <p key={warning} className="mt-1 text-xs text-(--color-sev-warning)">
                {warning}
              </p>
            ))}
          </div>
        )}
        {degradedProviders.map((provider) => (
          <div
            key={provider.provider_id}
            className="rounded-md border border-(--color-border-subtle) bg-(--color-base) px-3 py-2"
          >
            <div className="flex items-center gap-2">
              <code className="text-[11px] text-(--color-text-secondary)">
                {provider.provider_id}
              </code>
              <span className="text-[10px] uppercase tracking-wide text-(--color-text-tertiary)">
                {provider.status}
              </span>
            </div>
            <p className="mt-1 text-xs text-(--color-text-secondary)">{provider.status_reason}</p>
            {provider.warnings.slice(0, 2).map((warning) => (
              <p key={warning} className="mt-1 text-xs text-(--color-sev-warning)">
                {warning}
              </p>
            ))}
          </div>
        ))}
      </div>
    </div>
  );
}

function PlatformCapabilitiesSection({
  data,
  fetchedAt,
  environmentSummary,
}: {
  data: PlatformCapabilities;
  fetchedAt: string | null;
  environmentSummary: EnvironmentSummary | null;
}) {
  const [expanded, setExpanded] = useState(false);
  const counts = data.capabilities.reduce(
    (acc, capability) => {
      acc[capability.support] += 1;
      return acc;
    },
    { full: 0, partial: 0, none: 0 } as Record<CapabilitySupport, number>,
  );
  const authReady = isPlatformAuthReady(data.platform, environmentSummary);
  const authDiagnostic = getPlatformAuthDiagnostic(data.platform, environmentSummary);

  return (
    <div className="border border-(--color-border-subtle) rounded-lg bg-(--color-surface)">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full text-left px-4 py-3 flex items-center gap-2 hover:bg-(--color-elevated) transition-colors"
      >
        <ShieldCheck className="w-4 h-4 text-(--color-accent)" />
        <span className="text-sm font-medium text-(--color-text-primary)">
          Platform capabilities
        </span>
        <span className="text-xs text-(--color-text-tertiary)">
          {counts.full} full, {counts.partial} partial, {counts.none} blocked
          {fetchedAt ? ` · ${formatTimestamp(fetchedAt)}` : ""}
        </span>
      </button>
      {expanded && (
        <div className="px-4 pb-3 space-y-2">
          <div className="rounded-md border border-(--color-border-subtle) bg-(--color-base) px-3 py-2">
            <div className="flex items-center gap-2">
              <ShieldAlert
                className={`w-3.5 h-3.5 ${authReady ? "text-(--color-accent)" : "text-red-300"}`}
              />
              <span className="text-xs font-medium text-(--color-text-primary)">
                {authReady ? "Auth ready" : "Auth not ready"}
              </span>
            </div>
            {!authReady && authDiagnostic && (
              <p className="mt-1 text-xs text-(--color-text-secondary)">{authDiagnostic}</p>
            )}
          </div>
          {data.capabilities.map((capability) => (
            <div
              key={capability.key}
              className="rounded-md border border-(--color-border-subtle) bg-(--color-base) px-3 py-2"
            >
              <div className="flex items-center gap-2">
                <code className="text-[11px] text-(--color-text-secondary)">{capability.key}</code>
                <span
                  className={`rounded px-1.5 py-0.5 text-[10px] font-medium ${
                    capability.support === "full"
                      ? "bg-(--color-state-ready-bg) text-emerald-300"
                      : capability.support === "partial"
                        ? "bg-(--color-sev-warning-bg) text-yellow-300"
                        : "bg-(--color-sev-blocker-bg) text-red-300"
                  }`}
                >
                  {capability.support}
                </span>
              </div>
              {capability.constraints.length > 0 && (
                <div className="mt-1 space-y-1">
                  {capability.constraints.map((constraint) => (
                    <p key={constraint.code} className="text-xs text-(--color-text-secondary)">
                      {constraint.message}
                    </p>
                  ))}
                </div>
              )}
              {capability.fallback && (
                <p className="mt-1 text-xs text-(--color-text-tertiary)">
                  Fallback: {capability.fallback.action} · {capability.fallback.reason}
                </p>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function EventRow({ event }: { event: EventEntry }) {
  const [expanded, setExpanded] = useState(false);
  const time = new Date(event.timestamp).toLocaleTimeString();

  const payloadPreview = useMemo(() => {
    const keys = Object.keys(event.payload);
    if (keys.length === 0) return "";
    const parts: string[] = [];
    for (const key of keys.slice(0, 3)) {
      const val = event.payload[key];
      if (typeof val === "number" || typeof val === "string") {
        parts.push(`${key}=${val}`);
      }
    }
    return parts.join(", ");
  }, [event.payload]);

  return (
    <div className="border border-(--color-border-subtle) rounded-md bg-(--color-surface)">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full text-left px-3 py-2 flex items-center gap-2 hover:bg-(--color-elevated) transition-colors"
      >
        <span className="text-xs text-(--color-text-tertiary) shrink-0 font-mono w-20">{time}</span>
        <span className="text-xs font-medium text-(--color-text-primary) shrink-0">
          {event.event_type}
        </span>
        {payloadPreview && (
          <span className="text-xs text-(--color-text-tertiary) truncate">{payloadPreview}</span>
        )}
      </button>
      {expanded && Object.keys(event.payload).length > 0 && (
        <div className="px-3 pb-2 pt-0">
          <pre className="text-xs text-(--color-text-secondary) bg-(--color-base) rounded p-2 overflow-x-auto max-h-40">
            {JSON.stringify(event.payload, null, 2)}
          </pre>
        </div>
      )}
    </div>
  );
}

function ContextPackSection({ data }: { data: ContextPackSummary }) {
  const [expanded, setExpanded] = useState(false);
  const items = data.items ?? [];
  const included = items.filter((i) => i.included);

  return (
    <div className="border border-(--color-border-subtle) rounded-lg bg-(--color-surface)">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full text-left px-4 py-3 flex items-center gap-2 hover:bg-(--color-elevated) transition-colors"
      >
        <Package className="w-4 h-4 text-indigo-400" />
        <span className="text-sm font-medium text-(--color-text-primary)">
          Context pack evidence
        </span>
        <span className="text-xs text-(--color-text-tertiary)">
          {included.length} item{included.length !== 1 ? "s" : ""},{" "}
          {`${(data.total_bytes / 1024).toFixed(1)}KB`}
        </span>
      </button>
      {expanded && (
        <div className="px-4 pb-3 space-y-2">
          {items.map((item, i) => (
            <div
              key={i}
              className={`flex items-center gap-2 text-xs ${item.included ? "text-(--color-text-secondary)" : "text-(--color-text-tertiary)"}`}
            >
              <span
                className={`w-1.5 h-1.5 rounded-full ${item.included ? "bg-(--color-accent)" : "bg-(--color-border)"}`}
              />
              <span className="font-medium">{item.kind}</span>
              <span className="truncate">{item.label}</span>
              {item.included && (
                <span className="text-(--color-text-tertiary) ml-auto shrink-0">{item.bytes}B</span>
              )}
              {!item.included && item.omit_reason && (
                <span className="text-(--color-text-tertiary) ml-auto shrink-0 italic">
                  {item.omit_reason}
                </span>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function PlatformMetadataSection({
  data,
  fetchedAt,
  onRefresh,
  isRefreshing,
  refreshError,
  canRefresh,
}: {
  data: PlatformMetadata;
  fetchedAt: string | null;
  onRefresh: () => void;
  isRefreshing: boolean;
  refreshError: string | null;
  canRefresh: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const fetchedTimeLabel = formatTimestamp(fetchedAt);

  return (
    <div className="border border-(--color-border-subtle) rounded-lg bg-(--color-surface)">
      <div className="w-full px-4 py-3 flex items-center gap-2">
        <button
          onClick={() => setExpanded(!expanded)}
          className="min-w-0 flex items-center gap-2 text-left hover:text-(--color-text-primary) transition-colors"
        >
          <GitBranch className="w-4 h-4 text-(--color-text-secondary)" />
          <span className="text-sm font-medium text-(--color-text-primary)">
            {data.platform === "gitlab"
              ? "GitLab"
              : data.platform === "bitbucket"
                ? "Bitbucket"
                : "GitHub"}{" "}
            Metadata
          </span>
          <span className="text-xs text-(--color-text-tertiary)">
            {data.head_sha.slice(0, 7)} &middot; {data.labels.length} label
            {data.labels.length !== 1 ? "s" : ""}
            {data.platform === "github" &&
              data.linked_issue_numbers.length > 0 &&
              ` \u00b7 ${data.linked_issue_numbers.length} issue${data.linked_issue_numbers.length !== 1 ? "s" : ""}`}
            {data.platform === "gitlab" &&
              data.closes_issues.length > 0 &&
              ` \u00b7 ${data.closes_issues.length} issue${data.closes_issues.length !== 1 ? "s" : ""}`}
            {data.platform === "bitbucket" &&
              data.jira_issue_keys.length > 0 &&
              ` \u00b7 ${data.jira_issue_keys.length} Jira key${data.jira_issue_keys.length !== 1 ? "s" : ""}`}
            {fetchedTimeLabel && ` \u00b7 ${fetchedTimeLabel}`}
          </span>
        </button>
        <button
          onClick={onRefresh}
          disabled={isRefreshing || !canRefresh}
          className="ml-auto flex items-center gap-1 text-[11px] text-(--color-text-secondary) bg-(--color-elevated) hover:bg-(--color-elevated) px-2 py-1 rounded transition-colors disabled:opacity-50"
        >
          <RefreshCw className={`w-3 h-3 ${isRefreshing ? "animate-spin" : ""}`} />
          {isRefreshing ? "Refreshing..." : "Refresh"}
        </button>
      </div>
      {refreshError && (
        <p className="text-xs text-(--color-sev-blocker) bg-(--color-sev-blocker-bg) mx-4 mb-2 px-2 py-1 rounded">
          {refreshError}
        </p>
      )}
      {expanded && (
        <div className="px-4 pb-3 space-y-1 text-xs text-(--color-text-secondary)">
          <div>
            <span className="text-(--color-text-tertiary)">Head:</span> {data.head_ref} (
            {data.head_sha.slice(0, 12)})
          </div>
          <div>
            <span className="text-(--color-text-tertiary)">Base:</span> {data.base_ref} (
            {data.base_sha.slice(0, 12)})
          </div>
          {data.draft && (
            <div className="text-(--color-sev-warning)">
              Draft {data.platform === "gitlab" ? "MR" : "PR"}
            </div>
          )}
          {data.labels.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Labels:</span> {data.labels.join(", ")}
            </div>
          )}
          {data.platform === "github" && data.requested_reviewers.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Reviewers:</span>{" "}
              {data.requested_reviewers.join(", ")}
            </div>
          )}
          {data.platform === "github" && data.requested_teams.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Teams:</span>{" "}
              {data.requested_teams.join(", ")}
            </div>
          )}
          {data.platform === "gitlab" && data.reviewers.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Reviewers:</span>{" "}
              {data.reviewers.join(", ")}
            </div>
          )}
          {data.platform === "github" && data.review_state_summary.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Reviews:</span>{" "}
              {data.review_state_summary.map((r) => `${r.login}: ${r.state}`).join(", ")}
            </div>
          )}
          {data.platform === "gitlab" && data.approval_status && (
            <div>
              <span className="text-(--color-text-tertiary)">Approval:</span>{" "}
              {data.approval_status.approved ? "Approved" : "Pending"}
              {data.approval_status.approved_by.length > 0 &&
                ` by ${data.approval_status.approved_by.join(", ")}`}
            </div>
          )}
          {data.platform === "github" && data.linked_issue_numbers.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Linked issues:</span>{" "}
              {data.linked_issue_numbers.map((n) => `#${n}`).join(", ")}
            </div>
          )}
          {data.platform === "gitlab" && data.closes_issues.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Closes issues:</span>{" "}
              {data.closes_issues.map((n) => `#${n}`).join(", ")}
            </div>
          )}
          {data.platform === "github" && data.text_issue_refs.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Text refs:</span>{" "}
              {data.text_issue_refs.join(", ")}
            </div>
          )}
          {data.platform === "bitbucket" && data.reviewers.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Reviewers:</span>{" "}
              {data.reviewers.join(", ")}
            </div>
          )}
          {data.platform === "bitbucket" && data.default_reviewers.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Default reviewers:</span>{" "}
              {data.default_reviewers.join(", ")}
            </div>
          )}
          {data.platform === "bitbucket" && data.approval_status && (
            <div>
              <span className="text-(--color-text-tertiary)">Approval:</span>{" "}
              {data.approval_status.approved ? "Approved" : "Pending"}
              {data.approval_status.approved_by.length > 0 &&
                ` by ${data.approval_status.approved_by.join(", ")}`}
            </div>
          )}
          {data.platform === "bitbucket" && data.jira_issue_keys.length > 0 && (
            <div>
              <span className="text-(--color-text-tertiary)">Jira issues:</span>{" "}
              {data.jira_issue_keys.join(", ")}
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function formatTimestamp(value: string | null): string | null {
  if (!value) return null;
  const parsed = new Date(value);
  return Number.isNaN(parsed.getTime()) ? value : parsed.toLocaleTimeString();
}

function IssueContextSection({ items }: { items: ContextPackItem[] }) {
  const [expanded, setExpanded] = useState(false);
  const issueItems = items.filter((i) => i.kind === "issue");

  if (issueItems.length === 0) return null;

  const included = issueItems.filter((i) => i.included);
  const omitted = issueItems.filter((i) => !i.included);

  const grouped = new Map<string, ContextPackItem[]>();
  for (const item of issueItems) {
    const tracker = item.source.split(":")[0] || "unknown";
    const existing = grouped.get(tracker) || [];
    existing.push(item);
    grouped.set(tracker, existing);
  }

  return (
    <div className="border border-(--color-border-subtle) rounded-lg bg-(--color-surface)">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full text-left px-4 py-3 flex items-center gap-2 hover:bg-(--color-elevated) transition-colors"
      >
        <BookOpen className="w-4 h-4 text-(--color-sev-info)" />
        <span className="text-sm font-medium text-(--color-text-primary)">
          Issue context evidence
        </span>
        <span className="text-xs text-(--color-text-tertiary)">
          {included.length} included
          {omitted.length > 0 && `, ${omitted.length} omitted`}
        </span>
      </button>
      {expanded && (
        <div className="px-4 pb-3 space-y-3">
          {[...grouped.entries()].map(([tracker, trackerItems]) => (
            <div key={tracker}>
              <h4 className="text-xs font-medium text-(--color-text-secondary) uppercase tracking-wider mb-1">
                {tracker}
              </h4>
              <div className="space-y-1">
                {trackerItems.map((item, i) => (
                  <div
                    key={i}
                    className={`flex items-center gap-2 text-xs ${item.included ? "text-(--color-text-secondary)" : "text-(--color-text-tertiary)"}`}
                  >
                    <span
                      className={`w-1.5 h-1.5 rounded-full ${item.included ? "bg-(--color-sev-info)" : "bg-(--color-border)"}`}
                    />
                    <span className="truncate">{item.label}</span>
                    {item.confidence && (
                      <span
                        className={`shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium ${
                          item.confidence === "high"
                            ? "bg-(--color-state-ready-bg) text-emerald-300"
                            : item.confidence === "medium"
                              ? "bg-(--color-sev-warning-bg) text-yellow-300"
                              : "bg-(--color-elevated) text-(--color-text-secondary)"
                        }`}
                      >
                        {item.confidence}
                      </span>
                    )}
                    <span className="text-(--color-text-tertiary) text-[10px] truncate ml-auto shrink-0">
                      {item.source}
                    </span>
                    {!item.included && item.omit_reason && (
                      <span className="text-(--color-text-tertiary) italic shrink-0">
                        {item.omit_reason}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function LocalChecksSection({ data }: { data: LocalChecksSummary }) {
  const [expanded, setExpanded] = useState(false);
  const items = data.items ?? [];
  const totalErrors = data.total_errors ?? 0;
  const toolsRun = data.tools_run ?? [];

  return (
    <div className="border border-(--color-border-subtle) rounded-lg bg-(--color-surface)">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full text-left px-4 py-3 flex items-center gap-2 hover:bg-(--color-elevated) transition-colors"
      >
        <ShieldCheck className="w-4 h-4 text-amber-400" />
        <span className="text-sm font-medium text-(--color-text-primary)">
          Local check evidence
        </span>
        <span className="text-xs text-(--color-text-tertiary)">
          {totalErrors} error{totalErrors !== 1 ? "s" : ""} via {toolsRun.join(", ") || "none"}
        </span>
      </button>
      {expanded && (
        <div className="px-4 pb-3 space-y-1">
          {items.length === 0 && (
            <p className="text-xs text-(--color-text-tertiary) py-2">
              No local check errors found.
            </p>
          )}
          {items.map((item, i) => (
            <div key={i} className="text-xs text-(--color-text-secondary) flex items-start gap-2">
              <span className="text-(--color-sev-blocker) shrink-0 font-mono">{item.tool}</span>
              <span className="text-(--color-text-secondary) shrink-0 font-mono truncate max-w-48">
                {item.file}
                {item.line != null ? `:${item.line}` : ""}
              </span>
              <span className="truncate">{item.message}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
