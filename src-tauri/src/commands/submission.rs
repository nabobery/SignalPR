use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::cleaner::{remap, verify};
use crate::commands::intake::parse_pr_url;
use crate::preferences::{decisions, scoring};
use crate::storage::db::AppDb;
use crate::storage::event_log::EventLog;
use crate::storage::hashing::sha256_hex;
use crate::storage::models::{Finding, ReviewerDecision, SubmissionRecord};
use crate::storage::queries;

#[tauri::command]
pub async fn submit_review(
    app: AppHandle,
    run_id: String,
    action: String,
    force_resubmit: Option<bool>,
    review_summary_markdown: Option<String>,
    db: tauri::State<'_, AppDb>,
    event_log: tauri::State<'_, Arc<EventLog>>,
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
    let (pr_id, owner, repo, pr_number, mut findings, reviewer_decisions, old_diff, old_diff_hash) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let run = queries::get_review_run(&conn, &run_id)?
            .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;
        let pr = queries::get_pull_request(&conn, &run.pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let findings = queries::get_findings_for_run(&conn, &run_id)?;
        let reviewer_decisions = queries::get_decisions_for_run(&conn, &run_id)?;

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
            reviewer_decisions,
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

    let latest_decisions = latest_decisions_by_finding(&reviewer_decisions);

    // Filter to active findings only, excluding per-session deferred decisions.
    let active_findings: Vec<&Finding> = findings
        .iter()
        .filter(|f| should_include_in_submission(f, &latest_decisions))
        .collect();

    // Format review body with optional reviewer summary prepended
    let body = format_review_body(&active_findings, review_summary_markdown.as_deref());

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

    // Log submission started event
    let _ = event_log.append(
        &run_id,
        "submission_started",
        serde_json::json!({
            "action": action,
            "finding_count": active_findings.len(),
        }),
    );

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
        let now_rfc3339 = chrono::Utc::now().to_rfc3339();
        queries::update_submission_status(&conn, &sub_id, "submitted", None, None, &now_rfc3339)?;
        queries::update_review_run_status(&conn, &run_id, "submitted", None)?;

        // Best-effort: record reviewer decisions from the final submitted set.
        if let Err(e) = record_decisions_for_submission(&conn, &active_findings) {
            tracing::warn!("Failed to record reviewer decisions: {}", e);
        }

        // Recompute and persist run scorecard after submission
        if let Ok(scorecard) = crate::metrics::compute_run_scorecard(&conn, &run_id) {
            let _ = crate::metrics::store_run_scorecard_cache(&conn, &run_id, &scorecard);
        }

        // Log submission completed event
        let _ = event_log.append(
            &run_id,
            "submission_completed",
            serde_json::json!({
                "action": action,
                "finding_count": active_findings.len(),
            }),
        );

        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let now_rfc3339 = chrono::Utc::now().to_rfc3339();
        queries::update_submission_status(
            &conn,
            &sub_id,
            "failed",
            None,
            Some(&stderr),
            &now_rfc3339,
        )?;
        Err(AppError::Transient(format!(
            "gh pr review failed: {}",
            stderr
        )))
    }
}

fn record_decisions_for_submission(
    conn: &rusqlite::Connection,
    active_findings: &[&Finding],
) -> Result<(), crate::errors::AppError> {
    // Insert accept/edit decisions for all findings that made it into the final review body.
    for f in active_findings {
        let decision = if f.user_edited_body.is_some() || f.user_severity_override.is_some() {
            "edit"
        } else {
            "accept"
        };
        let d = decisions::build_decision(f, decision, None);
        let _ = queries::insert_decision(conn, &d);
    }

    // Refresh preference summaries for prompt-injection on subsequent runs.
    let all = queries::get_all_decisions(conn)?;
    let summaries = scoring::compute_preference_summaries(&all);
    for s in &summaries {
        let _ = queries::upsert_preference_summary(conn, s);
    }

    Ok(())
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

fn latest_decisions_by_finding(
    decisions: &[ReviewerDecision],
) -> std::collections::HashMap<String, String> {
    use chrono::{DateTime, Utc};
    use std::collections::HashMap;

    let mut latest: HashMap<String, (DateTime<Utc>, String)> = HashMap::new();
    for decision in decisions {
        let parsed = DateTime::parse_from_rfc3339(&decision.decided_at)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| DateTime::<Utc>::from(std::time::SystemTime::UNIX_EPOCH));
        match latest.get(&decision.finding_id) {
            Some((existing_ts, _)) if *existing_ts > parsed => {}
            _ => {
                latest.insert(
                    decision.finding_id.clone(),
                    (parsed, decision.decision.clone()),
                );
            }
        }
    }

    latest
        .into_iter()
        .map(|(id, (_, decision))| (id, decision))
        .collect()
}

