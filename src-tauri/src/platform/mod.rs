pub mod adapter;
pub mod bitbucket_adapter;
pub mod factory;
pub mod github_adapter;
pub mod gitlab_adapter;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[allow(dead_code)]
pub enum PlatformKind {
    GitHub,
    GitLab,
    Bitbucket,
}

impl std::fmt::Display for PlatformKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlatformKind::GitHub => write!(f, "github"),
            PlatformKind::GitLab => write!(f, "gitlab"),
            PlatformKind::Bitbucket => write!(f, "bitbucket"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum ParsedReviewUrl {
    GitHub {
        host: String,
        owner: String,
        repo: String,
        number: i32,
    },
    GitLab {
        host: String,
        project_path: String,
        iid: i32,
    },
    Bitbucket {
        host: String,
        workspace: String,
        repo_slug: String,
        pull_request_id: i32,
    },
}

impl ParsedReviewUrl {
    #[allow(dead_code)]
    pub fn platform_kind(&self) -> PlatformKind {
        match self {
            ParsedReviewUrl::GitHub { .. } => PlatformKind::GitHub,
            ParsedReviewUrl::GitLab { .. } => PlatformKind::GitLab,
            ParsedReviewUrl::Bitbucket { .. } => PlatformKind::Bitbucket,
        }
    }

    pub fn host(&self) -> &str {
        match self {
            ParsedReviewUrl::GitHub { host, .. } => host,
            ParsedReviewUrl::GitLab { host, .. } => host,
            ParsedReviewUrl::Bitbucket { host, .. } => host,
        }
    }

    pub fn number_or_iid(&self) -> i32 {
        match self {
            ParsedReviewUrl::GitHub { number, .. } => *number,
            ParsedReviewUrl::GitLab { iid, .. } => *iid,
            ParsedReviewUrl::Bitbucket {
                pull_request_id, ..
            } => *pull_request_id,
        }
    }

    /// Returns `(owner, repo)` for GitHub, `(group_path, project_name)` for GitLab,
    /// or `(workspace, repo_slug)` for Bitbucket.
    pub fn owner_and_repo(&self) -> (String, String) {
        match self {
            ParsedReviewUrl::GitHub { owner, repo, .. } => (owner.clone(), repo.clone()),
            ParsedReviewUrl::GitLab { project_path, .. } => {
                if let Some(slash) = project_path.rfind('/') {
                    (
                        project_path[..slash].to_string(),
                        project_path[slash + 1..].to_string(),
                    )
                } else {
                    (String::new(), project_path.clone())
                }
            }
            ParsedReviewUrl::Bitbucket {
                workspace,
                repo_slug,
                ..
            } => (workspace.clone(), repo_slug.clone()),
        }
    }
}

use regex::Regex;
use std::sync::OnceLock;

use crate::errors::AppError;

static GH_PR_REGEX: OnceLock<Regex> = OnceLock::new();
static GL_MR_REGEX: OnceLock<Regex> = OnceLock::new();
static BB_PR_REGEX: OnceLock<Regex> = OnceLock::new();

pub fn parse_review_url(url: &str) -> Result<ParsedReviewUrl, AppError> {
    let gh_re = GH_PR_REGEX.get_or_init(|| {
        Regex::new(
            r"^https?://(?P<host>github\.com)/(?P<owner>[^/]+)/(?P<repo>[^/]+)/pull/(?P<number>\d+)(?:[/?#].*)?$",
        )
        .expect("GH PR URL regex should be valid")
    });

    if let Some(caps) = gh_re.captures(url) {
        let number = caps["number"]
            .parse::<i32>()
            .map_err(|_| AppError::InvalidInput("PR number is not a valid integer".into()))?;
        return Ok(ParsedReviewUrl::GitHub {
            host: caps["host"].to_lowercase(),
            owner: caps["owner"].to_string(),
            repo: caps["repo"].to_string(),
            number,
        });
    }

    // Bitbucket Cloud: https://bitbucket.org/{workspace}/{repo_slug}/pull-requests/{id}
    // Allow trailing segments like /diff, /activity, query strings, fragments
    let bb_re = BB_PR_REGEX.get_or_init(|| {
        Regex::new(
            r"^https?://(?P<host>bitbucket\.org)/(?P<workspace>[^/]+)/(?P<repo_slug>[^/]+)/pull-requests/(?P<pr_id>\d+)(?:[/?#].*)?$",
        )
        .expect("BB PR URL regex should be valid")
    });

    if let Some(caps) = bb_re.captures(url) {
        let pull_request_id = caps["pr_id"]
            .parse::<i32>()
            .map_err(|_| AppError::InvalidInput("PR id is not a valid integer".into()))?;
        return Ok(ParsedReviewUrl::Bitbucket {
            host: caps["host"].to_lowercase(),
            workspace: caps["workspace"].to_string(),
            repo_slug: caps["repo_slug"].to_string(),
            pull_request_id,
        });
    }

    // GitLab: https://<host>/<project_path>/-/merge_requests/<iid>
    // project_path can be nested: group/subgroup/project
    let gl_re = GL_MR_REGEX.get_or_init(|| {
        Regex::new(
            r"^https?://(?P<host>[^/]+)/(?P<project_path>.+?)/-/merge_requests/(?P<iid>\d+)(?:[/?#].*)?$",
        )
            .expect("GL MR URL regex should be valid")
    });

    if let Some(caps) = gl_re.captures(url) {
        let iid = caps["iid"]
            .parse::<i32>()
            .map_err(|_| AppError::InvalidInput("MR iid is not a valid integer".into()))?;
        return Ok(ParsedReviewUrl::GitLab {
            host: caps["host"].to_lowercase(),
            project_path: caps["project_path"].to_string(),
            iid,
        });
    }

    Err(AppError::InvalidInput(
        "Invalid PR/MR URL. Supported: GitHub pull request, GitLab merge request, or Bitbucket Cloud pull request URLs.".into(),
    ))
}

/// Parse a git remote URL, supporting GitLab nested groups.
/// For non-GitHub hosts, allows multi-segment owner paths (group/subgroup).
pub fn parse_git_remote_url(url: &str) -> Option<ParsedRemote> {
    let url = url.trim();

    // SSH: git@host:path.git
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        if path.is_empty() {
            return None;
        }
        let host_lc = host.to_lowercase();
        return split_remote_path(&host_lc, path);
    }

    // http(s):// or ssh://
    let without_scheme = url.split_once("://").map(|(_, r)| r)?;
    let without_user = without_scheme
        .strip_prefix("git@")
        .unwrap_or(without_scheme);

    let (host_with_user, path) = without_user.split_once('/')?;
    let host = host_with_user
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(host_with_user);
    let path = path.split_once('?').map(|(p, _)| p).unwrap_or(path);
    let path = path.split_once('#').map(|(p, _)| p).unwrap_or(path);
    let path = path.strip_suffix(".git").unwrap_or(path);

    if path.is_empty() {
        return None;
    }

    let host_lc = host.to_lowercase();
    split_remote_path(&host_lc, path)
}

