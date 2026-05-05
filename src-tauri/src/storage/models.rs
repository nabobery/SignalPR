use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub local_path: String,
    pub remote_owner: String,
    pub remote_repo: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    pub id: String,
    pub workspace_id: String,
    pub pr_number: i32,
    pub title: String,
    pub author: Option<String>,
    pub base_branch: Option<String>,
    pub head_branch: Option<String>,
    pub url: String,
    pub diff_text: Option<String>,
    pub changed_files: Option<String>,
    pub fetched_at: String,
    // V3 fields
    pub diff_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: String,
    pub pr_id: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    // V7 fields: trust metrics + incremental rerun
    pub baseline_run_id: Option<String>,
    pub metrics_json: Option<String>,
    pub analysis_diff_hash: Option<String>,
    pub analysis_diff_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: String,
    pub review_run_id: String,
    pub agent_type: String,
    pub file_path: Option<String>,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub severity: String,
    pub confidence: f64,
    pub title: String,
    pub body: String,
    pub evidence: Option<String>,
    pub status: String,
    pub user_edited_body: Option<String>,
    pub user_severity_override: Option<String>,
    pub is_anchored: bool,
    pub created_at: String,
    // V2 fields (nullable for backward compat with Phase 1 data)
    pub cluster_id: Option<String>,
    pub lane_id: Option<String>,
    pub provider_name: Option<String>,
    pub diff_side: Option<String>,
    pub diff_new_line: Option<i32>,
    // V4 fields: auto-fix
    pub fix_search: Option<String>,
    pub fix_replace: Option<String>,
    pub fix_explanation: Option<String>,
    pub fix_status: Option<String>,
    // V7 field: stable identity across reruns
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRun {
    pub id: String,
    pub review_run_id: String,
    pub lane_id: String,
    pub provider_name: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub finding_count: i32,
    pub error_message: Option<String>,
    // V5 fields: session metadata
    pub governance_tier_at_run: Option<String>,
    pub provider_session_id: Option<String>,
    pub resume_cursor: Option<String>,
    pub checkpoint_metadata_json: Option<String>,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingCluster {
    pub id: String,
    pub review_run_id: String,
    pub label: Option<String>,
    pub representative_finding_id: Option<String>,
    pub member_count: i32,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionRecord {
    pub id: String,
    pub review_run_id: String,
    pub review_action: String,
    pub submitted_at: Option<String>,
    pub status: String,
    pub gh_review_id: Option<String>,
    pub error_message: Option<String>,
    // V3 fields
    pub idempotency_key: Option<String>,
    pub attempt_count: Option<i32>,
    pub last_attempt_at: Option<String>,
}

// V4 models: Preference learning

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerDecision {
    pub id: String,
    pub finding_id: String,
    pub review_run_id: String,
    pub decision: String, // "accept" | "reject" | "edit" | "skip"
    pub original_severity: String,
    pub original_agent_type: String,
    pub category_tag: Option<String>,
    pub time_to_decision_ms: Option<i64>,
    pub decided_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceSummary {
    pub id: String,
    pub agent_type: String,
    pub category_tag: Option<String>,
    pub accept_rate: f64,
    pub total_decisions: i32,
    pub last_updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    pub tool_name: String,
    pub status: String,
    pub version: Option<String>,
    pub message: Option<String>,
    pub checked_at: String,
}

// V6 model: Draft Review persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewDraft {
    pub run_id: String,
    pub summary_markdown: String,
    pub review_action: String,
    pub updated_at: String,
}

// Inbox enriched review row (not persisted; composed in queries)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxReviewRow {
    pub run_id: String,
    pub pr_id: String,
    pub pr_number: i32,
    pub title: String,
    pub author: Option<String>,
    pub pr_url: String,
    pub status: String,
    pub last_updated: String,
    pub active_finding_count: i32,
    pub providers_used: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxWorkspaceRow {
    pub workspace_id: String,
    pub local_path: String,
    pub remote_owner: String,
    pub remote_repo: String,
    pub last_reviewed_at: String,
}