fn should_include_in_submission(
    finding: &Finding,
    latest_decisions: &std::collections::HashMap<String, String>,
) -> bool {
    if finding.status != "active" {
        return false;
    }
    !matches!(
        latest_decisions.get(&finding.id).map(|d| d.as_str()),
        Some("skip")
    )
}

fn format_review_body(findings: &[&Finding], summary: Option<&str>) -> String {
    let mut body = String::new();
    body.push_str("## SignalPR Review\n\n");

    if let Some(s) = summary {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            body.push_str(trimmed);
            body.push_str("\n\n---\n\n");
        }
    }

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
            fix_search: None,
            fix_replace: None,
            fix_explanation: None,
            fix_status: None,
            fingerprint: None,
            source_kind: None,
            source_id: None,
            explain_json: None,
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
        let body = format_review_body(&findings, None);

        assert!(body.contains("## SignalPR Review"));
        assert!(body.contains("### Findings (2 issues)"));
        assert!(body.contains("**[BLOCKER]** Token bypass"));
        assert!(body.contains("`src/auth.rs:10-20`"));
        assert!(body.contains("**[WARNING]** N+1 query"));
        assert!(body.contains("*Reviewed with SignalPR*"));
    }

    #[test]
    fn test_format_review_body_empty() {
        let body = format_review_body(&[], None);
        assert!(body.contains("No significant findings"));
    }

    #[test]
    fn test_format_uses_edited_body() {
        let mut f = make_finding("warning", "Test", "Original", Some("file.rs"));
        f.user_edited_body = Some("Edited content".into());
        let findings: Vec<&Finding> = vec![&f];
        let body = format_review_body(&findings, None);
        assert!(body.contains("Edited content"));
        assert!(!body.contains("Original"));
    }

    #[test]
    fn test_format_uses_overridden_severity() {
        let mut f = make_finding("warning", "Test", "Body", Some("file.rs"));
        f.user_severity_override = Some("blocker".into());
        let findings: Vec<&Finding> = vec![&f];
        let body = format_review_body(&findings, None);
        assert!(body.contains("**[BLOCKER]**"));
    }

    #[test]
    fn test_format_multiline_body_blockquote() {
        let mut f = make_finding("warning", "Test", "Line 1\nLine 2", Some("file.rs"));
        f.user_edited_body = Some("Edited 1\nEdited 2".into());
        let findings: Vec<&Finding> = vec![&f];
        let body = format_review_body(&findings, None);
        assert!(body.contains("> Edited 1\n"));
        assert!(body.contains("> Edited 2\n"));
    }

    #[test]
    fn test_format_review_body_with_summary() {
        let f1 = make_finding("warning", "Test", "Body", Some("file.rs"));
        let findings: Vec<&Finding> = vec![&f1];
        let body = format_review_body(&findings, Some("Overall this PR looks good."));
        assert!(body.contains("## SignalPR Review"));
        assert!(body.contains("Overall this PR looks good."));
        assert!(body.contains("---"));
        assert!(body.contains("### Findings (1 issues)"));
    }

    #[test]
    fn test_format_review_body_with_empty_summary() {
        let f1 = make_finding("warning", "Test", "Body", Some("file.rs"));
        let findings: Vec<&Finding> = vec![&f1];
        let body = format_review_body(&findings, Some("   "));
        assert!(!body.contains("---\n\n###"));
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

    #[test]
    fn test_should_include_in_submission_excludes_skipped_findings() {
        use std::collections::HashMap;

        let mut f = make_finding("warning", "t", "b", Some("src/a.rs"));
        f.id = "f1".into();
        let included_without_decision = should_include_in_submission(&f, &HashMap::new());
        assert!(included_without_decision);

        let mut decisions = HashMap::new();
        decisions.insert("f1".to_string(), "skip".to_string());
        let included_with_skip = should_include_in_submission(&f, &decisions);
        assert!(!included_with_skip);
    }
}
