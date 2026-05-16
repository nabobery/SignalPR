use async_trait::async_trait;
use std::collections::HashSet;

use crate::errors::AppError;
use crate::platform::adapter::*;
use crate::providers::bitbucket::{
    BbPullRequest, BbTaskCommentRef, BitbucketApi, BitbucketApiError, CreateCommentPayload,
    CreateContent, CreateInline, CreateTaskPayload,
};

const INLINE_FINGERPRINT_PREFIX: &str = "<!-- signalpr:fingerprint=";
const INLINE_FINGERPRINT_SUFFIX: &str = " -->";
const SUMMARY_FINGERPRINT_PREFIX: &str = "<!-- signalpr:summary-fingerprint=";
const SUMMARY_FINGERPRINT_SUFFIX: &str = " -->";

pub struct BitbucketAdapter {
    api: BitbucketApi,
    workspace: String,
    repo_slug: String,
    pr_id: i32,
}

impl BitbucketAdapter {
    pub fn new(api: BitbucketApi, workspace: String, repo_slug: String, pr_id: i32) -> Self {
        Self {
            api,
            workspace,
            repo_slug,
            pr_id,
        }
    }

    fn review_target_from_pull_request(pr: &BbPullRequest) -> PlatformReviewTarget {
        PlatformReviewTarget {
            title: pr.title.clone(),
            author: pr.author.as_ref().map(|author| author.best_name()),
            base_branch: Some(pr.destination.branch.name.clone()),
            head_branch: Some(pr.source.branch.name.clone()),
        }
    }

    async fn build_metadata_from_pull_request(
        &self,
        pr: BbPullRequest,
    ) -> Result<PlatformMetadata, AppError> {
        let default_reviewers = self
            .api
            .list_default_reviewers(&self.workspace, &self.repo_slug)
            .await
            .unwrap_or_default();

        let reviewers: Vec<String> = pr.reviewers.iter().map(|u| u.best_name()).collect();

        let approved_by: Vec<String> = pr
            .participants
            .iter()
            .filter(|p| p.approved)
            .map(|p| p.user.best_name())
            .collect();

        let approval_status = Some(ApprovalInfo {
            approved: !approved_by.is_empty(),
            approved_by,
            approvals_required: None,
            approvals_left: None,
        });

        let default_reviewer_names: Vec<String> = default_reviewers
            .iter()
            .map(|r| {
                r.nickname
                    .as_deref()
                    .or(r.display_name.as_deref())
                    .unwrap_or("unknown")
                    .to_string()
            })
            .collect();

        let mut jira_text = pr.title.clone();
        if let Some(desc) = &pr.description {
            jira_text.push(' ');
            jira_text.push_str(desc);
        }
        jira_text.push(' ');
        jira_text.push_str(&pr.source.branch.name);
        jira_text.push(' ');
        jira_text.push_str(&pr.destination.branch.name);
        let jira_issue_keys = extract_jira_keys(&jira_text);

        Ok(PlatformMetadata::Bitbucket(BitbucketMeta {
            pr_body: pr.description,
            head_sha: pr.source.commit.hash,
            base_sha: pr.destination.commit.hash,
            head_ref: pr.source.branch.name,
            base_ref: pr.destination.branch.name,
            draft: pr.draft,
            labels: vec![],
            reviewers,
            reviewer_statuses: pr
                .participants
                .iter()
                .map(|participant| ReviewerStatus {
                    login: participant
                        .user
                        .nickname
                        .clone()
                        .or_else(|| participant.user.account_id.clone())
                        .unwrap_or_else(|| participant.user.best_name()),
                    display_name: participant.user.display_name.clone(),
                    state: participant
                        .state
                        .clone()
                        .or_else(|| participant.role.clone())
                        .unwrap_or_else(|| {
                            if participant.approved {
                                "approved".to_string()
                            } else {
                                "participant".to_string()
                            }
                        }),
                    updated_at: None,
                })
                .collect(),
            approval_status,
            default_reviewers: default_reviewer_names,
            jira_issue_keys,
        }))
    }
}

fn is_old_side(side: Option<&str>) -> bool {
    matches!(side, Some("LEFT" | "left" | "old" | "OLD"))
}

fn build_bitbucket_inline_payload(
    comment: &crate::platform::adapter::InlineComment,
) -> Option<CreateInline> {
    if comment.path.trim().is_empty() {
        return None;
    }
    let line = comment.line?;
    let start = comment.start_line.filter(|l| *l > 0);
    if is_old_side(comment.side.as_deref()) {
        Some(CreateInline {
            path: comment.path.clone(),
            from: Some(line),
            to: None,
            start_from: start,
            start_to: None,
        })
    } else {
        Some(CreateInline {
            path: comment.path.clone(),
            from: None,
            to: Some(line),
            start_from: None,
            start_to: start,
        })
    }
}

