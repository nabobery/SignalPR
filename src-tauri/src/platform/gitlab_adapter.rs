use async_trait::async_trait;

use crate::errors::AppError;
use crate::platform::adapter::*;
use crate::providers::gitlab::{
    CreateDiffNotePayload, DiffNotePosition, GitLabApi, GitLabApiError,
    MAX_ISSUE_BODY_EXCERPT_BYTES,
};

const INLINE_FINGERPRINT_PREFIX: &str = "<!-- signalpr:fingerprint=";
const INLINE_FINGERPRINT_SUFFIX: &str = " -->";
const SUMMARY_FINGERPRINT_PREFIX: &str = "<!-- signalpr:summary-fingerprint=";
const SUMMARY_FINGERPRINT_SUFFIX: &str = " -->";

pub struct GitLabAdapter {
    api: GitLabApi,
    project_path: String,
    iid: i32,
    host: String,
}

impl GitLabAdapter {
    pub fn new(api: GitLabApi, host: String, project_path: String, iid: i32) -> Self {
        Self {
            api,
            project_path,
            iid,
            host,
        }
    }
}

fn gl_err(e: GitLabApiError) -> AppError {
    match e {
        GitLabApiError::RateLimited { .. } => AppError::Transient(e.to_string()),
        GitLabApiError::HttpError { status: 404, .. } => AppError::NotFound(e.to_string()),
        GitLabApiError::HttpError { status, .. } if status >= 500 => {
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

#[async_trait]
impl PlatformAdapter for GitLabAdapter {
    fn platform_name(&self) -> &'static str {
        "gitlab"
    }

    async fn fetch_metadata(&self) -> Result<PlatformMetadata, AppError> {
        let mr = self
            .api
            .get_merge_request(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;

        let approvals = self
            .api
            .get_approvals(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;

        let reviewers = self
            .api
            .list_reviewers(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;

        let closes_issues = self
            .api
            .list_closes_issues(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;

        let diff_refs = mr.diff_refs.as_ref();
        let head_sha = diff_refs
            .and_then(|d| d.head_sha.clone())
            .unwrap_or_default();
        let base_sha = diff_refs
            .and_then(|d| d.base_sha.clone())
            .unwrap_or_default();

        let approval_info = Some(ApprovalInfo {
            approved: approvals.approved,
            approved_by: approvals
                .approved_by
                .into_iter()
                .map(|ab| ab.user.username)
                .collect(),
            approvals_required: approvals.approvals_required,
            approvals_left: approvals.approvals_left,
        });

        Ok(PlatformMetadata::GitLab(GitLabMeta {
            mr_body: mr.description,
            head_sha,
            base_sha,
            base_ref: mr.target_branch,
            head_ref: mr.source_branch,
            draft: mr.draft,
            labels: mr.labels,
            reviewers: reviewers.into_iter().map(|r| r.username).collect(),
            approval_status: approval_info,
            closes_issues: closes_issues.into_iter().map(|i| i.iid).collect(),
        }))
    }

    async fn fetch_diff(&self) -> Result<String, AppError> {
        self.api
            .get_raw_diffs(&self.project_path, self.iid, &self.host)
            .await
            .map_err(gl_err)
    }

    async fn fetch_issue_context(
        &self,
        issue_ids: &[i64],
        max_issues: usize,
    ) -> Result<Vec<IssueContext>, AppError> {
        let mut contexts = Vec::new();
        for &id in issue_ids.iter().take(max_issues) {
            match self.api.get_issue(&self.project_path, id).await {
                Ok(issue) => {
                    let excerpt = issue.description.as_deref().map(|b| {
                        let truncated =
                            crate::context_pack::truncate_utf8(b, MAX_ISSUE_BODY_EXCERPT_BYTES);
                        if truncated.len() < b.len() {
                            format!("{truncated}...")
                        } else {
                            truncated.to_string()
                        }
                    });
                    contexts.push(IssueContext {
                        number: issue.iid,
                        title: issue.title,
                        body_excerpt: excerpt,
                        labels: issue.labels,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch GitLab issue #{}: {}", id, e);
                }
            }
        }
        Ok(contexts)
    }

    async fn fetch_file_content(
        &self,
        path: &str,
        git_ref: &str,
    ) -> Result<Option<String>, AppError> {
        self.api
            .get_file_content(&self.project_path, path, git_ref)
            .await
            .map_err(gl_err)
    }

    async fn submit_review(
        &self,
        payload: SubmissionPayload,
    ) -> Result<SubmissionResult, AppError> {
        let event_lower = payload.event.to_lowercase();
        let mut notes_count = 0usize;

        let mr = self
            .api
            .get_merge_request(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;

        let existing_notes = self
            .api
            .list_notes(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;
        let existing_discussions = self
            .api
            .list_discussions(&self.project_path, self.iid)
            .await
            .map_err(gl_err)?;

        let mut existing_inline_fingerprints: std::collections::HashSet<String> = existing_notes
            .iter()
            .filter_map(|n| extract_inline_fingerprint(&n.body))
            .collect();
        for discussion in &existing_discussions {
            for note in &discussion.notes {
                if let Some(fp) = extract_inline_fingerprint(&note.body) {
                    existing_inline_fingerprints.insert(fp);
                }
            }
        }
        let mut existing_summary_fingerprints: std::collections::HashSet<String> = existing_notes
            .iter()
            .filter_map(|n| extract_summary_fingerprint(&n.body))
            .collect();
        for discussion in &existing_discussions {
            for note in &discussion.notes {
                if let Some(fp) = extract_summary_fingerprint(&note.body) {
                    existing_summary_fingerprints.insert(fp);
                }
            }
        }

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
                    .create_note(&self.project_path, self.iid, &body_with_fp)
                    .await
                    .map_err(gl_err)?;
                existing_summary_fingerprints.insert(summary_fingerprint);
                notes_count += 1;
            }
        }

        if !payload.inline_comments.is_empty() {
            let diff_refs = mr.diff_refs.as_ref().ok_or_else(|| {
                AppError::InvalidInput(
                    "GitLab MR is missing diff_refs; cannot create anchored inline discussions."
                        .to_string(),
                )
            })?;
            let base_sha = diff_refs.base_sha.clone().ok_or_else(|| {
                AppError::InvalidInput("GitLab MR diff_refs.base_sha is missing".to_string())
            })?;
            let start_sha = diff_refs.start_sha.clone().ok_or_else(|| {
                AppError::InvalidInput("GitLab MR diff_refs.start_sha is missing".to_string())
            })?;
            let head_sha = diff_refs.head_sha.clone().ok_or_else(|| {
                AppError::InvalidInput("GitLab MR diff_refs.head_sha is missing".to_string())
            })?;

            for comment in &payload.inline_comments {
                let Some(line) = comment.line else {
                    continue;
                };
                let line_is_left = comment.side.as_deref() == Some("LEFT");
                let inline_fingerprint =
                    extract_inline_fingerprint(&comment.body).unwrap_or_else(|| {
                        crate::storage::hashing::sha256_hex(&format!(
                            "{}|{}|{}|{}",
                            comment.path, line, line_is_left, comment.body
                        ))
                    });
                if !existing_inline_fingerprints.insert(inline_fingerprint) {
                    continue;
                }

                let (new_line, old_line) = if line_is_left {
                    (None, Some(line))
                } else {
                    (Some(line), None)
                };
                self.api
                    .create_discussion(
                        &self.project_path,
                        self.iid,
                        &CreateDiffNotePayload {
                            body: comment.body.clone(),
                            position: DiffNotePosition {
                                base_sha: base_sha.clone(),
                                start_sha: start_sha.clone(),
                                head_sha: head_sha.clone(),
                                position_type: "text".to_string(),
                                new_path: comment.path.clone(),
                                old_path: Some(comment.path.clone()),
                                new_line,
                                old_line,
                            },
                        },
                    )
                    .await
                    .map_err(gl_err)?;
                notes_count += 1;
            }
        }

        if event_lower == "approve" {
            self.api
                .approve_merge_request(&self.project_path, self.iid)
                .await
                .map_err(gl_err)?;
        } else if event_lower == "request_changes" {
            self.api
                .unapprove_merge_request(&self.project_path, self.iid)
                .await
                .map_err(gl_err)?;
        }

        Ok(SubmissionResult {
            review_id: None,
            url: Some(format!(
                "https://{}/{}/-/merge_requests/{}",
                self.host, self.project_path, self.iid
            )),
            notes_created: notes_count,
        })
    }
}
