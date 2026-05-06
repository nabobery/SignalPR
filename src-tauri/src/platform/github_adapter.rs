use async_trait::async_trait;

use crate::errors::AppError;
use crate::platform::adapter::*;
use crate::providers::github::{
    CreateReviewPayload, GitHubApi, GitHubApiError, ReviewCommentPayload,
};

pub struct GitHubAdapter {
    api: GitHubApi,
    owner: String,
    repo: String,
    number: i32,
}

impl GitHubAdapter {
    pub fn new(api: GitHubApi, owner: String, repo: String, number: i32) -> Self {
        Self {
            api,
            owner,
            repo,
            number,
        }
    }
}

fn gh_err(e: GitHubApiError) -> AppError {
    match e {
        GitHubApiError::RateLimited { .. } => AppError::Transient(e.to_string()),
        GitHubApiError::HttpError { status: 404, .. } => AppError::NotFound(e.to_string()),
        GitHubApiError::HttpError { status, .. } if status >= 500 => {
            AppError::Transient(e.to_string())
        }
        _ => AppError::InvalidInput(e.to_string()),
    }
}

#[async_trait]
impl PlatformAdapter for GitHubAdapter {
    fn platform_name(&self) -> &'static str {
        "github"
    }

    async fn fetch_metadata(&self) -> Result<PlatformMetadata, AppError> {
        let gh_pr = self
            .api
            .get_pull_request(&self.owner, &self.repo, self.number)
            .await
            .map_err(gh_err)?;

        let reviewers = self
            .api
            .get_requested_reviewers(&self.owner, &self.repo, self.number)
            .await
            .map_err(gh_err)?;

        let reviews = self
            .api
            .list_reviews(&self.owner, &self.repo, self.number)
            .await
            .map_err(gh_err)?;

        let linked_issues = self
            .api
            .get_linked_issues(&self.owner, &self.repo, self.number)
            .await
            .map_err(gh_err)?;

        let text_refs =
            crate::context_pack::extract_issue_refs(gh_pr.body.as_deref().unwrap_or(""));
        let title_refs = crate::context_pack::extract_issue_refs(&gh_pr.title);
        let mut all_text_refs = text_refs;
        for r in title_refs {
            if !all_text_refs.contains(&r) {
                all_text_refs.push(r);
            }
        }

        Ok(PlatformMetadata::GitHub(GitHubMeta {
            pr_body: gh_pr.body,
            head_sha: gh_pr.head.sha,
            base_sha: gh_pr.base.sha,
            base_ref: gh_pr.base.ref_name,
            head_ref: gh_pr.head.ref_name,
            draft: gh_pr.draft.unwrap_or(false),
            labels: gh_pr
                .labels
                .unwrap_or_default()
                .into_iter()
                .map(|l| l.name)
                .collect(),
            requested_reviewers: reviewers.users.into_iter().map(|u| u.login).collect(),
            requested_teams: reviewers.teams.into_iter().map(|t| t.slug).collect(),
            review_state_summary: reviews
                .into_iter()
                .filter_map(|r| {
                    Some(ReviewState {
                        login: r.user?.login,
                        state: r.state,
                        submitted_at: r.submitted_at,
                    })
                })
                .collect(),
            linked_issue_numbers: linked_issues,
            text_issue_refs: all_text_refs,
        }))
    }

    async fn fetch_diff(&self) -> Result<String, AppError> {
        let diff = self
            .api
            .get_pull_diff(&self.owner, &self.repo, self.number)
            .await
            .map_err(gh_err)?;
        Ok(diff)
    }

    async fn fetch_issue_context(
        &self,
        issue_ids: &[i64],
        max_issues: usize,
    ) -> Result<Vec<IssueContext>, AppError> {
        let mut contexts = Vec::new();
        for &id in issue_ids.iter().take(max_issues) {
            match self.api.get_issue(&self.owner, &self.repo, id).await {
                Ok(issue) => {
                    let excerpt = issue.body.as_deref().map(|b| {
                        let max = crate::providers::github::MAX_ISSUE_BODY_EXCERPT_BYTES;
                        if b.len() > max {
                            format!("{}...", &b[..max])
                        } else {
                            b.to_string()
                        }
                    });
                    contexts.push(IssueContext {
                        number: issue.number,
                        title: issue.title,
                        body_excerpt: excerpt,
                        labels: issue
                            .labels
                            .unwrap_or_default()
                            .into_iter()
                            .map(|l| l.name)
                            .collect(),
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to fetch issue #{}: {}", id, e);
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
            .get_file_content(&self.owner, &self.repo, path, git_ref)
            .await
            .map_err(gh_err)
    }

    async fn submit_review(
        &self,
        payload: SubmissionPayload,
    ) -> Result<SubmissionResult, AppError> {
        let gh_payload = CreateReviewPayload {
            commit_id: payload.commit_id,
            body: payload.body,
            event: payload.event,
            comments: payload
                .inline_comments
                .into_iter()
                .map(|c| ReviewCommentPayload {
                    path: c.path,
                    body: c.body,
                    line: c.line,
                    side: c.side,
                    start_line: c.start_line,
                    start_side: None,
                })
                .collect(),
        };

        let result = self
            .api
            .create_review(&self.owner, &self.repo, self.number, &gh_payload)
            .await
            .map_err(gh_err)?;

        Ok(SubmissionResult {
            review_id: Some(result.id.to_string()),
            url: Some(format!(
                "https://github.com/{}/{}/pull/{}#pullrequestreview-{}",
                self.owner, self.repo, self.number, result.id
            )),
            notes_created: gh_payload.comments.len(),
        })
    }
}
