use std::collections::HashSet;
use std::sync::Arc;

use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::cleaner::{remap, verify};
use crate::preferences::{decisions, scoring};
use crate::storage::db::AppDb;
use crate::storage::event_log::EventLog;
use crate::storage::hashing::sha256_hex;
use crate::storage::models::{Finding, ReviewerDecision, SubmissionRecord};
use crate::storage::queries;

const INLINE_FINGERPRINT_PREFIX: &str = "<!-- signalpr:fingerprint=";
const INLINE_FINGERPRINT_SUFFIX: &str = " -->";

struct SubmissionContext<'a> {
    app: &'a AppHandle,
    owner: &'a str,
    repo: &'a str,
    pr_number: i32,
    action: &'a str,
    body: &'a str,
    active_findings: &'a [&'a Finding],
    anchors_verified: bool,
}

#[derive(Clone, Copy)]
enum SuggestionPlatform {
    GitHub,
    GitLab,
    Bitbucket,
}

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
    let (pr_id, pr_url, pr_number, mut findings, reviewer_decisions, old_diff, old_diff_hash) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let run = queries::get_review_run(&conn, &run_id)?
            .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;
        let pr = queries::get_pull_request(&conn, &run.pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let findings = queries::get_findings_for_run(&conn, &run_id)?;
        let reviewer_decisions = queries::get_decisions_for_run(&conn, &run_id)?;

        let old_diff = pr.diff_text.clone().unwrap_or_default();
        let old_hash = pr
            .diff_hash
            .clone()
            .unwrap_or_else(|| sha256_hex(&old_diff));

        (
            run.pr_id,
            pr.url,
            pr.pr_number,
            findings,
            reviewer_decisions,
            old_diff,
            old_hash,
        )
    };
    let review_url = crate::platform::parse_review_url(&pr_url)?;
    let adapter = crate::commands::pr_metadata::build_adapter(&app, &review_url).await?;

    // Best-effort: fetch latest diff and reconcile anchors for safe inline comments.
    let mut anchors_verified_for_submission = false;
    let originally_active_ids: std::collections::HashSet<String> = findings
        .iter()
        .filter(|f| f.status == "active")
        .map(|f| f.id.clone())
        .collect();

    if let Ok(latest_diff) = adapter.fetch_diff().await {
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

    // Resolve head SHA from metadata for coherent review submission and queue state tracking.
    let (head_sha, is_gitlab, is_bitbucket) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;

        let meta_json = pr.platform_metadata_json.as_deref();
        let sha = meta_json.and_then(|j| {
            if let Ok(pm) = serde_json::from_str::<crate::platform::adapter::PlatformMetadata>(j) {
                match &pm {
                    crate::platform::adapter::PlatformMetadata::GitHub(g) => {
                        Some(g.head_sha.clone())
                    }
                    crate::platform::adapter::PlatformMetadata::GitLab(g) => {
                        Some(g.head_sha.clone())
                    }
                    crate::platform::adapter::PlatformMetadata::Bitbucket(b) => {
                        Some(b.head_sha.clone())
                    }
                }
            } else {
                serde_json::from_str::<crate::providers::github::PlatformMetadataSnapshot>(j)
                    .ok()
                    .map(|m| m.head_sha)
            }
        });

        let is_gl = matches!(review_url, crate::platform::ParsedReviewUrl::GitLab { .. });
        let is_bb = matches!(
            review_url,
            crate::platform::ParsedReviewUrl::Bitbucket { .. }
        );
        (sha, is_gl, is_bb)
    };

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
                commit_id_at_submission: head_sha.clone(),
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

    // Route submission by platform
    let (gh_review_id, used_native) = if is_gitlab || is_bitbucket {
        let suggestion_platform = if is_bitbucket {
            SuggestionPlatform::Bitbucket
        } else {
            SuggestionPlatform::GitLab
        };

        let inline_comments: Vec<crate::platform::adapter::InlineComment> = active_findings
            .iter()
            .filter(|f| f.is_anchored && anchors_verified_for_submission)
            .filter_map(|f| {
                let path = f.file_path.as_ref()?;
                let line = f.diff_new_line.or(f.line_start);
                let sev_label = f
                    .user_severity_override
                    .as_deref()
                    .unwrap_or(&f.severity)
                    .to_uppercase();
                let fingerprint = f.fingerprint.as_deref().unwrap_or(&f.id).to_string();
                let comment_body =
                    build_inline_comment_body(f, &sev_label, &fingerprint, suggestion_platform);
                Some(crate::platform::adapter::InlineComment {
                    path: path.clone(),
                    body: comment_body,
                    line,
                    side: f.diff_side.clone(),
                    start_line: None,
                })
            })
            .collect();

        let adapter_event = match action.as_str() {
            "approve" => "approve",
            "request-changes" => "request_changes",
            _ => "comment",
        };

        let payload = crate::platform::adapter::SubmissionPayload {
            body: body.clone(),
            event: adapter_event.to_string(),
            inline_comments,
            commit_id: head_sha.clone().unwrap_or_default(),
        };

        adapter.submit_review(payload).await?;
        (None, true)
    } else {
        let (owner, repo) = match &review_url {
            crate::platform::ParsedReviewUrl::GitHub { owner, repo, .. } => {
                (owner.clone(), repo.clone())
            }
            _ => {
                return Err(AppError::InvalidInput(
                    "Expected GitHub review URL for native GitHub submission".into(),
                ));
            }
        };
        let submission_ctx = SubmissionContext {
            app: &app,
            owner: &owner,
            repo: &repo,
            pr_number,
            action: &action,
            body: &body,
            active_findings: &active_findings,
            anchors_verified: anchors_verified_for_submission,
        };

        let submission_result = if let Some(ref sha) = head_sha {
            submit_via_native_api(&submission_ctx, sha).await
        } else {
            Err(AppError::InvalidInput(
                "No head SHA available; falling back to gh CLI".into(),
            ))
        };

        match submission_result {
            Ok(review_id) => (Some(review_id), true),
            Err(native_err) => {
                tracing::info!(
                    "Native API submission unavailable ({}); falling back to gh CLI",
                    native_err
                );
                submit_via_gh_cli(&submission_ctx).await?;
                (None, false)
            }
        }
    };

    // Record success
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let now_rfc3339 = chrono::Utc::now().to_rfc3339();
        queries::update_submission_status(
            &conn,
            &sub_id,
            "submitted",
            gh_review_id.as_deref(),
            None,
            &now_rfc3339,
        )?;
        queries::update_review_run_status(&conn, &run_id, "submitted", None)?;

        if let Err(e) = record_decisions_for_submission(&conn, &active_findings) {
            tracing::warn!("Failed to record reviewer decisions: {}", e);
        }

        if let Ok(scorecard) = crate::metrics::compute_run_scorecard(&conn, &run_id) {
            let _ = crate::metrics::store_run_scorecard_cache(&conn, &run_id, &scorecard);
        }
    }

    let _ = event_log.append(
        &run_id,
        "submission_completed",
        serde_json::json!({
            "action": action,
            "finding_count": active_findings.len(),
            "used_native_api": used_native,
            "gh_review_id": gh_review_id,
        }),
    );

    Ok(())
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

