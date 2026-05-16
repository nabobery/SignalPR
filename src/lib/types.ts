export interface ToolStatus {
  tool_name: string;
  status: "ready" | "degraded" | "missing" | "unauthenticated" | "incomplete";
  version: string | null;
  message: string | null;
  checked_at: string;
}

export interface PrIntakeResult {
  pr_id: string;
  owner: string;
  repo: string;
  pr_number: number;
  title: string;
  author: string | null;
  base_branch: string | null;
  head_branch: string | null;
  changed_file_count: number;
  workspace_suggestion: string | null;
}

export interface PullRequest {
  id: string;
  workspace_id: string;
  pr_number: number;
  title: string;
  author: string | null;
  base_branch: string | null;
  head_branch: string | null;
  url: string;
  diff_text: string | null;
  changed_files: string | null;
  fetched_at: string;
}

export interface Finding {
  id: string;
  review_run_id: string;
  agent_type: string;
  file_path: string | null;
  line_start: number | null;
  line_end: number | null;
  severity: "blocker" | "critical" | "warning" | "info" | "nitpick";
  confidence: number;
  title: string;
  body: string;
  evidence: string | null;
  status: "active" | "suppressed" | "edited";
  user_edited_body: string | null;
  user_severity_override: string | null;
  is_anchored: boolean;
  created_at: string;
  // Review metadata
  cluster_id: string | null;
  lane_id: string | null;
  provider_name: string | null;
  diff_side: string | null;
  diff_new_line: number | null;
  // Suggested fix details
  fix_search: string | null;
  fix_replace: string | null;
  fix_explanation: string | null;
  fix_status: FixStatus | null;
  // Stable identity and rerun state
  fingerprint: string | null;
  delta_state?: "new" | "unchanged" | "stale";
  baseline_finding_id?: string | null;
  baseline_decision?: "accept" | "reject" | "edit" | "skip" | null;
  // Analysis provenance and explainability
  source_kind: string | null;
  source_id: string | null;
  explain_json: string | null;
}

