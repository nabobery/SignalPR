use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::errors::AppError;
use crate::platform::{self, ParsedReviewUrl};
use crate::storage::db::AppDb;
use crate::storage::hashing::sha256_hex;
use crate::storage::models::{PullRequest, Workspace};
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct PrIntakeResult {
    pub pr_id: String,
    pub owner: String,
    pub repo: String,
    pub pr_number: i32,
    pub title: String,
    pub author: Option<String>,
    pub base_branch: Option<String>,
    pub head_branch: Option<String>,
    pub changed_file_count: usize,
    pub workspace_suggestion: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ParsedPrUrl {
    pub owner: String,
    pub repo: String,
    pub number: i32,
}

/// Legacy wrapper: parses a GitHub PR URL. Call sites that only need GitHub
/// can continue to use this; new code should prefer `platform::parse_review_url`.
#[allow(dead_code)]
pub fn parse_pr_url(url: &str) -> Result<ParsedPrUrl, AppError> {
    let parsed = platform::parse_review_url(url)?;
    match parsed {
        ParsedReviewUrl::GitHub {
            owner,
            repo,
            number,
            ..
        } => Ok(ParsedPrUrl {
            owner,
            repo,
            number,
        }),
        ParsedReviewUrl::GitLab {
            project_path, iid, ..
        } => {
            let (owner, repo) = if let Some(slash) = project_path.rfind('/') {
                (
                    project_path[..slash].to_string(),
                    project_path[slash + 1..].to_string(),
                )
            } else {
                (String::new(), project_path)
            };
            Ok(ParsedPrUrl {
                owner,
                repo,
                number: iid,
            })
        }
        ParsedReviewUrl::Bitbucket {
            workspace,
            repo_slug,
            pull_request_id,
            ..
        } => Ok(ParsedPrUrl {
            owner: workspace,
            repo: repo_slug,
            number: pull_request_id,
        }),
    }
}

pub use crate::platform::ParsedRemote;

pub fn parse_git_remote_url(url: &str) -> Option<ParsedRemote> {
    platform::parse_git_remote_url(url)
}

#[tauri::command]
pub async fn open_from_url(
    app: AppHandle,
    url: String,
    db: tauri::State<'_, AppDb>,
) -> Result<PrIntakeResult, AppError> {
    do_open_from_url(app, &url, &db).await
}

async fn do_open_from_url(
    app: AppHandle,
    url: &str,
    db: &AppDb,
) -> Result<PrIntakeResult, AppError> {
    let review_url = platform::parse_review_url(url)?;
    let (owner, repo) = review_url.owner_and_repo();
    let host = review_url.host().to_string();
    let number = review_url.number_or_iid();

    let adapter = crate::platform::factory::build_adapter(&app, &review_url).await?;
    let snapshot = adapter.fetch_review_snapshot().await?;
    let review_target = snapshot.review_target;
    let metadata = snapshot.metadata;
    let capabilities = snapshot.capabilities;
    let diff_text = snapshot.diff_text;
    let diff_hash = sha256_hex(&diff_text);
    let changed_files = derive_changed_files_from_diff(&diff_text);
    let metadata_json =
        serde_json::to_string(&metadata).map_err(|e| AppError::InvalidInput(e.to_string()))?;
    let capabilities_json =
        serde_json::to_string(&capabilities).map_err(|e| AppError::InvalidInput(e.to_string()))?;

    persist_intake(
        db,
        url,
        &host,
        &owner,
        &repo,
        number,
        &review_target.title,
        review_target.author.as_deref(),
        review_target.base_branch.as_deref(),
        review_target.head_branch.as_deref(),
        Some(&diff_text),
        &changed_files,
        Some(&diff_hash),
        Some(&metadata_json),
        Some(&capabilities_json),
    )
}

#[allow(clippy::too_many_arguments)]
fn persist_intake(
    db: &AppDb,
    url: &str,
    host: &str,
    owner: &str,
    repo: &str,
    number: i32,
    title: &str,
    author: Option<&str>,
    base_branch: Option<&str>,
    head_branch: Option<&str>,
    diff_text: Option<&str>,
    changed_files: &[String],
    diff_hash: Option<&str>,
    metadata_json: Option<&str>,
    capabilities_json: Option<&str>,
) -> Result<PrIntakeResult, AppError> {
    let workspace_suggestion = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::get_workspace_by_remote_and_host(&conn, host, owner, repo)?.map(|ws| ws.local_path)
    };

    let pr_id = uuid::Uuid::new_v4().to_string();
    let fetched_at = chrono::Utc::now().to_rfc3339();
    let ws_id = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let existing = queries::get_workspace_by_remote_and_host(&conn, host, owner, repo)?;
        match existing {
            Some(ws) => ws.id,
            None => {
                let new_ws_id = uuid::Uuid::new_v4().to_string();
                queries::insert_workspace(
                    &conn,
                    &Workspace {
                        id: new_ws_id.clone(),
                        local_path: "".to_string(),
                        remote_owner: owner.to_string(),
                        remote_repo: repo.to_string(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        remote_host: host.to_string(),
                    },
                )?;
                new_ws_id
            }
        }
    };

    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::insert_pull_request(
            &conn,
            &PullRequest {
                id: pr_id.clone(),
                workspace_id: ws_id,
                pr_number: number,
                title: title.to_string(),
                author: author.map(|s| s.to_string()),
                base_branch: base_branch.map(|s| s.to_string()),
                head_branch: head_branch.map(|s| s.to_string()),
                url: url.to_string(),
                diff_text: diff_text.map(|s| s.to_string()),
                changed_files: Some(serde_json::to_string(&changed_files)?),
                fetched_at: fetched_at.clone(),
                diff_hash: diff_hash.map(|s| s.to_string()),
                platform_metadata_json: metadata_json.map(ToOwned::to_owned),
                platform_metadata_fetched_at: metadata_json.map(|_| fetched_at.clone()),
                platform_capabilities_json: capabilities_json.map(ToOwned::to_owned),
                platform_capabilities_fetched_at: capabilities_json.map(|_| fetched_at.clone()),
            },
        )?;
    }

    Ok(PrIntakeResult {
        pr_id,
        owner: owner.to_string(),
        repo: repo.to_string(),
        pr_number: number,
        title: title.to_string(),
        author: author.map(|s| s.to_string()),
        base_branch: base_branch.map(|s| s.to_string()),
        head_branch: head_branch.map(|s| s.to_string()),
        changed_file_count: changed_files.len(),
        workspace_suggestion,
    })
}

