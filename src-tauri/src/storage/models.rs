use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub local_path: String,
    pub remote_owner: String,
    pub remote_repo: String,
    pub created_at: String,
    #[serde(default = "default_remote_host")]
    pub remote_host: String,
}

fn default_remote_host() -> String {
    "github.com".to_string()
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
    // Platform metadata snapshot
    pub platform_metadata_json: Option<String>,
    pub platform_metadata_fetched_at: Option<String>,
    pub platform_capabilities_json: Option<String>,
    pub platform_capabilities_fetched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewRun {
    pub id: String,
    pub pr_id: String,
    pub status: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub head_sha_at_run: Option<String>,
    // Review metrics and rerun comparison fields
    pub baseline_run_id: Option<String>,
    pub metrics_json: Option<String>,
    pub analysis_diff_hash: Option<String>,
    pub analysis_diff_text: Option<String>,
    // Context and local check artifacts
    pub context_pack_json: Option<String>,
    pub local_checks_json: Option<String>,
    pub provider_selection_json: Option<String>,
    // Rerun metadata
    pub rerun_trigger_source: Option<String>,
    pub rerun_reason: Option<String>,
    pub rerun_scope: Option<String>,
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
    // Queueing and clustering fields kept nullable for backward compatibility
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
    // Stable identity across reruns
    pub fingerprint: Option<String>,
    // Analysis provenance and explainability
    pub source_kind: Option<String>,
    pub source_id: Option<String>,
    pub explain_json: Option<String>,
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
    pub commit_id_at_submission: Option<String>,
    pub platform_review_id: Option<String>,
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
pub struct InboxMetadataFreshness {
    pub fetched_at: Option<String>,
    pub is_stale: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxReviewerSignal {
    pub has_signal: bool,
    pub label: String,
    pub precision: String,
    pub requested_reviewers: Vec<String>,
    pub requested_teams: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxLaneHealth {
    pub state: String,
    pub failed_count: i32,
    pub timed_out_count: i32,
    pub running_count: i32,
    pub completed_count: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxSubmissionHealth {
    pub state: String,
    pub submitted_at: Option<String>,
    pub review_action: Option<String>,
    pub commit_id: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxReviewFreshness {
    pub state: String,
    pub reviewed_at: Option<String>,
    pub reviewed_head_sha: Option<String>,
    pub current_head_sha: Option<String>,
    pub has_unreviewed_updates: bool,
}

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
    pub queue_state: String,
    pub platform: String,
    pub repo_owner: String,
    pub repo_name: String,
    pub remote_host: String,
    pub workspace_id: String,
    pub workspace_path: String,
    pub draft: bool,
    pub has_saved_review_draft: bool,
    pub metadata_freshness: InboxMetadataFreshness,
    pub platform_capabilities: Option<crate::platform::adapter::PlatformCapabilities>,
    pub platform_capabilities_fetched_at: Option<String>,
    pub review_freshness: InboxReviewFreshness,
    pub reviewer_signal: InboxReviewerSignal,
    pub lane_health: InboxLaneHealth,
    pub submission_health: InboxSubmissionHealth,
    pub attention_reasons: Vec<String>,
    pub allowed_actions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxWorkspaceRow {
    pub workspace_id: String,
    pub local_path: String,
    pub remote_owner: String,
    pub remote_repo: String,
    pub last_reviewed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxSection {
    pub id: String,
    pub title: String,
    pub items: Vec<InboxReviewRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboxAttentionSummary {
    pub total_items: i32,
    pub failed_runs: i32,
    pub failed_submissions: i32,
    pub stale_metadata: i32,
    pub degraded_runs: i32,
}