/// Submit a single coherent GitHub review via `POST /pulls/{n}/reviews`.
/// Returns the review ID string on success.
async fn submit_via_native_api(
    ctx: &SubmissionContext<'_>,
    head_sha: &str,
) -> Result<String, crate::errors::AppError> {
    use crate::errors::AppError;
    use crate::providers::github::{
        resolve_api_from_app, CreateReviewPayload, ReviewCommentPayload,
    };

    let (api, _) = resolve_api_from_app(ctx.app).await?;
    let existing_fingerprints = fetch_existing_inline_fingerprints(&api, ctx).await;

    let event = match ctx.action {
        "approve" => "APPROVE",
        "request-changes" => "REQUEST_CHANGES",
        _ => "COMMENT",
    };

    let comments: Vec<ReviewCommentPayload> = if ctx.anchors_verified {
        let mut comments = Vec::new();
        let mut seen_fingerprints = existing_fingerprints;
        for finding in ctx.active_findings.iter().copied() {
            let Some(path) = finding.file_path.as_ref() else {
                continue;
            };
            let Some(line) = finding.diff_new_line else {
                continue;
            };

            let severity_label = finding
                .user_severity_override
                .as_deref()
                .unwrap_or(&finding.severity)
                .to_uppercase();
            let fingerprint = inline_comment_fingerprint(finding);
            if !seen_fingerprints.insert(fingerprint.clone()) {
                continue;
            }
            let comment_body = build_inline_comment_body(
                finding,
                &severity_label,
                &fingerprint,
                SuggestionPlatform::GitHub,
            );

            let (start_line, start_side) = if let Some(ls) = finding.line_start {
                if ls < line {
                    (Some(ls), finding.diff_side.clone())
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };

            comments.push(ReviewCommentPayload {
                path: path.clone(),
                body: comment_body,
                line: Some(line),
                side: finding.diff_side.clone().or_else(|| Some("RIGHT".into())),
                start_line,
                start_side,
            });
        }
        comments
    } else {
        vec![]
    };

    let payload = CreateReviewPayload {
        commit_id: head_sha.to_string(),
        body: ctx.body.to_string(),
        event: event.to_string(),
        comments,
    };

    let result = api
        .create_review(ctx.owner, ctx.repo, ctx.pr_number, &payload)
        .await
        .map_err(|e| AppError::Transient(format!("Native review submission failed: {}", e)))?;

    Ok(result.id.to_string())
}

/// Fallback: submit via `gh pr review` CLI + separate inline comments.
async fn submit_via_gh_cli(ctx: &SubmissionContext<'_>) -> Result<(), crate::errors::AppError> {
    use crate::errors::AppError;

    let shell = ctx.app.shell();
    let gh_action = match ctx.action {
        "approve" => "--approve",
        "request-changes" => "--request-changes",
        _ => "--comment",
    };

    let output = shell
        .command("gh")
        .args([
            "pr",
            "review",
            &ctx.pr_number.to_string(),
            "--repo",
            &format!("{}/{}", ctx.owner, ctx.repo),
            gh_action,
            "--body",
            ctx.body,
        ])
        .output()
        .await
        .map_err(|e| AppError::Transient(format!("Failed to run gh: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        return Err(AppError::Transient(format!(
            "gh pr review failed: {}",
            stderr
        )));
    }

    if ctx.anchors_verified {
        let inline_findings: Vec<&Finding> = ctx
            .active_findings
            .iter()
            .filter(|f| f.diff_new_line.is_some() && f.file_path.is_some())
            .copied()
            .collect();
        if !inline_findings.is_empty() {
            if let Err(e) = post_inline_comments(ctx, &inline_findings).await {
                tracing::warn!("Inline comments (best-effort) failed: {}", e);
            }
        }
    }

    Ok(())
}

