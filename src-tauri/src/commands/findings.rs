use crate::errors::AppError;
use crate::metrics;
use crate::preferences::decisions::build_decision;
use crate::preferences::scoring;
use crate::storage::db::AppDb;
use crate::storage::queries;

fn record_reject_decision_for_suppressed(
    conn: &rusqlite::Connection,
    finding_id: &str,
) -> Result<(), AppError> {
    let finding = queries::get_finding_by_id(conn, finding_id)?
        .ok_or_else(|| AppError::NotFound(format!("Finding not found: {}", finding_id)))?;
    let d = build_decision(&finding, "reject", None);
    queries::insert_decision(conn, &d)?;

    // Recompute preference summaries for the affected agent_type (best-effort).
    let agent_decisions = queries::get_decisions_for_agent_type(conn, &finding.agent_type)?;
    let summaries = scoring::compute_preference_summaries(&agent_decisions);
    for summary in &summaries {
        let _ = queries::upsert_preference_summary(conn, summary);
    }

    Ok(())
}

#[tauri::command]
pub async fn update_finding(
    finding_id: String,
    body: Option<String>,
    severity: Option<String>,
    status: Option<String>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    queries::update_finding(
        &conn,
        &finding_id,
        body.as_deref(),
        severity.as_deref(),
        status.as_deref(),
    )?;

    if status.as_deref() == Some("suppressed") {
        if let Err(e) = record_reject_decision_for_suppressed(&conn, &finding_id) {
            tracing::warn!("Failed to record reject decision: {}", e);
        }
    }

    // Recompute run scorecard after finding update
    if let Ok(Some(finding)) = queries::get_finding_by_id(&conn, &finding_id) {
        if let Ok(scorecard) = metrics::compute_run_scorecard(&conn, &finding.review_run_id) {
            let _ = metrics::store_run_scorecard_cache(&conn, &finding.review_run_id, &scorecard);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::*;

    fn setup_test_finding(conn: &rusqlite::Connection) {
        let ws = Workspace {
            id: "ws1".into(),
            local_path: "/tmp/test".into(),
            remote_owner: "owner".into(),
            remote_repo: "repo".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            remote_host: "github.com".into(),
        };
        queries::insert_workspace(conn, &ws).unwrap();

        let pr = PullRequest {
            id: "pr1".into(),
            workspace_id: "ws1".into(),
            pr_number: 1,
            title: "Test PR".into(),
            author: None,
            base_branch: None,
            head_branch: None,
            url: "https://github.com/owner/repo/pull/1".into(),
            diff_text: None,
            changed_files: None,
            fetched_at: "2026-01-01T00:00:00Z".into(),
            diff_hash: None,
            platform_metadata_json: None,
            platform_metadata_fetched_at: None,
            platform_capabilities_json: None,
            platform_capabilities_fetched_at: None,
        };
        queries::insert_pull_request(conn, &pr).unwrap();

        let run = ReviewRun {
            id: "run1".into(),
            pr_id: "pr1".into(),
            status: "completed".into(),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            completed_at: Some("2026-01-01T00:01:00Z".into()),
            error_message: None,
            head_sha_at_run: None,
            baseline_run_id: None,
            metrics_json: None,
            analysis_diff_hash: None,
            analysis_diff_text: None,
            context_pack_json: None,
            local_checks_json: None,
            rerun_trigger_source: None,
            rerun_reason: None,
            rerun_scope: None,
        };
        queries::insert_review_run(conn, &run).unwrap();

        let finding = Finding {
            id: "f1".into(),
            review_run_id: "run1".into(),
            agent_type: "security".into(),
            file_path: Some("src/auth.rs".into()),
            line_start: Some(10),
            line_end: Some(20),
            severity: "warning".into(),
            confidence: 0.8,
            title: "Auth token validation issue".into(),
            body: "Token not validated properly".into(),
            evidence: None,
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: false,
            created_at: "2026-01-01T00:00:00Z".into(),
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
        };
        queries::insert_finding(conn, &finding).unwrap();
    }

    #[test]
    fn record_reject_decision_inserts_row() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        setup_test_finding(&conn);

        record_reject_decision_for_suppressed(&conn, "f1").unwrap();
        let decisions = queries::get_decisions_for_run(&conn, "run1").unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision, "reject");
        assert_eq!(decisions[0].finding_id, "f1");
    }
}
