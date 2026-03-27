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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: String,
    pub pr_id: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolStatus {
    pub tool_name: String,
    pub status: String,
    pub version: Option<String>,
    pub message: Option<String>,
    pub checked_at: String,
}