/// Phase B: Post inline review comments for findings with valid diff anchors.
///
/// Uses the GitHub REST API endpoint:
///   POST /repos/{owner}/{repo}/pulls/{pr_number}/comments
///
/// This is best-effort — failures are logged but never bubble up to the caller,
/// keeping the main `gh pr review` submission (Phase A) reliable.
async fn post_inline_comments(
    ctx: &SubmissionContext<'_>,
    findings: &[&Finding],
) -> Result<(), String> {
    let shell = ctx.app.shell();
    let mut existing_fingerprints =
        match crate::providers::github::resolve_api_from_app(ctx.app).await {
            Ok((api, _)) => fetch_existing_inline_fingerprints(&api, ctx).await,
            Err(_) => HashSet::new(),
        };

    // Get HEAD commit SHA (needed for the inline comment API)
    let sha_output = shell
        .command("gh")
        .args([
            "pr",
            "view",
            &ctx.pr_number.to_string(),
            "--repo",
            &format!("{}/{}", ctx.owner, ctx.repo),
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
        let fingerprint = inline_comment_fingerprint(finding);
        if !existing_fingerprints.insert(fingerprint.clone()) {
            continue;
        }
        let comment_body = build_inline_comment_body(
            finding,
            &severity_label,
            &fingerprint,
            SuggestionPlatform::GitHub,
        );

        tracing::info!(
            "Posting inline comment on {}:{} — {}",
            file_path,
            line,
            finding.title
        );

        let line_str = line.to_string();
        let endpoint = format!(
            "repos/{}/{}/pulls/{}/comments",
            ctx.owner, ctx.repo, ctx.pr_number
        );

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

async fn fetch_existing_inline_fingerprints(
    api: &crate::providers::github::GitHubApi,
    ctx: &SubmissionContext<'_>,
) -> HashSet<String> {
    match api
        .list_review_comments(ctx.owner, ctx.repo, ctx.pr_number)
        .await
    {
        Ok(comments) => comments
            .into_iter()
            .filter_map(|comment| extract_inline_fingerprint(&comment.body))
            .collect(),
        Err(err) => {
            tracing::warn!(
                "Unable to list existing inline comments for dedupe: {}",
                err
            );
            HashSet::new()
        }
    }
}

fn extract_inline_fingerprint(body: &str) -> Option<String> {
    let start = body.find(INLINE_FINGERPRINT_PREFIX)?;
    let marker_start = start + INLINE_FINGERPRINT_PREFIX.len();
    let remainder = &body[marker_start..];
    let end_offset = remainder.find(INLINE_FINGERPRINT_SUFFIX)?;
    let raw = remainder[..end_offset].trim();
    if raw.is_empty() {
        None
    } else {
        Some(raw.to_string())
    }
}

fn inline_comment_fingerprint(finding: &Finding) -> String {
    finding.fingerprint.clone().unwrap_or_else(|| {
        sha256_hex(&format!(
            "{}|{}|{}|{}|{}",
            finding.file_path.as_deref().unwrap_or_default(),
            finding.diff_new_line.unwrap_or_default(),
            finding.title,
            finding.severity,
            finding.body
        ))
    })
}

fn render_suggestion_block(
    replacement: &str,
    platform: SuggestionPlatform,
    old_line_span: usize,
) -> String {
    let header = match platform {
        SuggestionPlatform::GitHub => "suggestion".to_string(),
        SuggestionPlatform::GitLab => {
            let bounded_span = old_line_span.clamp(1, 101);
            format!("suggestion:-0+{}", bounded_span.saturating_sub(1))
        }
        SuggestionPlatform::Bitbucket => {
            // Bitbucket has no first-class suggestion semantics; render as a plain code block.
            "diff".to_string()
        }
    };
    if !replacement.contains("```") {
        return format!("```{}\n{}\n```", header, replacement);
    }
    if !replacement.contains("~~~") {
        return format!("~~~{}\n{}\n~~~", header, replacement);
    }
    format!("````{}\n{}\n````", header, replacement)
}

fn build_inline_comment_body(
    finding: &Finding,
    severity_label: &str,
    fingerprint: &str,
    platform: SuggestionPlatform,
) -> String {
    let display_body = finding.user_edited_body.as_deref().unwrap_or(&finding.body);
    let mut comment_body = format!(
        "**[{}]** {}\n\n{}",
        severity_label, finding.title, display_body
    );

    if let (Some(search), Some(replace)) = (&finding.fix_search, &finding.fix_replace) {
        if !search.trim().is_empty() {
            let old_line_span = search.lines().count().max(1);
            comment_body.push_str("\n\n");
            comment_body.push_str(&render_suggestion_block(replace, platform, old_line_span));
        }
    }

    comment_body.push_str("\n\n");
    comment_body.push_str(INLINE_FINGERPRINT_PREFIX);
    comment_body.push_str(fingerprint);
    comment_body.push_str(INLINE_FINGERPRINT_SUFFIX);
    comment_body
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
                commit_id_at_submission: None,
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
                commit_id_at_submission: None,
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

    #[test]
    fn test_render_suggestion_block_plain() {
        let result = render_suggestion_block("let x = 1;", SuggestionPlatform::GitHub, 1);
        assert_eq!(result, "```suggestion\nlet x = 1;\n```");
    }

    #[test]
    fn test_render_suggestion_block_with_backticks() {
        let result = render_suggestion_block(
            "use `foo::bar`;\nlet x = ```nested```;",
            SuggestionPlatform::GitHub,
            1,
        );
        assert_eq!(
            result,
            "~~~suggestion\nuse `foo::bar`;\nlet x = ```nested```;\n~~~"
        );
    }

    #[test]
    fn test_render_suggestion_block_with_both_fences() {
        let code = "triple ``` and tilde ~~~ in same code";
        let result = render_suggestion_block(code, SuggestionPlatform::GitHub, 1);
        assert!(result.starts_with("````suggestion\n"));
        assert!(result.ends_with("\n````"));
    }

    #[test]
    fn test_render_suggestion_block_gitlab_multiline() {
        let result = render_suggestion_block("line one\nline two", SuggestionPlatform::GitLab, 2);
        assert_eq!(result, "```suggestion:-0+1\nline one\nline two\n```");
    }

    #[test]
    fn test_build_inline_comment_body_includes_fingerprint() {
        let f = make_finding(
            "warning",
            "Unused var",
            "Variable not used",
            Some("src/a.rs"),
        );
        let body = build_inline_comment_body(&f, "WARNING", "fp123", SuggestionPlatform::GitHub);
        assert!(body.contains("**[WARNING]** Unused var"));
        assert!(body.contains("Variable not used"));
        assert!(body.contains("fp123"));
    }

    #[test]
    fn test_build_inline_comment_body_with_fix_suggestion() {
        let mut f = make_finding("warning", "Fix me", "Body", Some("src/a.rs"));
        f.fix_search = Some("old_code".into());
        f.fix_replace = Some("new_code".into());
        let body = build_inline_comment_body(&f, "WARNING", "fp", SuggestionPlatform::GitHub);
        assert!(body.contains("```suggestion\nnew_code\n```"));
    }

    #[test]
    fn test_build_inline_comment_body_with_gitlab_fix_suggestion() {
        let mut f = make_finding("warning", "Fix me", "Body", Some("src/a.rs"));
        f.fix_search = Some("old_a\nold_b".into());
        f.fix_replace = Some("new_a\nnew_b".into());
        let body = build_inline_comment_body(&f, "WARNING", "fp", SuggestionPlatform::GitLab);
        assert!(body.contains("```suggestion:-0+1\nnew_a\nnew_b\n```"));
    }
}