// Explainability payload stored per finding
export interface FindingExplanation {
  schema_version?: number;
  origin: {
    source_kind: string;
    source_id: string | null;
    lane_id: string;
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

// Context pack summary stored on review run
export interface ContextPackSummary {
  total_bytes: number;
  item_count: number;
  items: ContextPackItem[];
  prompt_suffix: string;
}

export interface ContextPackItem {
  kind: string;
  label: string;
  source: string;
  bytes: number;
  included: boolean;
  omit_reason?: string;
  content?: string;
  confidence?: string;
}

// Local checks summary stored on review run
export interface LocalChecksSummary {
  total_errors: number;
  included_count: number;
  tools_run: string[];
  items: LocalCheckItem[];
}

export interface LocalCheckItem {
  tool: string;
  file: string;
  line: number | null;
  column: number | null;
  severity: string;
  message: string;
  rule_id: string | null;
}

export interface LaneSnapshot {
  lane_id: string;
  status: string;
  finding_count: number;
  provider_name: string;
  error_message: string | null;
}

export interface FindingCluster {
  id: string;
  review_run_id: string;
  label: string | null;
  representative_finding_id: string | null;
  member_count: number;
  created_at: string;
}

export interface ReviewRun {
  id: string;
  pr_id: string;
  status: string;
  started_at: string | null;
  completed_at: string | null;
  error_message: string | null;
  baseline_run_id: string | null;
}

// Scorecard metrics
export interface LaneScorecard {
  lane_id: string;
  provider_name: string;
  lane_latency_ms: number | null;
  raw_findings_count: number;
  surfaced_findings_count: number;
  reviewer_accept_rate: number;
  reviewer_edit_rate: number;
  suppress_rate: number;
  anchor_validity: number;
  submission_inclusion_rate: number;
  cost_usd: number | null;
}

export interface RunScorecard {
  lanes: LaneScorecard[];
  overall_surfaced: number;
  overall_accept_rate: number;
  overall_edit_rate: number;
  overall_suppress_rate: number;
}

// Rerun delta
export interface ReviewDeltaSnapshot {
  changed_files: string[];
  changed_hunks_by_file: Record<string, { new_start: number; new_count: number }[]>;
  counts: { new: number; unchanged: number; stale: number; resolved: number };
  resolved: {
    id: string;
    title: string;
    file_path: string | null;
    agent_type: string;
    severity: string;
  }[];
}

export interface ReviewFreshnessSummary {
  is_rerun: boolean;
  baseline_run_id: string | null;
  reviewed_head_sha: string | null;
  current_head_sha: string | null;
  head_changed_since_review: boolean;
  rerun_trigger_source: "workspace" | "queue" | null;
  rerun_reason: "manual" | "head_updated" | "metadata_refresh" | null;
  rerun_scope: "full_pr" | null;
}

export interface ReviewSnapshot {
  run_id: string;
  pr_id: string;
  status: string;
  pr_title: string;
  pr_number: number;
  pr_url: string;
  diff_text: string | null;
  changed_files: string[];
  findings: Finding[];
  error_message: string | null;
  lane_statuses: LaneSnapshot[];
  clusters: FindingCluster[];
  baseline_run_id: string | null;
  metrics: RunScorecard | null;
  delta: ReviewDeltaSnapshot | null;
  review_freshness: ReviewFreshnessSummary;
  decisions_by_finding_id: Record<string, string> | null;
  // Analysis artifacts
  context_pack_summary: ContextPackSummary | null;
  local_checks_summary: LocalChecksSummary | null;
  // Platform metadata
  platform_metadata: PlatformMetadata | null;
  platform_metadata_fetched_at: string | null;
}

// Platform metadata across supported review hosts
export type PlatformMetadata = GitHubMetadata | GitLabMetadata | BitbucketMetadata;

export interface GitHubMetadata {
  platform: "github";
  pr_body: string | null;
  head_sha: string;
  base_sha: string;
  base_ref: string;
  head_ref: string;
  draft: boolean;
  labels: string[];
  requested_reviewers: string[];
  requested_teams: string[];
  review_state_summary: ReviewStateSummary[];
  linked_issue_numbers: number[];
  text_issue_refs: string[];
}

export interface GitLabMetadata {
  platform: "gitlab";
  mr_body: string | null;
  head_sha: string;
  base_sha: string;
  base_ref: string;
  head_ref: string;
  draft: boolean;
  labels: string[];
  reviewers: string[];
  approval_status: ApprovalInfo | null;
  closes_issues: number[];
}

export interface BitbucketMetadata {
  platform: "bitbucket";
  pr_body: string | null;
  head_sha: string;
  base_sha: string;
  head_ref: string;
  base_ref: string;
  draft: boolean;
  labels: string[];
  reviewers: string[];
  approval_status: ApprovalInfo | null;
  default_reviewers: string[];
  jira_issue_keys: string[];
}

export interface ApprovalInfo {
  approved: boolean;
  approved_by: string[];
  approvals_required: number | null;
  approvals_left: number | null;
}

export interface ReviewStateSummary {
  login: string;
  state: string;
  submitted_at: string | null;
}

/** Type guard for GitHub metadata */
export function isGitHubMetadata(meta: unknown): meta is GitHubMetadata {
  if (!meta || typeof meta !== "object") return false;
  const candidate = meta as Partial<GitHubMetadata>;
  return (
    candidate.platform === "github" &&
    typeof candidate.head_sha === "string" &&
    typeof candidate.base_sha === "string" &&
    Array.isArray(candidate.requested_reviewers)
  );
}

/** Type guard for GitLab metadata */
export function isGitLabMetadata(meta: unknown): meta is GitLabMetadata {
  if (!meta || typeof meta !== "object") return false;
  const candidate = meta as Partial<GitLabMetadata>;
  return (
    candidate.platform === "gitlab" &&
    typeof candidate.head_sha === "string" &&
    typeof candidate.base_sha === "string" &&
    Array.isArray(candidate.reviewers)
  );
}

/** Type guard for Bitbucket metadata */
export function isBitbucketMetadata(meta: unknown): meta is BitbucketMetadata {
  if (!meta || typeof meta !== "object") return false;
  const candidate = meta as Partial<BitbucketMetadata>;
  return (
    candidate.platform === "bitbucket" &&
    typeof candidate.head_sha === "string" &&
    typeof candidate.base_sha === "string" &&
    Array.isArray(candidate.reviewers)
  );
}

export interface RefreshMetadataResult {
  pr_id: string;
  fetched_at: string;
  metadata: PlatformMetadata;
}

export interface ReviewerDecision {
  id: string;
  finding_id: string;
  review_run_id: string;
  decision: "accept" | "reject" | "edit" | "skip";
  original_severity: string;
  original_agent_type: string;
  category_tag: string | null;
  time_to_decision_ms: number | null;
  decided_at: string;
}

export interface PreferenceSummary {
  id: string;
  agent_type: string;
  category_tag: string | null;
  accept_rate: number;
  total_decisions: number;
  last_updated: string;
}

export type FixStatus = "none" | "pending" | "applied" | "accepted" | "rejected";

export type ReviewAction = "approve" | "comment" | "request-changes";

export interface AgentDefinition {
  name: string;
  system_prompt: string;
  agent_type: string;
  provider: string | null;
  is_builtin: boolean;
}

export interface AgentDefinitionsResponse {
  agents: AgentDefinition[];
}

export interface ChannelStatus {
  source: string;
  connected: boolean;
  message: string | null;
}

export interface ChannelEvent {
  source: string;
  pr_url: string;
  requester: string | null;
  channel: string | null;
  received_at: string;
}

export interface AppError {
  code: string;
  message: string;
}

// Codex App Server approval types
export interface CodexApprovalRequest {
  request_id: unknown;
  method: string;
  thread_id: string;
  turn_id: string;
  item_id: string;
  params: Record<string, unknown>;
}

// Codex App Server streaming delta event
export interface CodexLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// Copilot permission request
export interface CopilotPermissionRequest {
  session_id: string;
  event_id: string;
  kind: string;
  command: string | null;
  file_name: string | null;
  event: Record<string, unknown>;
}

// Copilot streaming delta event
export interface CopilotLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// OpenCode permission request
export interface OpenCodePermissionRequest {
  session_id: string;
  request_id: string;
  permission: string;
  patterns: string[];
  metadata: Record<string, unknown>;
  tool: string | null;
}

// OpenCode streaming delta event
export interface OpenCodeLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// Gemini (ACP) permission request — currently observational only:
// the backend denies every tool request by selecting the agent's
// `reject_once` option before broadcasting the attempt to the UI, so
// the card is a "what did the agent just try to do" log, not an
// actionable prompt. A future PR will gate the ACP response on a user
// decision via `resolve_gemini_permission`.
export interface GeminiPermissionRequest {
  session_id: string;
  request_id: string;
  tool_call: Record<string, unknown>;
  options: unknown;
}

// Gemini ACP streaming delta event
export interface GeminiLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// Cursor (ACP) permission request — currently observational only:
// the backend denies every tool request by selecting the agent's
// `reject_once` option and broadcasts the attempt so the UI can
// surface what the agent tried to do. A future PR will gate the ACP
// response on a user decision via `resolve_cursor_permission`.
export interface CursorPermissionRequest {
  session_id: string;
  request_id: string;
  tool_call: Record<string, unknown>;
  options: unknown;
}

// Cursor ACP streaming delta event
export interface CursorLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// PI Agent SDK streaming delta event
export interface PiLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// Claude Code streaming delta event
export interface ClaudeCodeLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// Claude Code permission request (observational in v1)
export interface ClaudeCodePermissionRequest {
  lane_id: string;
  tool_name: string;
  tool_input: unknown;
  reason: string;
  action: string;
  request_id?: string;
}

// --- Inbox Overview ---

export interface InboxReviewRow {
  run_id: string;
  pr_id: string;
  pr_number: number;
  title: string;
  author: string | null;
  pr_url: string;
  status: string;
  last_updated: string;
  active_finding_count: number;
  providers_used: string[];
  queue_state: string;
  platform: string;
  repo_owner: string;
  repo_name: string;
  remote_host: string;
  workspace_id: string;
  workspace_path: string;
  draft: boolean;
  has_saved_review_draft: boolean;
  metadata_freshness: InboxMetadataFreshness;
  review_freshness: InboxReviewFreshness;
  reviewer_signal: InboxReviewerSignal;
  lane_health: InboxLaneHealth;
  submission_health: InboxSubmissionHealth;
  attention_reasons: string[];
  allowed_actions: string[];
}

export interface InboxWorkspaceRow {
  workspace_id: string;
  local_path: string;
  remote_owner: string;
  remote_repo: string;
  last_reviewed_at: string;
}

export interface EnvironmentSummary {
  can_review: boolean;
  can_submit: boolean;
  available_providers: string[];
  warnings: string[];
  tools: ToolStatus[];
}

export interface InboxMetadataFreshness {
  fetched_at: string | null;
  is_stale: boolean;
}

export interface InboxReviewFreshness {
  state: "current" | "stale";
  reviewed_at: string | null;
  reviewed_head_sha: string | null;
  current_head_sha: string | null;
  has_unreviewed_updates: boolean;
}

export interface InboxReviewerSignal {
  has_signal: boolean;
  label: string;
  precision: string;
  requested_reviewers: string[];
  requested_teams: string[];
}

export interface InboxLaneHealth {
  state: string;
  failed_count: number;
  timed_out_count: number;
  running_count: number;
  completed_count: number;
}

export interface InboxSubmissionHealth {
  state: string;
  submitted_at: string | null;
  review_action: string | null;
  commit_id: string | null;
  error_message: string | null;
}

export interface InboxSection {
  id: string;
  title: string;
  items: InboxReviewRow[];
}

export interface InboxAttentionSummary {
  total_items: number;
  failed_runs: number;
  failed_submissions: number;
  stale_metadata: number;
  degraded_runs: number;
}

export interface InboxOverview {
  environment_summary: EnvironmentSummary;
  attention_summary: InboxAttentionSummary;
  sections: InboxSection[];
  recent_workspaces: InboxWorkspaceRow[];
}

// --- Review Draft ---

export interface ReviewDraft {
  run_id: string;
  summary_markdown: string;
  review_action: ReviewAction;
  updated_at: string;
}

// --- Provider Credential Platform ---

export type CredentialSource = "environment" | "keychain" | "none";

export type ProviderCredentialField =
  | "anthropic_api_key"
  | "gemini_api_key"
  | "google_api_key"
  | "cursor_api_key"
  | "opencode_server_password";

export interface CredentialStatus {
  field: ProviderCredentialField;
  source: CredentialSource;
  provider_ids: string[];
}

// --- Provider Capability Registry ---

export type ToolGovernanceTier = "read_only" | "guarded_write" | "trusted_write";

export interface ProviderCapabilities {
  provider_id: string;
  display_name: string;
  opt_in_only: boolean;
  in_auto_fallback: boolean;
  credential_fields: { provider_id: string; field: string; env_var: string }[];
  interactive_permissions: boolean;
  default_governance_tier: ToolGovernanceTier;
  supports_session_resume: boolean;
  supports_checkpointing: boolean;
  paid_eval_eligible: boolean;
}

// --- Session Metadata ---

export interface AgentRunMetadata {
  id: string;
  review_run_id: string;
  lane_id: string;
  provider_name: string;
  governance_tier_at_run: ToolGovernanceTier | null;
  provider_session_id: string | null;
  resume_cursor: string | null;
  checkpoint_metadata_json: string | null;
  cost_usd: number | null;
  started_at: string;
  completed_at: string | null;
  status: string;
  finding_count: number;
}
