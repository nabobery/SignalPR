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
}

export type ReviewAction = "approve" | "comment" | "request-changes";