fn derive_changed_files_from_diff(diff_text: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in diff_text.lines() {
        let Some(rest) = line.strip_prefix("diff --git ") else {
            continue;
        };
        let Some((_, new_part)) = rest.rsplit_once(" b/") else {
            continue;
        };
        let path = new_part.trim();
        if path.is_empty() {
            continue;
        }
        if seen.insert(path.to_string()) {
            files.push(path.to_string());
        }
    }
    files
}

#[cfg(test)]
fn require_diff_text(success: bool, stdout: &[u8], stderr: &[u8]) -> Result<String, AppError> {
    if success {
        return Ok(String::from_utf8_lossy(stdout).to_string());
    }
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let msg = if stderr.is_empty() {
        "platform diff fetch failed".to_string()
    } else {
        format!("platform diff fetch failed: {}", stderr)
    };
    Err(AppError::Transient(msg))
}

#[tauri::command]
pub async fn confirm_workspace(
    app: AppHandle,
    pr_id: String,
    local_path: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    do_confirm_workspace(app, &pr_id, &local_path, &db).await
}

async fn do_confirm_workspace(
    app: AppHandle,
    pr_id: &str,
    local_path: &str,
    db: &AppDb,
) -> Result<(), AppError> {
    let (host, owner, repo, workspace_id) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let review_url = platform::parse_review_url(&pr.url)?;
        let (owner, repo) = review_url.owner_and_repo();
        (review_url.host().to_string(), owner, repo, pr.workspace_id)
    };

    let shell = app.shell();
    let remote_output = shell
        .command("git")
        .args(["-C", local_path, "remote", "-v"])
        .output()
        .await
        .map_err(|e| AppError::InvalidInput(format!("Failed to check git remotes: {}", e)))?;

    if !remote_output.status.success() {
        return Err(AppError::InvalidInput(format!(
            "Path '{}' is not a git repository",
            local_path
        )));
    }

    let remote_text = String::from_utf8_lossy(&remote_output.stdout).to_string();
    let remotes = parse_git_remotes(&remote_text);

    if !matches_target_repo_on_host(&remotes, &host, &owner, &repo) {
        return Err(AppError::InvalidInput(format!(
            "No remote in '{}' matches repository {}/{} on {}. Found remotes: {}",
            local_path,
            owner,
            repo,
            host,
            remotes
                .iter()
                .map(|(name, h, o, r)| format!("{} ({}/{} on {})", name, o, r, h))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        conn.execute(
            "UPDATE workspaces SET local_path = ?1 WHERE id = ?2",
            rusqlite::params![local_path, workspace_id],
        )?;
    }

    Ok(())
}

/// Parsed remote entry: name, host, owner, repo.
pub type RemoteEntry = (String, String, String, String);

/// Parse `git remote -v` output into a list of (remote_name, host, owner, repo) tuples.
/// Only fetch remotes are included (not push). Supports all hosts (GitHub, GitLab, etc.).
pub fn parse_git_remotes(output: &str) -> Vec<RemoteEntry> {
    let mut remotes = Vec::new();
    for line in output.lines() {
        if !line.contains("(fetch)") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].to_string();
        let url = parts[1];
        if let Some(parsed) = parse_git_remote_url(url) {
            remotes.push((name, parsed.host, parsed.owner, parsed.repo));
        }
    }
    remotes
}

/// Check if any remote matches the target owner/repo for a specific host.
pub fn matches_target_repo_on_host(
    remotes: &[RemoteEntry],
    target_host: &str,
    target_owner: &str,
    target_repo: &str,
) -> bool {
    let host_lc = target_host.to_lowercase();
    let owner_lc = target_owner.to_lowercase();
    let repo_lc = target_repo.to_lowercase();
    remotes
        .iter()
        .any(|(_, h, o, r)| h == &host_lc && o == &owner_lc && r == &repo_lc)
}

/// Legacy 3-tuple version for backward compatibility with existing tests.
#[allow(dead_code)]
pub fn matches_target_repo(
    remotes: &[(String, String, String)],
    target_owner: &str,
    target_repo: &str,
) -> bool {
    let owner_lc = target_owner.to_lowercase();
    let repo_lc = target_repo.to_lowercase();
    remotes
        .iter()
        .any(|(_, o, r)| o == &owner_lc && r == &repo_lc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_pr_url() {
        let result = parse_pr_url("https://github.com/octocat/hello-world/pull/42").unwrap();
        assert_eq!(result.owner, "octocat");
        assert_eq!(result.repo, "hello-world");
        assert_eq!(result.number, 42);
    }

    #[test]
    fn test_parse_pr_url_with_trailing_path() {
        let result = parse_pr_url("https://github.com/octocat/hello-world/pull/42/files").unwrap();
        assert_eq!(result.number, 42);
    }

    #[test]
    fn test_parse_pr_url_with_query_params() {
        let result =
            parse_pr_url("https://github.com/octocat/hello-world/pull/42?diff=split").unwrap();
        assert_eq!(result.number, 42);
    }

    #[test]
    fn test_parse_invalid_url() {
        assert!(parse_pr_url("https://github.com/octocat/hello-world").is_err());
        assert!(parse_pr_url("not a url").is_err());
    }

    #[test]
    fn test_parse_gitlab_mr_url_via_legacy() {
        let result =
            parse_pr_url("https://gitlab.com/group/subgroup/project/-/merge_requests/10").unwrap();
        assert_eq!(result.owner, "group/subgroup");
        assert_eq!(result.repo, "project");
        assert_eq!(result.number, 10);
    }

    #[test]
    fn test_parse_remote_ssh() {
        assert_eq!(
            parse_git_remote_url("git@github.com:Owner/Repo.git"),
            Some(ParsedRemote {
                host: "github.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_https_with_dot_git() {
        assert_eq!(
            parse_git_remote_url("https://github.com/owner/repo.git"),
            Some(ParsedRemote {
                host: "github.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_https_without_dot_git() {
        assert_eq!(
            parse_git_remote_url("https://github.com/owner/repo"),
            Some(ParsedRemote {
                host: "github.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_ssh_scheme() {
        assert_eq!(
            parse_git_remote_url("ssh://git@github.com/owner/repo.git"),
            Some(ParsedRemote {
                host: "github.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_gitlab_nested_groups() {
        assert_eq!(
            parse_git_remote_url("git@gitlab.com:group/subgroup/project.git"),
            Some(ParsedRemote {
                host: "gitlab.com".into(),
                owner: "group/subgroup".into(),
                repo: "project".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_gitlab_https_nested() {
        assert_eq!(
            parse_git_remote_url("https://gitlab.com/group/subgroup/project.git"),
            Some(ParsedRemote {
                host: "gitlab.com".into(),
                owner: "group/subgroup".into(),
                repo: "project".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_extra_path_segments_rejected_on_github() {
        assert_eq!(parse_git_remote_url("https://github.com/a/b/c"), None);
        assert_eq!(parse_git_remote_url("git@github.com:a/b/c.git"), None);
    }

    #[test]
    fn test_parse_remote_invalid_url() {
        assert_eq!(parse_git_remote_url("not a url"), None);
        assert_eq!(parse_git_remote_url("github.com/owner/repo"), None);
    }

    // --- Multi-remote and fork tests ---

    #[test]
    fn test_parse_git_remotes_multi() {
        let output = "origin\thttps://github.com/user/fork.git (fetch)\norigin\thttps://github.com/user/fork.git (push)\nupstream\thttps://github.com/org/repo.git (fetch)\nupstream\thttps://github.com/org/repo.git (push)\n";
        let remotes = parse_git_remotes(output);
        assert_eq!(remotes.len(), 2);
        assert_eq!(
            remotes[0],
            (
                "origin".into(),
                "github.com".into(),
                "user".into(),
                "fork".into()
            )
        );
        assert_eq!(
            remotes[1],
            (
                "upstream".into(),
                "github.com".into(),
                "org".into(),
                "repo".into()
            )
        );
    }

    #[test]
    fn test_parse_git_remotes_ssh() {
        let output = "origin\tgit@github.com:user/repo.git (fetch)\norigin\tgit@github.com:user/repo.git (push)\n";
        let remotes = parse_git_remotes(output);
        assert_eq!(remotes.len(), 1);
        assert_eq!(
            remotes[0],
            (
                "origin".into(),
                "github.com".into(),
                "user".into(),
                "repo".into()
            )
        );
    }

    #[test]
    fn test_parse_git_remotes_gitlab() {
        let output = "origin\tgit@gitlab.com:group/subgroup/project.git (fetch)\norigin\tgit@gitlab.com:group/subgroup/project.git (push)\n";
        let remotes = parse_git_remotes(output);
        assert_eq!(remotes.len(), 1);
        assert_eq!(
            remotes[0],
            (
                "origin".into(),
                "gitlab.com".into(),
                "group/subgroup".into(),
                "project".into()
            )
        );
    }

    #[test]
    fn test_matches_target_repo_on_host_direct() {
        let remotes: Vec<RemoteEntry> = vec![(
            "origin".into(),
            "github.com".into(),
            "org".into(),
            "repo".into(),
        )];
        assert!(matches_target_repo_on_host(
            &remotes,
            "github.com",
            "org",
            "repo"
        ));
        assert!(!matches_target_repo_on_host(
            &remotes,
            "gitlab.com",
            "org",
            "repo"
        ));
    }

    #[test]
    fn test_matches_target_repo_direct() {
        let remotes = vec![("origin".into(), "org".into(), "repo".into())];
        assert!(matches_target_repo(&remotes, "org", "repo"));
        assert!(!matches_target_repo(&remotes, "other", "repo"));
    }

    #[test]
    fn test_matches_target_repo_fork() {
        let remotes = vec![
            ("origin".into(), "user".into(), "fork".into()),
            ("upstream".into(), "org".into(), "repo".into()),
        ];
        assert!(matches_target_repo(&remotes, "org", "repo"));
    }

    #[test]
    fn test_matches_target_repo_case_insensitive() {
        let remotes = vec![("origin".into(), "org".into(), "repo".into())];
        assert!(matches_target_repo(&remotes, "ORG", "REPO"));
    }

    #[test]
    fn test_require_diff_text_success() {
        let out = require_diff_text(true, b"diff --git a/a b/a\n", b"").unwrap();
        assert!(out.contains("diff --git"));
    }

    #[test]
    fn test_require_diff_text_failure_is_transient() {
        let err = require_diff_text(false, b"", b"rate limit exceeded").unwrap_err();
        assert!(matches!(err, AppError::Transient(_)));
        assert!(err.to_string().contains("rate limit"));
    }

    #[test]
    fn test_derive_changed_files_from_diff_dedupes_paths() {
        let diff = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
diff --git a/src/b.rs b/src/b.rs
index 333..444 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
diff --git a/src/a.rs b/src/a.rs
index 555..666 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -3 +3 @@
";
        let files = derive_changed_files_from_diff(diff);
        assert_eq!(files, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    }

    #[test]
    fn test_derive_changed_files_from_diff_handles_rename() {
        let diff = "diff --git a/src/old_name.rs b/src/new_name.rs\n";
        let files = derive_changed_files_from_diff(diff);
        assert_eq!(files, vec!["src/new_name.rs".to_string()]);
    }
}
