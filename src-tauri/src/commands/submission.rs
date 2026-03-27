use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::cleaner::{remap, verify};
use crate::commands::intake::parse_pr_url;
use crate::storage::db::AppDb;
use crate::storage::hashing::sha256_hex;
use crate::storage::models::{Finding, SubmissionRecord};
use crate::storage::queries;

#[tauri::command]
pub async fn submit_review(
    app: AppHandle,
    run_id: String,
    action: String,
    force_resubmit: Option<bool>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), crate::errors::AppError> {
    use crate::errors::AppError;

    let action = match action.as_str() {
        "approve" | "comment" | "request-changes" => action,
        _ => {
            return Err(AppError::InvalidInput(
                "Invalid review action. Must be approve, comment, or request-changes.".into(),
            ))
        }
    };

    // Check for duplicate submission (skip if force_resubmit)
    if !force_resubmit.unwrap_or(false) {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        if let Some(_existing) = queries::get_submission_for_run(&conn, &run_id)? {
            return Err(AppError::InvalidInput(
                "Review already submitted for this run. Pass force_resubmit to override.".into(),
            ));
        }
    }

    // Get run + PR + findings
    let (pr_id, owner, repo, pr_number, mut findings, old_diff, old_diff_hash) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let run = queries::get_review_run(&conn, &run_id)?
            .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;
        let pr = queries::get_pull_request(&conn, &run.pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let findings = queries::get_findings_for_run(&conn, &run_id)?;

        let parsed = parse_pr_url(&pr.url)?;
        let owner = parsed.owner;
        let repo = parsed.repo;

        let old_diff = pr.diff_text.clone().unwrap_or_default();
        let old_hash = pr
            .diff_hash
            .clone()
            .unwrap_or_else(|| sha256_hex(&old_diff));

        (
            run.pr_id,
            owner,
            repo,
            pr.pr_number,
            findings,
            old_diff,
            old_hash,
        )
    };

    // Best-effort: fetch latest diff and reconcile anchors for safe inline comments.
    let mut anchors_verified_for_submission = false;
    let originally_active_ids: std::collections::HashSet<String> = findings
        .iter()
        .filter(|f| f.status == "active")
        .map(|f| f.id.clone())
        .collect();

    if let Ok(latest_diff) = fetch_latest_diff(&app, &owner, &repo, pr_number).await {
        let new_hash = sha256_hex(&latest_diff);
        let (updated, suppressed_ids) =
            reconcile_findings_for_latest_diff(findings, &old_diff, &latest_diff);

        // Persist PR diff + finding anchor updates
        {
            let conn =
                db.0.lock()
                    .map_err(|e| AppError::InvalidInput(e.to_string()))?;

            // Keep PR diff/hash fresh for future resume/submission attempts.
            let _ = queries::update_pull_request_diff(&conn, &pr_id, &latest_diff, &new_hash);

            // Update anchors for all findings we still keep (verified against latest diff).
            for f in &updated {
                let _ = queries::update_finding_anchor(
                    &conn,
                    &f.id,
                    f.line_start,
                    f.line_end,
                    f.is_anchored,
                    f.diff_side.as_deref(),
                    f.diff_new_line,
                );
            }

            // Suppress findings that no longer map to the latest diff.
            for id in suppressed_ids {
                if !originally_active_ids.contains(&id) {
                    continue;
                }
                let _ = queries::update_finding(&conn, &id, None, None, Some("suppressed"));
            }
        }

        // Refresh local copy for body/inline selection.
        // If hashes differ, the mapping above demotes/suppresses as needed.
        findings = updated;
        anchors_verified_for_submission = true;

        if old_diff_hash != new_hash {
            tracing::info!(
                "PR diff changed since intake ({} → {}), reconciled anchors before submission",
                old_diff_hash,
                new_hash
            );
        }
    } else {
        tracing::warn!("Failed to fetch latest PR diff; skipping anchor reconciliation");
    }

    // Filter to active findings only
    let active_findings: Vec<&Finding> = findings.iter().filter(|f| f.status == "active").collect();

    // Format review body
    let body = format_review_body(&active_findings);

    let idempotency_key = sha256_hex(&format!("{}|{}|{}", run_id, action, body));

    // Create submission record
    let sub_id = uuid::Uuid::new_v4().to_string();
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let history = queries::get_submission_history(&conn, &run_id).unwrap_or_default();
        let attempt = next_attempt_count(&history);
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
                idempotency_key: Some(idempotency_key),
                attempt_count: Some(attempt),
                last_attempt_at: Some(chrono::Utc::now().to_rfc3339()),
            },
        )?;
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
        .map_err(|e| AppError::Transient(format!("Failed to run gh: {}", e)))?;

    if output.status.success() {
        // Phase B (best-effort): attempt to post inline comments for anchored findings
        if anchors_verified_for_submission {
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
        }

        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::update_submission_status(&conn, &sub_id, "submitted", None, None)?;
        queries::update_review_run_status(&conn, &run_id, "submitted", None)?;
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::update_submission_status(&conn, &sub_id, "failed", None, Some(&stderr))?;
        Err(AppError::Transient(format!(
            "gh pr review failed: {}",
            stderr
        )))
    }
}

