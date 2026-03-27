use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::errors::AppError;
use crate::storage::db::AppDb;
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
pub struct ParsedPrUrl {
    pub owner: String,
    pub repo: String,
    pub number: i32,
}

static PR_URL_REGEX: OnceLock<Regex> = OnceLock::new();

pub fn parse_pr_url(url: &str) -> Result<ParsedPrUrl, AppError> {
    let re = PR_URL_REGEX.get_or_init(|| {
        Regex::new(r"https?://github\.com/(?P<owner>[^/]+)/(?P<repo>[^/]+)/pull/(?P<number>\d+)")
            .expect("PR URL regex should be valid")
    });
    let caps = re
        .captures(url)
        .ok_or_else(|| AppError::InvalidInput("Invalid GitHub PR URL format".into()))?;
    Ok(ParsedPrUrl {
        owner: caps["owner"].to_string(),
        repo: caps["repo"].to_string(),
        number: caps["number"]
            .parse::<i32>()
            .map_err(|_| AppError::InvalidInput("PR number is not a valid integer".into()))?,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParsedRemote {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

pub fn parse_git_remote_url(url: &str) -> Option<ParsedRemote> {
    let url = url.trim();

    // SSH: git@github.com:owner/repo.git (or without .git)
    if let Some(rest) = url.strip_prefix("git@") {
        let (host, path) = rest.split_once(':')?;
        let path = path.strip_suffix(".git").unwrap_or(path);
        let (owner, repo) = path.split_once('/')?;
        if repo.is_empty() || repo.contains('/') {
            return None;
        }
        return Some(ParsedRemote {
            host: host.to_lowercase(),
            owner: owner.to_lowercase(),
            repo: repo.to_lowercase(),
        });
    }

    // http(s)://github.com/owner/repo(.git) or ssh://git@github.com/owner/repo(.git)
    let without_scheme = url.split_once("://").map(|(_, r)| r)?;

    // Strip optional user prefix for ssh:// URLs
    let without_user = without_scheme
        .strip_prefix("git@")
        .unwrap_or(without_scheme);

    let (host, path) = without_user.split_once('/')?;
    let path = path.split_once('?').map(|(p, _)| p).unwrap_or(path);
    let path = path.split_once('#').map(|(p, _)| p).unwrap_or(path);

    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let owner = parts.next()?;
    let repo = parts.next()?;
    if parts.next().is_some() {
        // Extra path segments beyond owner/repo are not supported.
        return None;
    }

    let repo = repo.strip_suffix(".git").unwrap_or(repo);
    if repo.is_empty() {
        return None;
    }

    Some(ParsedRemote {
        host: host.to_lowercase(),
        owner: owner.to_lowercase(),
        repo: repo.to_lowercase(),
    })
}

#[tauri::command]
pub async fn open_from_url(
    app: AppHandle,
    url: String,
    db: tauri::State<'_, AppDb>,
) -> Result<PrIntakeResult, String> {
    do_open_from_url(app, &url, &db)
        .await
        .map_err(|e| e.to_string())
}

async fn do_open_from_url(
    app: AppHandle,
    url: &str,
    db: &AppDb,
) -> Result<PrIntakeResult, AppError> {
    let parsed = parse_pr_url(url)?;
    let shell = app.shell();
    let full_repo = format!("{}/{}", parsed.owner, parsed.repo);

    // Fetch PR metadata via gh
    let meta_output = shell
        .command("gh")
        .args([
            "pr",
            "view",
            &parsed.number.to_string(),
            "--repo",
            &full_repo,
            "--json",
            "title,author,baseRefName,headRefName,files",
        ])
        .output()
        .await
        .map_err(|e| AppError::InvalidInput(format!("Failed to run gh: {}", e)))?;

    if !meta_output.status.success() {
        let stderr = String::from_utf8_lossy(&meta_output.stderr);
        return Err(AppError::InvalidInput(format!(
            "gh pr view failed: {}",
            stderr
        )));
    }

    let meta: serde_json::Value = serde_json::from_slice(&meta_output.stdout)?;

    let title = meta["title"].as_str().unwrap_or("").to_string();
    let author = meta["author"]["login"].as_str().map(|s| s.to_string());
    let base_branch = meta["baseRefName"].as_str().map(|s| s.to_string());
    let head_branch = meta["headRefName"].as_str().map(|s| s.to_string());
    let files = meta["files"].as_array();
    let changed_files: Vec<String> = files
        .map(|f| {
            f.iter()
                .filter_map(|file| file["path"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Fetch diff
    let diff_output = shell
        .command("gh")
        .args([
            "pr",
            "diff",
            &parsed.number.to_string(),
            "--repo",
            &full_repo,
        ])
        .output()
        .await
        .map_err(|e| AppError::InvalidInput(format!("Failed to fetch diff: {}", e)))?;

    let diff_text = if diff_output.status.success() {
        Some(String::from_utf8_lossy(&diff_output.stdout).to_string())
    } else {
        None
    };

    // Check for existing workspace suggestion
    let workspace_suggestion = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::get_workspace_by_remote(&conn, &parsed.owner, &parsed.repo)?
            .map(|ws| ws.local_path)
    };

    // Persist PR
    let pr_id = uuid::Uuid::new_v4().to_string();
    let ws_id = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        // Upsert workspace with a placeholder path if no suggestion exists
        let existing = queries::get_workspace_by_remote(&conn, &parsed.owner, &parsed.repo)?;
        match existing {
            Some(ws) => ws.id,
            None => {
                let new_ws_id = uuid::Uuid::new_v4().to_string();
                queries::insert_workspace(
                    &conn,
                    &Workspace {
                        id: new_ws_id.clone(),
                        local_path: "".to_string(), // Will be set by confirm_workspace
                        remote_owner: parsed.owner.clone(),
                        remote_repo: parsed.repo.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
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
                pr_number: parsed.number,
                title: title.clone(),
                author: author.clone(),
                base_branch: base_branch.clone(),
                head_branch: head_branch.clone(),
                url: url.to_string(),
                diff_text,
                changed_files: Some(serde_json::to_string(&changed_files)?),
                fetched_at: chrono::Utc::now().to_rfc3339(),
            },
        )?;
    }

    Ok(PrIntakeResult {
        pr_id,
        owner: parsed.owner,
        repo: parsed.repo,
        pr_number: parsed.number,
        title,
        author,
        base_branch,
        head_branch,
        changed_file_count: changed_files.len(),
        workspace_suggestion,
    })
}

#[tauri::command]
pub async fn confirm_workspace(
    app: AppHandle,
    pr_id: String,
    local_path: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), String> {
    do_confirm_workspace(app, &pr_id, &local_path, &db)
        .await
        .map_err(|e| e.to_string())
}

async fn do_confirm_workspace(
    app: AppHandle,
    pr_id: &str,
    local_path: &str,
    db: &AppDb,
) -> Result<(), AppError> {
    // Get the PR to find owner/repo
    let (owner, repo) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        // Parse owner/repo from the stored URL
        let parsed = parse_pr_url(&pr.url)?;
        (parsed.owner, parsed.repo)
    };

    // Validate: is it a git repo with matching remote?
    let shell = app.shell();
    let remote_output = shell
        .command("git")
        .args(["-C", local_path, "remote", "get-url", "origin"])
        .output()
        .await
        .map_err(|e| AppError::InvalidInput(format!("Failed to check git remote: {}", e)))?;

    if !remote_output.status.success() {
        return Err(AppError::InvalidInput(format!(
            "Path '{}' is not a git repository or has no 'origin' remote",
            local_path
        )));
    }

    let remote_url = String::from_utf8_lossy(&remote_output.stdout)
        .trim()
        .to_string();

    let parsed_remote = parse_git_remote_url(&remote_url).ok_or_else(|| {
        AppError::InvalidInput(format!("Cannot parse remote URL: {}", remote_url))
    })?;

    if parsed_remote.host != "github.com"
        || parsed_remote.owner != owner.to_lowercase()
        || parsed_remote.repo != repo.to_lowercase()
    {
        return Err(AppError::InvalidInput(format!(
            "Remote '{}' does not match expected repository {}/{}",
            remote_url, owner, repo
        )));
    }

    // Update workspace local_path
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        conn.execute(
            "UPDATE workspaces SET local_path = ?1 WHERE remote_owner = ?2 AND remote_repo = ?3",
            rusqlite::params![local_path, owner, repo],
        )?;
    }

    Ok(())
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
        assert!(parse_pr_url("https://gitlab.com/octocat/hello-world/pull/42").is_err());
        assert!(parse_pr_url("not a url").is_err());
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
    fn test_parse_remote_spoof_host_mismatch() {
        assert_eq!(
            parse_git_remote_url("https://evil.com/owner/repo"),
            Some(ParsedRemote {
                host: "evil.com".into(),
                owner: "owner".into(),
                repo: "repo".into(),
            })
        );
    }

    #[test]
    fn test_parse_remote_extra_path_segments_rejected() {
        assert_eq!(parse_git_remote_url("https://github.com/a/b/c"), None);
        assert_eq!(parse_git_remote_url("git@github.com:a/b/c.git"), None);
    }

    #[test]
    fn test_parse_remote_invalid_url() {
        assert_eq!(parse_git_remote_url("not a url"), None);
        assert_eq!(parse_git_remote_url("github.com/owner/repo"), None);
    }
}