/// Split a remote path into owner + repo.
/// For `github.com`, enforces exactly `owner/repo` (2 segments).
/// For other hosts (GitLab, self-managed), treats the last segment as repo
/// and joins preceding segments as the owner path (supports nested groups).
fn split_remote_path(host: &str, path: &str) -> Option<ParsedRemote> {
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() < 2 {
        return None;
    }

    if host == "github.com" && segments.len() != 2 {
        return None;
    }

    let repo = segments.last().unwrap().to_lowercase();
    if repo.is_empty() {
        return None;
    }
    let owner = segments[..segments.len() - 1]
        .iter()
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .join("/");

    Some(ParsedRemote {
        host: host.to_string(),
        owner,
        repo,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedRemote {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_review_url tests ---

    #[test]
    fn test_parse_github_pr_url() {
        let result = parse_review_url("https://github.com/octocat/hello-world/pull/42").unwrap();
        match result {
            ParsedReviewUrl::GitHub {
                host,
                owner,
                repo,
                number,
            } => {
                assert_eq!(host, "github.com");
                assert_eq!(owner, "octocat");
                assert_eq!(repo, "hello-world");
                assert_eq!(number, 42);
            }
            _ => panic!("Expected GitHub variant"),
        }
    }

    #[test]
    fn test_parse_github_pr_url_with_trailing() {
        let result =
            parse_review_url("https://github.com/octocat/hello-world/pull/42/files").unwrap();
        assert_eq!(result.number_or_iid(), 42);
        assert_eq!(result.platform_kind(), PlatformKind::GitHub);
    }

    #[test]
    fn test_parse_gitlab_mr_url() {
        let result =
            parse_review_url("https://gitlab.com/group/project/-/merge_requests/123").unwrap();
        match result {
            ParsedReviewUrl::GitLab {
                host,
                project_path,
                iid,
            } => {
                assert_eq!(host, "gitlab.com");
                assert_eq!(project_path, "group/project");
                assert_eq!(iid, 123);
            }
            _ => panic!("Expected GitLab variant"),
        }
    }

    #[test]
    fn test_parse_gitlab_nested_groups() {
        let result =
            parse_review_url("https://gitlab.com/group/subgroup/project/-/merge_requests/456")
                .unwrap();
        match result {
            ParsedReviewUrl::GitLab {
                project_path, iid, ..
            } => {
                assert_eq!(project_path, "group/subgroup/project");
                assert_eq!(iid, 456);
            }
            _ => panic!("Expected GitLab variant"),
        }
    }

    #[test]
    fn test_parse_gitlab_self_managed() {
        let result =
            parse_review_url("https://git.company.com/team/repo/-/merge_requests/1").unwrap();
        match result {
            ParsedReviewUrl::GitLab { host, .. } => {
                assert_eq!(host, "git.company.com");
            }
            _ => panic!("Expected GitLab variant"),
        }
    }

    #[test]
    fn test_parse_gitlab_with_port() {
        let result =
            parse_review_url("https://git.company.com:8443/team/repo/-/merge_requests/9").unwrap();
        match result {
            ParsedReviewUrl::GitLab { host, iid, .. } => {
                assert_eq!(host, "git.company.com:8443");
                assert_eq!(iid, 9);
            }
            _ => panic!("Expected GitLab variant"),
        }
    }

    #[test]
    fn test_parse_review_url_rejects_number_prefix_matches() {
        assert!(parse_review_url("https://github.com/octo/repo/pull/42foo").is_err());
        assert!(parse_review_url("https://gitlab.com/group/repo/-/merge_requests/99abc").is_err());
    }

    // --- Bitbucket URL parsing tests ---

    #[test]
    fn test_parse_bitbucket_pr_url() {
        let result =
            parse_review_url("https://bitbucket.org/myworkspace/my-repo/pull-requests/7").unwrap();
        match result {
            ParsedReviewUrl::Bitbucket {
                host,
                workspace,
                repo_slug,
                pull_request_id,
            } => {
                assert_eq!(host, "bitbucket.org");
                assert_eq!(workspace, "myworkspace");
                assert_eq!(repo_slug, "my-repo");
                assert_eq!(pull_request_id, 7);
            }
            _ => panic!("Expected Bitbucket variant"),
        }
    }

    #[test]
    fn test_parse_bitbucket_pr_url_with_trailing_diff() {
        let result =
            parse_review_url("https://bitbucket.org/acme/backend/pull-requests/42/diff").unwrap();
        assert_eq!(result.number_or_iid(), 42);
        assert_eq!(result.platform_kind(), PlatformKind::Bitbucket);
    }

    #[test]
    fn test_parse_bitbucket_pr_url_with_query_and_fragment() {
        let result = parse_review_url(
            "https://bitbucket.org/acme/backend/pull-requests/99?tab=activity#comment-123",
        )
        .unwrap();
        assert_eq!(result.number_or_iid(), 99);
        assert_eq!(result.platform_kind(), PlatformKind::Bitbucket);
    }

    #[test]
    fn test_parse_bitbucket_owner_and_repo() {
        let parsed = parse_review_url("https://bitbucket.org/ws/repo/pull-requests/1").unwrap();
        let (owner, repo) = parsed.owner_and_repo();
        assert_eq!(owner, "ws");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_parse_bitbucket_rejects_non_cloud_host() {
        let result = parse_review_url(
            "https://bitbucket.mycompany.com/projects/PROJ/repos/my-repo/pull-requests/1",
        );
        assert!(
            result.is_err() || {
                if let Ok(parsed) = &result {
                    parsed.platform_kind() != PlatformKind::Bitbucket
                } else {
                    true
                }
            }
        );
    }

    #[test]
    fn test_parse_invalid_url() {
        assert!(parse_review_url("https://example.com/not-a-pr").is_err());
        assert!(parse_review_url("not a url").is_err());
    }

    #[test]
    fn test_parse_bitbucket_rejects_number_suffix() {
        assert!(parse_review_url("https://bitbucket.org/ws/repo/pull-requests/42abc").is_err());
    }

    #[test]
    fn test_owner_and_repo_github() {
        let parsed = parse_review_url("https://github.com/org/repo/pull/1").unwrap();
        let (owner, repo) = parsed.owner_and_repo();
        assert_eq!(owner, "org");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn test_owner_and_repo_gitlab_nested() {
        let parsed =
            parse_review_url("https://gitlab.com/group/subgroup/project/-/merge_requests/1")
                .unwrap();
        let (owner, repo) = parsed.owner_and_repo();
        assert_eq!(owner, "group/subgroup");
        assert_eq!(repo, "project");
    }

    // --- parse_git_remote_url tests ---

    #[test]
    fn test_remote_ssh_github() {
        let r = parse_git_remote_url("git@github.com:Owner/Repo.git").unwrap();
        assert_eq!(r.host, "github.com");
        assert_eq!(r.owner, "owner");
        assert_eq!(r.repo, "repo");
    }

    #[test]
    fn test_remote_https_github() {
        let r = parse_git_remote_url("https://github.com/owner/repo.git").unwrap();
        assert_eq!(r.host, "github.com");
        assert_eq!(r.owner, "owner");
        assert_eq!(r.repo, "repo");
    }

    #[test]
    fn test_remote_https_with_userinfo_and_port() {
        let r = parse_git_remote_url("https://git@git.company.com:8443/team/repo.git").unwrap();
        assert_eq!(r.host, "git.company.com:8443");
        assert_eq!(r.owner, "team");
        assert_eq!(r.repo, "repo");
    }

    #[test]
    fn test_remote_github_extra_segments_rejected() {
        assert!(parse_git_remote_url("https://github.com/a/b/c").is_none());
        assert!(parse_git_remote_url("git@github.com:a/b/c.git").is_none());
    }

    #[test]
    fn test_remote_gitlab_nested_groups() {
        let r = parse_git_remote_url("https://gitlab.com/group/subgroup/project.git").unwrap();
        assert_eq!(r.host, "gitlab.com");
        assert_eq!(r.owner, "group/subgroup");
        assert_eq!(r.repo, "project");
    }

    #[test]
    fn test_remote_gitlab_ssh_nested() {
        let r = parse_git_remote_url("git@gitlab.com:group/subgroup/project.git").unwrap();
        assert_eq!(r.host, "gitlab.com");
        assert_eq!(r.owner, "group/subgroup");
        assert_eq!(r.repo, "project");
    }

    #[test]
    fn test_remote_self_managed_gitlab() {
        let r = parse_git_remote_url("https://git.company.com/team/project.git").unwrap();
        assert_eq!(r.host, "git.company.com");
        assert_eq!(r.owner, "team");
        assert_eq!(r.repo, "project");
    }

    #[test]
    fn test_remote_invalid() {
        assert!(parse_git_remote_url("not a url").is_none());
        assert!(parse_git_remote_url("github.com/owner/repo").is_none());
    }

    #[test]
    fn test_remote_ssh_scheme() {
        let r = parse_git_remote_url("ssh://git@github.com/owner/repo.git").unwrap();
        assert_eq!(r.host, "github.com");
        assert_eq!(r.owner, "owner");
        assert_eq!(r.repo, "repo");
    }
}
