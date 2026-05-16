import type {
  ContextPackSummary,
  Finding,
  LocalChecksSummary,
  PlatformMetadata,
  ReviewFreshnessSummary,
} from "./types";

export interface ParsedFindingExplanation {
  schema_version: number;
  origin: {
    source_kind: string;
    source_id: string | null;
    lane_id: string | null;
    provider_name: string | null;
  };
  ranking?: {
    confidence_raw: number;
    severity_raw: string;
    suppressed_reason?: string | null;
  };
  preferences?: {
    category_tag: string | null;
    accept_rate: number | null;
    total_decisions: number | null;
    override_action?: string | null;
  };
  ownership?: {
    owners: string[];
  };
  issue_context?: {
    included_count: number;
    sources: string[];
  };
}

export interface TrustBadge {
  key: string;
  label: string;
  tone: "neutral" | "support" | "caution";
}

export interface FindingTrustViewModel {
  explanation: ParsedFindingExplanation | null;
  provenanceBadges: TrustBadge[];
  supportBadges: TrustBadge[];
  warningBadges: TrustBadge[];
  hasEvidence: boolean;
  hasDeterministicSupport: boolean;
  hasReviewerHistory: boolean;
  hasOwnership: boolean;
  hasIssueContext: boolean;
  hasPlatformContext: boolean;
}

