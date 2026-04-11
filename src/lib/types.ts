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