#[tauri::command]
pub async fn get_submission_history(
    run_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<Vec<SubmissionRecord>, crate::errors::AppError> {
    use crate::errors::AppError;
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(queries::get_submission_history(&conn, &run_id)?)
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

async fn fetch_latest_diff(
    app: &AppHandle,
    owner: &str,
    repo: &str,
    pr_number: i32,
) -> Result<String, crate::errors::AppError> {
    use crate::errors::AppError;
    let shell = app.shell();
    let output = shell
        .command("gh")
        .args([
            "pr",
            "diff",
            &pr_number.to_string(),
            "--repo",
            &format!("{}/{}", owner, repo),
        ])
        .output()
        .await
        .map_err(|e| AppError::Transient(format!("Failed to fetch latest diff: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let msg = if stderr.is_empty() {
            "gh pr diff failed".to_string()
        } else {
            format!("gh pr diff failed: {}", stderr)
        };
        return Err(AppError::Transient(msg));
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn reconcile_findings_for_latest_diff(
    findings: Vec<Finding>,
    old_diff: &str,
    new_diff: &str,
) -> (Vec<Finding>, Vec<String>) {
    // 1. If the diff changed, try to remap anchors to account for hunk shifts.
    // 2. Always verify against the latest diff so inline anchors are safe.
    let (candidate, orphaned) = if old_diff == new_diff || old_diff.trim().is_empty() {
        (findings, vec![])
    } else {
        let result = remap::remap_findings(findings, old_diff, new_diff);
        let orphaned = result.orphaned.into_iter().map(|f| f.id).collect();
        (result.remapped, orphaned)
    };

    let candidate_ids: std::collections::HashSet<String> =
        candidate.iter().map(|f| f.id.clone()).collect();
    let verified = verify::verify(candidate, new_diff);
    let verified_ids: std::collections::HashSet<String> =
        verified.iter().map(|f| f.id.clone()).collect();

    // Anything orphaned or dropped by verify should be suppressed to avoid stale submissions.
    let mut suppressed: Vec<String> = orphaned;
    for id in candidate_ids.difference(&verified_ids) {
        suppressed.push(id.clone());
    }

    (verified, suppressed)
}

fn next_attempt_count(history: &[SubmissionRecord]) -> i32 {
    history
        .iter()
        .filter_map(|s| s.attempt_count)
        .max()
        .unwrap_or(0)
        + 1
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

    #[test]
    fn test_next_attempt_count_uses_max() {
        let h = vec![
            SubmissionRecord {
                id: "a".into(),
                review_run_id: "r".into(),
                review_action: "comment".into(),
                submitted_at: None,
                status: "failed".into(),
                gh_review_id: None,
                error_message: None,
                idempotency_key: None,
                attempt_count: Some(2),
                last_attempt_at: None,
            },
            SubmissionRecord {
                id: "b".into(),
                review_run_id: "r".into(),
                review_action: "comment".into(),
                submitted_at: None,
                status: "failed".into(),
                gh_review_id: None,
                error_message: None,
                idempotency_key: None,
                attempt_count: Some(5),
                last_attempt_at: None,
            },
        ];
        assert_eq!(next_attempt_count(&h), 6);
    }

    #[test]
    fn test_reconcile_findings_remaps_and_verifies() {
        let old_diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,6 +10,8 @@ fn foo() {\n     let x = 1;\n+    let y = 2;\n     process(x);\n";
        let new_diff = "diff --git a/src/a.rs b/src/a.rs\n--- a/src/a.rs\n+++ b/src/a.rs\n@@ -10,6 +15,8 @@ fn foo() {\n     let x = 1;\n+    let y = 2;\n     process(x);\n";

        let mut f = make_finding("warning", "t", "b", Some("src/a.rs"));
        f.id = "f1".into();
        f.line_start = Some(10);
        f.line_end = Some(10);
        f.is_anchored = true;

        let (updated, suppressed) = reconcile_findings_for_latest_diff(vec![f], old_diff, new_diff);
        assert!(suppressed.is_empty());
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].line_start, Some(15));
        assert_eq!(updated[0].diff_new_line, Some(15));
        assert_eq!(updated[0].diff_side.as_deref(), Some("RIGHT"));
    }
}