export interface RunTrustOverview {
  sourceCounts: Array<{ key: string; label: string; count: number }>;
  findingsWithEvidence: number;
  findingsWithIssueContext: number;
  findingsWithOwnership: number;
  findingsWithDeterministicSupport: number;
  localChecksIncluded: number;
  localCheckTools: string[];
  hasPlatformMetadata: boolean;
  platformFreshnessLabel: string | null;
  reviewFreshnessLabel: string | null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function asNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asStringArray(value: unknown): string[] {
  return Array.isArray(value)
    ? value.filter((item): item is string => typeof item === "string")
    : [];
}

function dedupeLabels(labels: string[]): string[] {
  return [...new Set(labels.filter(Boolean))];
}

function parseOrigin(
  value: unknown,
  fallback: Partial<ParsedFindingExplanation["origin"]> = {},
): ParsedFindingExplanation["origin"] | null {
  if (!isRecord(value)) {
    if (!fallback.source_kind) return null;
    return {
      source_kind: fallback.source_kind,
      source_id: fallback.source_id ?? null,
      lane_id: fallback.lane_id ?? null,
      provider_name: fallback.provider_name ?? null,
    };
  }

  const source_kind = asString(value.source_kind) ?? fallback.source_kind ?? null;
  if (!source_kind) return null;

  return {
    source_kind,
    source_id: asString(value.source_id) ?? fallback.source_id ?? null,
    lane_id: asString(value.lane_id) ?? fallback.lane_id ?? null,
    provider_name: asString(value.provider_name) ?? fallback.provider_name ?? null,
  };
}

function parseRanking(value: unknown): ParsedFindingExplanation["ranking"] | undefined {
  if (!isRecord(value)) return undefined;
  const confidence_raw = asNumber(value.confidence_raw);
  const severity_raw = asString(value.severity_raw);
  if (confidence_raw == null || !severity_raw) return undefined;
  return {
    confidence_raw,
    severity_raw,
    suppressed_reason: asString(value.suppressed_reason),
  };
}

function parsePreferences(value: unknown): ParsedFindingExplanation["preferences"] | undefined {
  if (!isRecord(value)) return undefined;
  return {
    category_tag: asString(value.category_tag),
    accept_rate: asNumber(value.accept_rate),
    total_decisions: asNumber(value.total_decisions),
    override_action: asString(value.override_action),
  };
}

function parseOwnership(value: unknown): ParsedFindingExplanation["ownership"] | undefined {
  if (!isRecord(value)) return undefined;
  const owners = asStringArray(value.owners);
  return owners.length > 0 ? { owners } : undefined;
}

function parseIssueContext(value: unknown): ParsedFindingExplanation["issue_context"] | undefined {
  if (!isRecord(value)) return undefined;
  const included_count = asNumber(value.included_count);
  if (included_count == null) return undefined;
  return {
    included_count,
    sources: asStringArray(value.sources),
  };
}

export function parseFindingExplanation(
  explainJson: string | null,
  fallbackOrigin: Partial<ParsedFindingExplanation["origin"]> = {},
): ParsedFindingExplanation | null {
  if (!explainJson) return null;

  try {
    const parsed = JSON.parse(explainJson) as unknown;
    if (!isRecord(parsed)) return null;

    const origin = parseOrigin(parsed.origin, fallbackOrigin);
    if (!origin) return null;

    return {
      schema_version: asNumber(parsed.schema_version) ?? 1,
      origin,
      ranking: parseRanking(parsed.ranking),
      preferences: parsePreferences(parsed.preferences),
      ownership: parseOwnership(parsed.ownership),
      issue_context: parseIssueContext(parsed.issue_context),
    };
  } catch {
    return null;
  }
}

export function sourceKindLabel(sourceKind: string | null | undefined): string {
  switch (sourceKind) {
    case "local_check":
      return "Local check";
    case "ai_provider":
    case null:
    case undefined:
      return "AI review";
    default:
      return sourceKind.replace(/_/g, " ");
  }
}

function hasPlatformSignals(platformMetadata: PlatformMetadata | null): boolean {
  if (!platformMetadata) return false;
  if (platformMetadata.labels.length > 0) return true;
  if (platformMetadata.platform === "github") {
    return (
      platformMetadata.requested_reviewers.length > 0 ||
      platformMetadata.requested_teams.length > 0 ||
      platformMetadata.linked_issue_numbers.length > 0 ||
      platformMetadata.text_issue_refs.length > 0
    );
  }
  if (platformMetadata.platform === "gitlab") {
    return platformMetadata.reviewers.length > 0 || platformMetadata.closes_issues.length > 0;
  }
  return (
    platformMetadata.reviewers.length > 0 ||
    platformMetadata.default_reviewers.length > 0 ||
    platformMetadata.jira_issue_keys.length > 0
  );
}

export function buildFindingTrustViewModel(
  finding: Finding,
  options: {
    platformMetadata?: PlatformMetadata | null;
  } = {},
): FindingTrustViewModel {
  const explanation = parseFindingExplanation(finding.explain_json, {
    source_kind: finding.source_kind ?? "ai_provider",
    source_id: finding.source_id,
    lane_id: finding.lane_id,
    provider_name: finding.provider_name,
  });
  const sourceKind = explanation?.origin.source_kind ?? finding.source_kind ?? "ai_provider";
  const providerName = explanation?.origin.provider_name ?? finding.provider_name;
  const laneId = explanation?.origin.lane_id ?? finding.lane_id;
  const sourceId = explanation?.origin.source_id ?? finding.source_id;

  const provenanceBadges: TrustBadge[] = [
    {
      key: "source-kind",
      label: sourceKindLabel(sourceKind),
      tone: sourceKind === "local_check" ? "support" : "neutral",
    },
  ];

  if (providerName) {
    provenanceBadges.push({ key: "provider", label: providerName, tone: "neutral" });
  }
  if (laneId) {
    provenanceBadges.push({ key: "lane", label: laneId, tone: "neutral" });
  }
  if (sourceId && sourceKind !== "ai_provider") {
    provenanceBadges.push({ key: "source-id", label: sourceId, tone: "support" });
  }

  const supportBadges: TrustBadge[] = [];
  const warningBadges: TrustBadge[] = [];

  if (finding.evidence) {
    supportBadges.push({ key: "evidence", label: "Evidence", tone: "support" });
  }
  if (sourceKind === "local_check") {
    supportBadges.push({ key: "deterministic-local", label: "Deterministic", tone: "support" });
  }
  if (explanation?.issue_context && explanation.issue_context.included_count > 0) {
    supportBadges.push({
      key: "issue-context",
      label: `Issue context (${explanation.issue_context.included_count})`,
      tone: "support",
    });
  }
  if (explanation?.ownership && explanation.ownership.owners.length > 0) {
    supportBadges.push({ key: "owners", label: "Owners", tone: "support" });
  }
  if (hasPlatformSignals(options.platformMetadata ?? null)) {
    supportBadges.push({ key: "platform-context", label: "Platform context", tone: "support" });
  }
  if (explanation?.preferences?.accept_rate != null) {
    supportBadges.push({ key: "reviewer-history", label: "Reviewer history", tone: "neutral" });
  }
  if (finding.diff_new_line == null && !finding.is_anchored && finding.file_path) {
    warningBadges.push({ key: "body-only", label: "Body-only fallback", tone: "caution" });
  }
  if (
    sourceKind === "ai_provider" &&
    supportBadges.every((badge) => badge.key !== "deterministic-local")
  ) {
    const hasConcreteSupport = supportBadges.some((badge) =>
      ["evidence", "issue-context", "owners"].includes(badge.key),
    );
    if (!hasConcreteSupport) {
      warningBadges.push({
        key: "ai-only",
        label: "AI inference only",
        tone: "caution",
      });
    }
  }

  return {
    explanation,
    provenanceBadges,
    supportBadges,
    warningBadges,
    hasEvidence: finding.evidence != null,
    hasDeterministicSupport: supportBadges.some((badge) =>
      ["deterministic-local", "issue-context", "owners"].includes(badge.key),
    ),
    hasReviewerHistory: explanation?.preferences?.accept_rate != null,
    hasOwnership: (explanation?.ownership?.owners.length ?? 0) > 0,
    hasIssueContext: (explanation?.issue_context?.included_count ?? 0) > 0,
    hasPlatformContext: supportBadges.some((badge) => badge.key === "platform-context"),
  };
}

function sourceCountKey(sourceKind: string | null | undefined): string {
  return sourceKind ?? "ai_provider";
}

function freshnessLabel(fetchedAt: string | null): string | null {
  if (!fetchedAt) return null;
  const parsed = new Date(fetchedAt);
  if (Number.isNaN(parsed.getTime())) return null;
  const ageMinutes = Math.max(0, Math.round((Date.now() - parsed.getTime()) / 60000));
  if (ageMinutes < 1) return "updated just now";
  if (ageMinutes < 60) return `updated ${ageMinutes}m ago`;
  const ageHours = Math.round(ageMinutes / 60);
  if (ageHours < 24) return `updated ${ageHours}h ago`;
  const ageDays = Math.round(ageHours / 24);
  return `updated ${ageDays}d ago`;
}

export function buildRunTrustOverview(input: {
  findings: Finding[];
  localChecksSummary?: LocalChecksSummary | null;
  contextPackSummary?: ContextPackSummary | null;
  platformMetadata?: PlatformMetadata | null;
  platformMetadataFetchedAt?: string | null;
  reviewFreshness: ReviewFreshnessSummary;
}): RunTrustOverview {
  const activeFindings = input.findings.filter((finding) => finding.status === "active");
  const sourceCountsMap = new Map<string, number>();
  let findingsWithEvidence = 0;
  let findingsWithIssueContext = 0;
  let findingsWithOwnership = 0;
  let findingsWithDeterministicSupport = 0;

  for (const finding of activeFindings) {
    const trust = buildFindingTrustViewModel(finding, {
      platformMetadata: input.platformMetadata ?? null,
    });
    const key = sourceCountKey(trust.explanation?.origin.source_kind ?? finding.source_kind);
    sourceCountsMap.set(key, (sourceCountsMap.get(key) ?? 0) + 1);
    if (trust.hasEvidence) findingsWithEvidence += 1;
    if (trust.hasIssueContext) findingsWithIssueContext += 1;
    if (trust.hasOwnership) findingsWithOwnership += 1;
    if (trust.hasDeterministicSupport) findingsWithDeterministicSupport += 1;
  }

  const sourceCounts = [...sourceCountsMap.entries()]
    .map(([key, count]) => ({
      key,
      label: sourceKindLabel(key),
      count,
    }))
    .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));

  const localChecksIncluded =
    input.localChecksSummary?.included_count ?? input.localChecksSummary?.items?.length ?? 0;

  let reviewFreshnessLabel: string | null = null;
  if (input.reviewFreshness.is_rerun) {
    reviewFreshnessLabel = input.reviewFreshness.head_changed_since_review
      ? "Rerun against newer head"
      : "Rerun on current head";
  }

  return {
    sourceCounts,
    findingsWithEvidence,
    findingsWithIssueContext,
    findingsWithOwnership,
    findingsWithDeterministicSupport,
    localChecksIncluded,
    localCheckTools: input.localChecksSummary?.tools_run ?? [],
    hasPlatformMetadata: input.platformMetadata != null,
    platformFreshnessLabel: freshnessLabel(input.platformMetadataFetchedAt ?? null),
    reviewFreshnessLabel,
  };
}

export function summarizeAnnotationTrust(
  findings: Finding[],
  platformMetadata?: PlatformMetadata | null,
): {
  provenanceLabel: string;
  supportLabels: string[];
  warningLabels: string[];
} {
  const provenanceLabels: string[] = [];
  const supportLabels: string[] = [];
  const warningLabels: string[] = [];

  for (const finding of findings) {
    const trust = buildFindingTrustViewModel(finding, {
      platformMetadata: platformMetadata ?? null,
    });
    provenanceLabels.push(...trust.provenanceBadges.map((badge) => badge.label));
    supportLabels.push(...trust.supportBadges.map((badge) => badge.label));
    warningLabels.push(...trust.warningBadges.map((badge) => badge.label));
  }

  return {
    provenanceLabel: dedupeLabels(provenanceLabels).join(" · "),
    supportLabels: dedupeLabels(supportLabels),
    warningLabels: dedupeLabels(warningLabels),
  };
}