fn bb_err(e: BitbucketApiError) -> AppError {
    match e {
        BitbucketApiError::RateLimited { .. } => AppError::Transient(e.to_string()),
        BitbucketApiError::HttpError { status: 404, .. } => AppError::NotFound(e.to_string()),
        BitbucketApiError::HttpError { status, .. } if status >= 500 => {
            AppError::Transient(e.to_string())
        }
        _ => AppError::InvalidInput(e.to_string()),
    }
}

fn extract_marker(body: &str, prefix: &str, suffix: &str) -> Option<String> {
    let start = body.find(prefix)?;
    let marker_start = start + prefix.len();
    let remainder = &body[marker_start..];
    let end_offset = remainder.find(suffix)?;
    let raw = remainder[..end_offset].trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn extract_inline_fingerprint(body: &str) -> Option<String> {
    extract_marker(body, INLINE_FINGERPRINT_PREFIX, INLINE_FINGERPRINT_SUFFIX)
}

fn extract_summary_fingerprint(body: &str) -> Option<String> {
    extract_marker(body, SUMMARY_FINGERPRINT_PREFIX, SUMMARY_FINGERPRINT_SUFFIX)
}

/// Extract Jira issue keys (e.g. `PROJ-123`) from text.
pub fn extract_jira_keys(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\b([A-Z][A-Z0-9]+-\d+)\b").expect("valid jira regex");
    let mut seen = HashSet::new();
    let mut keys = Vec::new();
    for cap in re.captures_iter(text) {
        let key = cap[1].to_string();
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    keys
}

#[async_trait]
impl PlatformAdapter for BitbucketAdapter {
    fn platform_id(&self) -> PlatformId {
        PlatformId::Bitbucket
    }

    fn platform_name(&self) -> &'static str {
        "bitbucket"
    }

    fn capabilities(&self) -> PlatformCapabilities {
        PlatformCapabilities {
            platform: PlatformId::Bitbucket,
            capabilities: vec![
                PlatformCapability {
                    key: PlatformCapabilityKey::PrMetadata,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::DiffFetch,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::FileContent,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::IssueContext,
                    support: CapabilitySupport::None,
                    constraints: vec![CapabilityConstraint {
                        code: "jira_outside_adapter".into(),
                        message: "Bitbucket issue context is not native in SignalPR; Jira hydration runs outside the platform adapter.".into(),
                    }],
                    fallback: Some(CapabilityFallback {
                        action: "jira_hydration".into(),
                        reason: "Use Jira-linked issue context when integration credentials are configured.".into(),
                    }),
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::ReviewSummaryComment,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::InlineComment,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::ApproveReview,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::RequestChangesReview,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::PendingCommentBatch,
                    support: CapabilitySupport::None,
                    constraints: vec![CapabilityConstraint {
                        code: "pending_workflow_not_implemented".into(),
                        message: "SignalPR does not currently maintain a pending Bitbucket review batch prior to submission.".into(),
                    }],
                    fallback: Some(CapabilityFallback {
                        action: "direct_submit".into(),
                        reason: "Submit comments and state changes immediately.".into(),
                    }),
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::SuggestionMarkup,
                    support: CapabilitySupport::Partial,
                    constraints: vec![CapabilityConstraint {
                        code: "markdown_only".into(),
                        message: "Bitbucket suggestions are rendered as plain markdown/code blocks rather than first-class applyable suggestions.".into(),
                    }],
                    fallback: Some(CapabilityFallback {
                        action: "plain_code_block".into(),
                        reason: "Render suggested code as a regular fenced block in the review comment.".into(),
                    }),
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::ReviewerMetadata,
                    support: CapabilitySupport::Full,
                    constraints: vec![],
                    fallback: None,
                },
                PlatformCapability {
                    key: PlatformCapabilityKey::WebhookNotifications,
                    support: CapabilitySupport::None,
                    constraints: vec![CapabilityConstraint {
                        code: "not_integrated".into(),
                        message: "Bitbucket webhook notifications are not yet integrated into SignalPR.".into(),
                    }],
                    fallback: Some(CapabilityFallback {
                        action: "manual_refresh".into(),
                        reason: "Use queue refresh and rerun until notification transport is implemented.".into(),
                    }),
                },
            ],
        }
    }

    async fn fetch_review_snapshot(&self) -> Result<PlatformReviewSnapshot, AppError> {
        let pr = self
            .api
            .get_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
            .await
            .map_err(bb_err)?;
        let review_target = Self::review_target_from_pull_request(&pr);
        let metadata = self.build_metadata_from_pull_request(pr).await?;
        let diff_text = self.fetch_diff().await?;

        Ok(PlatformReviewSnapshot {
            review_target,
            metadata,
            diff_text,
            capabilities: self.capabilities(),
        })
    }

    async fn fetch_review_target(&self) -> Result<PlatformReviewTarget, AppError> {
        let pr = self
            .api
            .get_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
            .await
            .map_err(bb_err)?;

        Ok(Self::review_target_from_pull_request(&pr))
    }

    async fn fetch_metadata(&self) -> Result<PlatformMetadata, AppError> {
        let pr = self
            .api
            .get_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
            .await
            .map_err(bb_err)?;
        self.build_metadata_from_pull_request(pr).await
    }

    async fn fetch_diff(&self) -> Result<String, AppError> {
        self.api
            .get_pull_request_diff(&self.workspace, &self.repo_slug, self.pr_id)
            .await
            .map_err(bb_err)
    }

    async fn fetch_issue_context(
        &self,
        _issue_ids: &[i64],
        _max_issues: usize,
    ) -> Result<Vec<IssueContext>, AppError> {
        // Bitbucket doesn't have native issues; Jira hydration is handled
        // outside the adapter in review.rs to avoid contract redesign.
        Ok(vec![])
    }

    async fn fetch_file_content(
        &self,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<String>, AppError> {
        self.api
            .get_file_content_via_src(&self.workspace, &self.repo_slug, git_ref, path)
            .await
            .map_err(bb_err)
    }

    async fn submit_review(
        &self,
        payload: SubmissionPayload,
    ) -> Result<SubmissionResult, AppError> {
        let event_lower = payload.event.to_lowercase();
        let mut notes_count = 0usize;

        // Fetch existing comments for fingerprint-based dedupe
        let existing_comments = self
            .api
            .list_pull_request_comments(&self.workspace, &self.repo_slug, self.pr_id)
            .await
            .unwrap_or_default();

        let mut existing_inline_fingerprints: HashSet<String> = existing_comments
            .iter()
            .filter_map(|c| extract_inline_fingerprint(&c.content.raw))
            .collect();
        let mut existing_summary_fingerprints: HashSet<String> = existing_comments
            .iter()
            .filter_map(|c| extract_summary_fingerprint(&c.content.raw))
            .collect();
        let mut existing_task_contents: HashSet<String> = self
            .api
            .list_tasks(&self.workspace, &self.repo_slug, self.pr_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|t| t.content.raw.trim().to_string())
            .collect();

        // Post summary comment
        if !payload.body.trim().is_empty() {
            let summary_fingerprint = crate::storage::hashing::sha256_hex(&format!(
                "{}|{}|{}",
                payload.event, payload.commit_id, payload.body
            ));
            if !existing_summary_fingerprints.contains(&summary_fingerprint) {
                let body_with_fp = format!(
                    "{}\n\n{}{}{}",
                    payload.body.trim_end(),
                    SUMMARY_FINGERPRINT_PREFIX,
                    summary_fingerprint,
                    SUMMARY_FINGERPRINT_SUFFIX
                );
                self.api
                    .create_pull_request_comment(
                        &self.workspace,
                        &self.repo_slug,
                        self.pr_id,
                        &CreateCommentPayload {
                            content: CreateContent { raw: body_with_fp },
                            inline: None,
                        },
                    )
                    .await
                    .map_err(bb_err)?;
                existing_summary_fingerprints.insert(summary_fingerprint);
                notes_count += 1;
            }
        }

        // Post inline comments
        let is_request_changes =
            event_lower == "request_changes" || event_lower == "request-changes";
        for comment in &payload.inline_comments {
            let inline_fingerprint =
                extract_inline_fingerprint(&comment.body).unwrap_or_else(|| {
                    crate::storage::hashing::sha256_hex(&format!(
                        "{}|{}|{}",
                        comment.path,
                        comment.line.unwrap_or(0),
                        comment.body
                    ))
                });
            if !existing_inline_fingerprints.insert(inline_fingerprint) {
                continue;
            }

            let inline_payload = CreateCommentPayload {
                content: CreateContent {
                    raw: comment.body.clone(),
                },
                inline: build_bitbucket_inline_payload(comment),
            };

            let created = self
                .api
                .create_pull_request_comment(
                    &self.workspace,
                    &self.repo_slug,
                    self.pr_id,
                    &inline_payload,
                )
                .await
                .map_err(bb_err)?;
            notes_count += 1;

            // For request-changes: create tasks on high-severity inline comments
            if is_request_changes {
                let is_high_severity =
                    comment.body.contains("**[BLOCKER]") || comment.body.contains("**[CRITICAL]");
                if is_high_severity {
                    let task_content = format!(
                        "Address finding on {}:{}",
                        comment.path,
                        comment.line.unwrap_or(0)
                    );
                    if !existing_task_contents.insert(task_content.clone()) {
                        continue;
                    }
                    let _ = self
                        .api
                        .create_task(
                            &self.workspace,
                            &self.repo_slug,
                            self.pr_id,
                            &CreateTaskPayload {
                                content: CreateContent { raw: task_content },
                                comment: Some(BbTaskCommentRef { id: created.id }),
                            },
                        )
                        .await;
                }
            }
        }

        // Handle approval/unapproval
        if event_lower == "approve" {
            let _ = self
                .api
                .remove_change_request_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
                .await;
            self.api
                .approve_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
                .await
                .map_err(bb_err)?;
        } else if is_request_changes {
            self.api
                .request_changes_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
                .await
                .map_err(bb_err)?;
            // Unapprove if we previously approved
            let _ = self
                .api
                .unapprove_pull_request(&self.workspace, &self.repo_slug, self.pr_id)
                .await;
        }

        Ok(SubmissionResult {
            review_id: None,
            url: Some(format!(
                "https://bitbucket.org/{}/{}/pull-requests/{}",
                self.workspace, self.repo_slug, self.pr_id
            )),
            notes_created: notes_count,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_jira_keys_from_text() {
        let text = "PROJ-123: Fix authentication\n\nRelated to AUTH-456 and UI-7";
        let keys = extract_jira_keys(text);
        assert_eq!(keys, vec!["PROJ-123", "AUTH-456", "UI-7"]);
    }

    #[test]
    fn test_extract_jira_keys_deduplicates() {
        let text = "PROJ-123 mentioned twice PROJ-123";
        let keys = extract_jira_keys(text);
        assert_eq!(keys, vec!["PROJ-123"]);
    }

    #[test]
    fn test_extract_jira_keys_no_match() {
        let text = "no issue keys here, just proj-123 lowercase";
        let keys = extract_jira_keys(text);
        assert!(keys.is_empty());
    }

    #[test]
    fn test_extract_jira_keys_from_branch_name() {
        let text = "feature/JIRA-42-add-login";
        let keys = extract_jira_keys(text);
        assert_eq!(keys, vec!["JIRA-42"]);
    }

    #[test]
    fn test_extract_inline_fingerprint() {
        let body = "Some comment\n\n<!-- signalpr:fingerprint=abc123 -->";
        assert_eq!(extract_inline_fingerprint(body), Some("abc123".to_string()));
    }

    #[test]
    fn test_extract_inline_fingerprint_none() {
        let body = "No fingerprint here";
        assert_eq!(extract_inline_fingerprint(body), None);
    }

    #[test]
    fn test_extract_summary_fingerprint() {
        let body = "Summary\n\n<!-- signalpr:summary-fingerprint=def456 -->";
        assert_eq!(
            extract_summary_fingerprint(body),
            Some("def456".to_string())
        );
    }

    #[test]
    fn test_build_bitbucket_inline_payload_new_side() {
        let c = crate::platform::adapter::InlineComment {
            path: "src/main.rs".into(),
            body: "body".into(),
            line: Some(42),
            side: Some("RIGHT".into()),
            start_line: Some(40),
        };
        let inline = build_bitbucket_inline_payload(&c).expect("inline");
        assert_eq!(inline.to, Some(42));
        assert_eq!(inline.from, None);
        assert_eq!(inline.start_to, Some(40));
        assert_eq!(inline.start_from, None);
    }

    #[test]
    fn test_build_bitbucket_inline_payload_old_side() {
        let c = crate::platform::adapter::InlineComment {
            path: "src/main.rs".into(),
            body: "body".into(),
            line: Some(11),
            side: Some("LEFT".into()),
            start_line: Some(9),
        };
        let inline = build_bitbucket_inline_payload(&c).expect("inline");
        assert_eq!(inline.from, Some(11));
        assert_eq!(inline.to, None);
        assert_eq!(inline.start_from, Some(9));
        assert_eq!(inline.start_to, None);
    }

    #[test]
    fn test_build_bitbucket_inline_payload_none_without_line() {
        let c = crate::platform::adapter::InlineComment {
            path: "src/main.rs".into(),
            body: "body".into(),
            line: None,
            side: None,
            start_line: None,
        };
        assert!(build_bitbucket_inline_payload(&c).is_none());
    }
}
