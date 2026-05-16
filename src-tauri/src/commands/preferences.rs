use crate::errors::AppError;
use crate::metrics;
use crate::preferences::decisions::build_decision;
use crate::preferences::scoring::compute_preference_summaries;
use crate::storage::db::AppDb;
use crate::storage::models::PreferenceSummary;
use crate::storage::queries;

#[tauri::command]
pub async fn record_decision(
    finding_id: String,
    decision: String,
    time_to_decision_ms: Option<i64>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    // Validate decision value
    if !matches!(decision.as_str(), "accept" | "reject" | "edit" | "skip") {
        return Err(AppError::InvalidInput(format!(
            "Invalid decision '{}': must be accept, reject, edit, or skip",
            decision
        )));
    }

    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    // Look up the finding
    let finding = queries::get_finding_by_id(&conn, &finding_id)?
        .ok_or_else(|| AppError::NotFound(format!("Finding not found: {}", finding_id)))?;

    // Build and insert the decision
    let reviewer_decision = build_decision(&finding, &decision, time_to_decision_ms);
    let agent_type = reviewer_decision.original_agent_type.clone();
    queries::insert_decision(&conn, &reviewer_decision)?;

    // Recompute preference summaries for the affected agent_type
    let agent_decisions = queries::get_decisions_for_agent_type(&conn, &agent_type)?;
    let summaries = compute_preference_summaries(&agent_decisions);
    for summary in &summaries {
        queries::upsert_preference_summary(&conn, summary)?;
    }

    // Recompute run scorecard after decision change
    let run_id = &finding.review_run_id;
    if let Ok(scorecard) = metrics::compute_run_scorecard(&conn, run_id) {
        let _ = metrics::store_run_scorecard_cache(&conn, run_id, &scorecard);
    }

    Ok(())
}

#[tauri::command]
pub async fn get_preferences(
    db: tauri::State<'_, AppDb>,
) -> Result<Vec<PreferenceSummary>, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(queries::get_preference_summaries(&conn)?)
}

#[cfg(test)]
mod tests {
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::*;
    use crate::storage::queries;

    fn setup_test_run(conn: &rusqlite::Connection) {
        // Create workspace -> PR -> review run chain (once)
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
        };
        queries::insert_review_run(conn, &run).unwrap();
    }

    fn insert_test_finding(conn: &rusqlite::Connection, id: &str) -> Finding {
        let finding = Finding {
            id: id.into(),
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
        finding
    }

    #[test]
    fn test_record_and_retrieve_decision() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        setup_test_run(&conn);
        let _finding = insert_test_finding(&conn, "f1");

        // Simulate what record_decision does
        let finding = queries::get_finding_by_id(&conn, "f1").unwrap().unwrap();
        let decision =
            crate::preferences::decisions::build_decision(&finding, "accept", Some(1500));
        queries::insert_decision(&conn, &decision).unwrap();

        let decisions = queries::get_all_decisions(&conn).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision, "accept");
        assert_eq!(decisions[0].finding_id, "f1");
    }

    #[test]
    fn test_preference_summaries_computed_after_decisions() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        setup_test_run(&conn);
        let f1 = insert_test_finding(&conn, "f1");
        let f2 = insert_test_finding(&conn, "f2");
        let f3 = insert_test_finding(&conn, "f3");

        // Record multiple decisions
        let d1 = crate::preferences::decisions::build_decision(&f1, "accept", Some(500));
        let d2 = crate::preferences::decisions::build_decision(&f2, "accept", Some(500));
        let d3 = crate::preferences::decisions::build_decision(&f3, "reject", Some(500));
        queries::insert_decision(&conn, &d1).unwrap();
        queries::insert_decision(&conn, &d2).unwrap();
        queries::insert_decision(&conn, &d3).unwrap();

        // Recompute summaries
        let agent_decisions = queries::get_decisions_for_agent_type(&conn, "security").unwrap();
        let summaries = crate::preferences::scoring::compute_preference_summaries(&agent_decisions);

        assert!(!summaries.is_empty());
        // With 2 accepts and 1 reject (all recent), rate should be ~0.67
        let s = &summaries[0];
        assert!(s.accept_rate > 0.6 && s.accept_rate < 0.7);
        assert_eq!(s.total_decisions, 3);

        // Upsert and retrieve
        for summary in &summaries {
            queries::upsert_preference_summary(&conn, summary).unwrap();
        }
        let stored = queries::get_preference_summaries(&conn).unwrap();
        assert!(!stored.is_empty());
    }
}
