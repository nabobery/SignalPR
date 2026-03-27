use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::commands::intake::parse_pr_url;
use crate::storage::db::AppDb;
use crate::storage::models::{Finding, SubmissionRecord};
use crate::storage::queries;

#[tauri::command]
pub async fn submit_review(
    app: AppHandle,
    run_id: String,
    action: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), String> {
    // Check for duplicate submission
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        if let Some(_existing) =
            queries::get_submission_for_run(&conn, &run_id).map_err(|e| e.to_string())?
        {
            return Err(
                "Review already submitted for this run. Use force_resubmit if intentional.".into(),
            );
        }
    }

    // Get run + PR + findings
    let (owner, repo, pr_number, findings) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let run = queries::get_review_run(&conn, &run_id)
            .map_err(|e| e.to_string())?
            .ok_or("Review run not found")?;
        let pr = queries::get_pull_request(&conn, &run.pr_id)
            .map_err(|e| e.to_string())?
            .ok_or("PR not found")?;
        let findings = queries::get_findings_for_run(&conn, &run_id).map_err(|e| e.to_string())?;

        let parsed = parse_pr_url(&pr.url).map_err(|_| "Cannot parse PR URL".to_string())?;
        let owner = parsed.owner;
        let repo = parsed.repo;

        (owner, repo, pr.pr_number, findings)
    };

    // Filter to active findings only
    let active_findings: Vec<&Finding> = findings.iter().filter(|f| f.status == "active").collect();

    // Format review body
    let body = format_review_body(&active_findings);

    // Create submission record
    let sub_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        queries::insert_submission(
            &conn,
            &SubmissionRecord {
                id: sub_id.clone(),
                review_run_id: run_id.clone(),
                review_action: action.clone(),
                submitted_at: None,
                status: "pending".into(),
                gh_review_id: None,
                error_message: None,
            },
        )
        .map_err(|e| e.to_string())?;
    }

    // Submit via gh
    let shell = app.shell();
    let gh_action = match action.as_str() {
        "approve" => "--approve",
        "request-changes" => "--request-changes",
        _ => "--comment",
    };

    let output = shell
        .command("gh")
        .args([
            "pr",
            "review",
            &pr_number.to_string(),
            "--repo",
            &format!("{}/{}", owner, repo),
            gh_action,
            "--body",
            &body,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run gh: {}", e))?;

    if output.status.success() {
        // Phase B (best-effort): attempt to post inline comments for anchored findings
        let inline_findings: Vec<&Finding> = active_findings
            .iter()
            .filter(|f| f.diff_new_line.is_some() && f.file_path.is_some())
            .copied()
            .collect();
        if !inline_findings.is_empty() {
            if let Err(e) =
                post_inline_comments(&app, &owner, &repo, pr_number, &inline_findings).await
            {
                tracing::warn!("Inline comments (best-effort) failed: {}", e);
            }
        }

        let conn = db.0.lock().map_err(|e| e.to_string())?;
        queries::update_submission_status(&conn, &sub_id, "submitted", None, None)
            .map_err(|e| e.to_string())?;
        queries::update_review_run_status(&conn, &run_id, "submitted", None)
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        queries::update_submission_status(&conn, &sub_id, "failed", None, Some(&stderr))
            .map_err(|e| e.to_string())?;
        Err(format!("gh pr review failed: {}", stderr))
    }
}

/// Phase B: Post inline review comments for findings with valid diff anchors.
///
/// Uses the GitHub REST API endpoint:
///   POST /repos/{owner}/{repo}/pulls/{pr_number}/comments
///
/// This is best-effort — failures are logged but never bubble up to the caller,
/// keeping the main `gh pr review` submission (Phase A) reliable.
async fn post_inline_comments(
    app: &AppHandle,
    owner: &str,
    repo: &str,
    pr_number: i32,
    findings: &[&Finding],
) -> Result<(), String> {
    let shell = app.shell();

    // Get HEAD commit SHA (needed for the inline comment API)
    let sha_output = shell
        .command("gh")
        .args([
            "pr",
            "view",
            &pr_number.to_string(),
            "--repo",
            &format!("{}/{}", owner, repo),
            "--json",
            "headRefOid",
            "--jq",
            ".headRefOid",
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to get HEAD SHA: {}", e))?;

    if !sha_output.status.success() {
        return Err(format!(
            "Failed to get HEAD SHA: {}",
            String::from_utf8_lossy(&sha_output.stderr)
        ));
    }

    let commit_sha = String::from_utf8_lossy(&sha_output.stdout)
        .trim()
        .to_string();
    if commit_sha.is_empty() {
        return Err("HEAD SHA is empty".into());
    }

    for finding in findings {
        let file_path = match &finding.file_path {
            Some(fp) => fp.clone(),
            None => continue,
        };
        let line = match finding.diff_new_line {
            Some(l) => l,
            None => continue,
        };

        let severity_label = finding
            .user_severity_override
            .as_deref()
            .unwrap_or(&finding.severity)
            .to_uppercase();
        let display_body = finding.user_edited_body.as_deref().unwrap_or(&finding.body);
        let comment_body = format!(
            "**[{}]** {}\n\n{}",
            severity_label, finding.title, display_body
        );

        tracing::info!(
            "Posting inline comment on {}:{} — {}",
            file_path,
            line,
            finding.title
        );

        let line_str = line.to_string();
        let endpoint = format!("repos/{}/{}/pulls/{}/comments", owner, repo, pr_number);

        let result = shell
            .command("gh")
            .args([
                "api",
                &endpoint,
                "--method",
                "POST",
                "-f",
                &format!("body={}", comment_body),
                "-f",
                &format!("commit_id={}", commit_sha),
                "-f",
                &format!("path={}", file_path),
                "-f",
                "side=RIGHT",
                "-F",
                &format!("line={}", line_str),
            ])
            .output()
            .await;

        match result {
            Ok(out) if out.status.success() => {
                tracing::info!("Inline comment posted for {}", finding.title);
            }
            Ok(out) => {
                tracing::warn!(
                    "Inline comment failed for {}: {}",
                    finding.title,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
            Err(e) => {
                tracing::warn!("Inline comment error for {}: {}", finding.title, e);
            }
        }
    }

    Ok(())
}

fn format_review_body(findings: &[&Finding]) -> String {
    let mut body = String::new();
    body.push_str("## SignalPR Review\n\n");

    if findings.is_empty() {
        body.push_str("No significant findings.\n");
        return body;
    }

    body.push_str(&format!("### Findings ({} issues)\n\n", findings.len()));

    for (i, f) in findings.iter().enumerate() {
        let severity_label = f
            .user_severity_override
            .as_deref()
            .unwrap_or(&f.severity)
            .to_uppercase();
        let display_body = f.user_edited_body.as_deref().unwrap_or(&f.body);

        body.push_str(&format!("**[{}]** {}\n", severity_label, f.title));

        if let Some(ref fp) = f.file_path {
            let location = match (f.line_start, f.line_end) {
                (Some(s), Some(e)) if f.is_anchored => format!("`{}:{}-{}`", fp, s, e),
                (Some(s), _) if f.is_anchored => format!("`{}:{}`", fp, s),
                _ => format!("`{}`", fp),
            };
            body.push_str(&format!("{}\n", location));
        }

        if display_body.is_empty() {
            body.push_str("> \n");
        } else {
            for line in display_body.lines() {
                body.push_str(&format!("> {}\n", line));
            }
        }

        if i < findings.len() - 1 {
            body.push_str("\n---\n\n");
        }
    }

    body.push_str("\n\n*Reviewed with SignalPR*\n");
    body
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::Finding;

    fn make_finding(severity: &str, title: &str, body: &str, file: Option<&str>) -> Finding {
        Finding {
            id: "f1".into(),
            review_run_id: "run".into(),
            agent_type: "security".into(),
            file_path: file.map(|s| s.into()),
            line_start: Some(10),
            line_end: Some(20),
            severity: severity.into(),
            confidence: 0.9,
            title: title.into(),
            body: body.into(),
            evidence: None,
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: file.is_some(),
            created_at: "2026-01-01".into(),
            cluster_id: None,
            lane_id: None,
            provider_name: None,
            diff_side: None,
            diff_new_line: None,
        }
    }

    #[test]
    fn test_format_review_body_with_findings() {
        let f1 = make_finding(
            "blocker",
            "Token bypass",
            "Auth is bypassed",
            Some("src/auth.rs"),
        );
        let f2 = make_finding("warning", "N+1 query", "Loop queries", Some("src/db.rs"));
        let findings: Vec<&Finding> = vec![&f1, &f2];
        let body = format_review_body(&findings);

        assert!(body.contains("## SignalPR Review"));
        assert!(body.contains("### Findings (2 issues)"));
        assert!(body.contains("**[BLOCKER]** Token bypass"));
        assert!(body.contains("`src/auth.rs:10-20`"));
        assert!(body.contains("**[WARNING]** N+1 query"));
        assert!(body.contains("*Reviewed with SignalPR*"));
    }

    #[test]
    fn test_format_review_body_empty() {
        let body = format_review_body(&[]);
        assert!(body.contains("No significant findings"));
    }

    #[test]
    fn test_format_uses_edited_body() {
        let mut f = make_finding("warning", "Test", "Original", Some("file.rs"));
        f.user_edited_body = Some("Edited content".into());
        let findings: Vec<&Finding> = vec![&f];
        let body = format_review_body(&findings);
        assert!(body.contains("Edited content"));
        assert!(!body.contains("Original"));
    }

    #[test]
    fn test_format_uses_overridden_severity() {
        let mut f = make_finding("warning", "Test", "Body", Some("file.rs"));
        f.user_severity_override = Some("blocker".into());
        let findings: Vec<&Finding> = vec![&f];
        let body = format_review_body(&findings);
        assert!(body.contains("**[BLOCKER]**"));
    }

    #[test]
    fn test_format_multiline_body_blockquote() {
        let mut f = make_finding("warning", "Test", "Line 1\nLine 2", Some("file.rs"));
        f.user_edited_body = Some("Edited 1\nEdited 2".into());
        let findings: Vec<&Finding> = vec![&f];
        let body = format_review_body(&findings);
        assert!(body.contains("> Edited 1\n"));
        assert!(body.contains("> Edited 2\n"));
    }
}
