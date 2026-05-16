use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlatformId {
    #[serde(rename = "github")]
    GitHub,
    #[serde(rename = "gitlab")]
    GitLab,
    #[serde(rename = "bitbucket")]
    Bitbucket,
}

impl PlatformId {
    pub fn as_str(self) -> &'static str {
        match self {
            PlatformId::GitHub => "github",
            PlatformId::GitLab => "gitlab",
            PlatformId::Bitbucket => "bitbucket",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlatformCapabilityKey {
    PrMetadata,
    DiffFetch,
    FileContent,
    IssueContext,
    ReviewSummaryComment,
    InlineComment,
    ApproveReview,
    RequestChangesReview,
    PendingCommentBatch,
    SuggestionMarkup,
    ReviewerMetadata,
    WebhookNotifications,
}

impl PlatformCapabilityKey {
    pub fn as_str(self) -> &'static str {
        match self {
            PlatformCapabilityKey::PrMetadata => "pr_metadata",
            PlatformCapabilityKey::DiffFetch => "diff_fetch",
            PlatformCapabilityKey::FileContent => "file_content",
            PlatformCapabilityKey::IssueContext => "issue_context",
            PlatformCapabilityKey::ReviewSummaryComment => "review_summary_comment",
            PlatformCapabilityKey::InlineComment => "inline_comment",
            PlatformCapabilityKey::ApproveReview => "approve_review",
            PlatformCapabilityKey::RequestChangesReview => "request_changes_review",
            PlatformCapabilityKey::PendingCommentBatch => "pending_comment_batch",
            PlatformCapabilityKey::SuggestionMarkup => "suggestion_markup",
            PlatformCapabilityKey::ReviewerMetadata => "reviewer_metadata",
            PlatformCapabilityKey::WebhookNotifications => "webhook_notifications",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilitySupport {
    Full,
    Partial,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityConstraint {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityFallback {
    pub action: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCapability {
    pub key: PlatformCapabilityKey,
    pub support: CapabilitySupport,
    #[serde(default)]
    pub constraints: Vec<CapabilityConstraint>,
    #[serde(default)]
    pub fallback: Option<CapabilityFallback>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformCapabilities {
    pub platform: PlatformId,
    pub capabilities: Vec<PlatformCapability>,
}

impl PlatformCapabilities {
    pub fn get(&self, key: PlatformCapabilityKey) -> Option<&PlatformCapability> {
        self.capabilities.iter().find(|cap| cap.key == key)
    }
}

#[derive(Debug, Clone)]
pub struct PlatformReviewSnapshot {
    pub review_target: PlatformReviewTarget,
    pub metadata: PlatformMetadata,
    pub diff_text: String,
    pub capabilities: PlatformCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewerStatus {
    pub login: String,
    pub display_name: Option<String>,
    pub state: String,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformReviewTarget {
    pub title: String,
    pub author: Option<String>,
    pub base_branch: Option<String>,
    pub head_branch: Option<String>,
}

/// Platform-agnostic metadata snapshot stored as `platform_metadata_json`.
/// Uses a discriminated union (`platform` field) so the frontend can branch rendering.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "platform")]
pub enum PlatformMetadata {
    #[serde(rename = "github")]
    GitHub(GitHubMeta),
    #[serde(rename = "gitlab")]
    GitLab(GitLabMeta),
    #[serde(rename = "bitbucket")]
    Bitbucket(BitbucketMeta),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubMeta {
    pub pr_body: Option<String>,
    pub head_sha: String,
    pub base_sha: String,
    pub base_ref: String,
    pub head_ref: String,
    pub draft: bool,
    pub labels: Vec<String>,
    pub requested_reviewers: Vec<String>,
    pub requested_teams: Vec<String>,
    pub review_state_summary: Vec<ReviewState>,
    pub linked_issue_numbers: Vec<i64>,
    pub text_issue_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitLabMeta {
    pub mr_body: Option<String>,
    pub head_sha: String,
    pub base_sha: String,
    pub base_ref: String,
    pub head_ref: String,
    pub draft: bool,
    pub labels: Vec<String>,
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub reviewer_statuses: Vec<ReviewerStatus>,
    pub approval_status: Option<ApprovalInfo>,
    pub closes_issues: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BitbucketMeta {
    pub pr_body: Option<String>,
    pub head_sha: String,
    pub base_sha: String,
    pub head_ref: String,
    pub base_ref: String,
    pub draft: bool,
    pub labels: Vec<String>,
    pub reviewers: Vec<String>,
    #[serde(default)]
    pub reviewer_statuses: Vec<ReviewerStatus>,
    pub approval_status: Option<ApprovalInfo>,
    pub default_reviewers: Vec<String>,
    pub jira_issue_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewState {
    pub login: String,
    pub state: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalInfo {
    pub approved: bool,
    pub approved_by: Vec<String>,
    pub approvals_required: Option<i32>,
    pub approvals_left: Option<i32>,
}

/// Issue context hydrated from the platform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueContext {
    pub number: i64,
    pub title: String,
    pub body_excerpt: Option<String>,
    pub labels: Vec<String>,
    #[serde(default)]
    pub state: Option<String>,
}

/// Submission result from posting a review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmissionResult {
    pub review_id: Option<String>,
    pub url: Option<String>,
    pub notes_created: usize,
}

/// A single inline comment for submission.
#[derive(Debug, Clone)]
pub struct InlineComment {
    pub path: String,
    pub body: String,
    pub line: Option<i32>,
    pub side: Option<String>,
    pub start_line: Option<i32>,
}

/// Submission payload (top-level body + inline comments + event).
#[derive(Debug, Clone)]
pub struct SubmissionPayload {
    pub body: String,
    pub event: String,
    pub inline_comments: Vec<InlineComment>,
    pub commit_id: String,
}

/// Minimal platform adapter contract.
/// Each platform implements this trait to normalize the reviewer loop.
#[async_trait]
pub trait PlatformAdapter: Send + Sync {
    #[allow(dead_code)]
    fn platform_id(&self) -> PlatformId;

    fn platform_name(&self) -> &'static str;

    fn capabilities(&self) -> PlatformCapabilities;

    async fn fetch_review_snapshot(&self) -> Result<PlatformReviewSnapshot, AppError> {
        Ok(PlatformReviewSnapshot {
            review_target: self.fetch_review_target().await?,
            metadata: self.fetch_metadata().await?,
            diff_text: self.fetch_diff().await?,
            capabilities: self.capabilities(),
        })
    }

    async fn fetch_review_target(&self) -> Result<PlatformReviewTarget, AppError>;

    async fn fetch_metadata(&self) -> Result<PlatformMetadata, AppError>;

    async fn fetch_diff(&self) -> Result<String, AppError>;

    async fn fetch_issue_context(
        &self,
        issue_ids: &[i64],
        max_issues: usize,
    ) -> Result<Vec<IssueContext>, AppError>;

    async fn fetch_file_content(
        &self,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<String>, AppError>;

    async fn submit_review(&self, payload: SubmissionPayload)
        -> Result<SubmissionResult, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_id_serialization_uses_public_identifiers() {
        assert_eq!(
            serde_json::to_string(&PlatformId::GitHub).unwrap(),
            "\"github\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformId::GitLab).unwrap(),
            "\"gitlab\""
        );
        assert_eq!(
            serde_json::to_string(&PlatformId::Bitbucket).unwrap(),
            "\"bitbucket\""
        );
    }

    #[test]
    fn test_platform_capabilities_round_trip_with_public_platform_ids() {
        let json = r#"{
            "platform":"github",
            "capabilities":[
                {"key":"pr_metadata","support":"full","constraints":[],"fallback":null},
                {"key":"diff_fetch","support":"partial","constraints":[],"fallback":null}
            ]
        }"#;

        let parsed: PlatformCapabilities = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.platform, PlatformId::GitHub);
        assert_eq!(
            parsed
                .get(PlatformCapabilityKey::DiffFetch)
                .map(|cap| cap.support),
            Some(CapabilitySupport::Partial)
        );
    }

    #[test]
    fn test_platform_metadata_github_serialization() {
        let meta = PlatformMetadata::GitHub(GitHubMeta {
            pr_body: Some("Fix auth".into()),
            head_sha: "abc".into(),
            base_sha: "def".into(),
            base_ref: "main".into(),
            head_ref: "fix".into(),
            draft: false,
            labels: vec!["bug".into()],
            requested_reviewers: vec!["alice".into()],
            requested_teams: vec![],
            review_state_summary: vec![],
            linked_issue_numbers: vec![1],
            text_issue_refs: vec!["#1".into()],
        });
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"platform\":\"github\""));
        let deser: PlatformMetadata = serde_json::from_str(&json).unwrap();
        match deser {
            PlatformMetadata::GitHub(g) => assert_eq!(g.head_sha, "abc"),
            _ => panic!("Expected GitHub variant"),
        }
    }

    #[test]
    fn test_platform_metadata_bitbucket_serialization() {
        let meta = PlatformMetadata::Bitbucket(BitbucketMeta {
            pr_body: Some("Fix login".into()),
            head_sha: "aaa".into(),
            base_sha: "bbb".into(),
            head_ref: "feature/login".into(),
            base_ref: "main".into(),
            draft: false,
            labels: vec![],
            reviewers: vec!["alice".into()],
            reviewer_statuses: vec![],
            approval_status: Some(ApprovalInfo {
                approved: true,
                approved_by: vec!["bob".into()],
                approvals_required: None,
                approvals_left: None,
            }),
            default_reviewers: vec!["charlie".into()],
            jira_issue_keys: vec!["AUTH-42".into()],
        });
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"platform\":\"bitbucket\""));
        assert!(json.contains("AUTH-42"));
        let deser: PlatformMetadata = serde_json::from_str(&json).unwrap();
        match deser {
            PlatformMetadata::Bitbucket(b) => {
                assert_eq!(b.head_sha, "aaa");
                assert_eq!(b.jira_issue_keys, vec!["AUTH-42"]);
                assert_eq!(b.reviewers, vec!["alice"]);
                assert!(b.approval_status.unwrap().approved);
            }
            _ => panic!("Expected Bitbucket variant"),
        }
    }

    #[test]
    fn test_platform_metadata_gitlab_serialization() {
        let meta = PlatformMetadata::GitLab(GitLabMeta {
            mr_body: Some("Fix MR".into()),
            head_sha: "abc".into(),
            base_sha: "def".into(),
            base_ref: "main".into(),
            head_ref: "fix".into(),
            draft: true,
            labels: vec![],
            reviewers: vec!["bob".into()],
            reviewer_statuses: vec![],
            approval_status: Some(ApprovalInfo {
                approved: false,
                approved_by: vec![],
                approvals_required: Some(2),
                approvals_left: Some(2),
            }),
            closes_issues: vec![5],
        });
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"platform\":\"gitlab\""));
        let deser: PlatformMetadata = serde_json::from_str(&json).unwrap();
        match deser {
            PlatformMetadata::GitLab(g) => {
                assert_eq!(g.head_sha, "abc");
                assert!(g.approval_status.unwrap().approvals_required == Some(2));
            }
            _ => panic!("Expected GitLab variant"),
        }
    }
}
