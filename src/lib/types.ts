export interface ToolStatus {
  tool_name: string;
  status: "ready" | "degraded" | "missing" | "unauthenticated";
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
  // V2 fields
  cluster_id: string | null;
  lane_id: string | null;
  provider_name: string | null;
  diff_side: string | null;
  diff_new_line: number | null;
  // V4 fields: auto-fix
  fix_search: string | null;
  fix_replace: string | null;
  fix_explanation: string | null;
  fix_status: FixStatus | null;
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
}

export interface ReviewSnapshot {
  run_id: string;
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

// Phase 4: Codex App Server approval types
export interface CodexApprovalRequest {
  request_id: unknown;
  method: string;
  thread_id: string;
  turn_id: string;
  item_id: string;
  params: Record<string, unknown>;
}

// Phase 4: Codex App Server streaming delta event
export interface CodexLaneDelta {
  lane_id: string;
  delta: string;
  buffer: string;
}

// Copilot SDK v3 permission request
export interface CopilotPermissionRequest {
  session_id: string;
  event_id: string;
  kind: string;
  command: string | null;
  file_name: string | null;
  event: Record<string, unknown>;
}

// Copilot SDK streaming delta event
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

export interface InboxOverview {
  environment_summary: EnvironmentSummary;
  incomplete_reviews: InboxReviewRow[];
  recent_reviews: InboxReviewRow[];
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
