#![allow(dead_code)]
use rusqlite::{params, Connection};

use super::models::*;

// --- Workspaces ---

pub fn insert_workspace(conn: &Connection, ws: &Workspace) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO workspaces (id, local_path, remote_owner, remote_repo, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![ws.id, ws.local_path, ws.remote_owner, ws.remote_repo, ws.created_at],
    )?;
    Ok(())
}

pub fn get_workspace_by_remote(
    conn: &Connection,
    owner: &str,
    repo: &str,
) -> Result<Option<Workspace>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, local_path, remote_owner, remote_repo, created_at FROM workspaces WHERE remote_owner = ?1 AND remote_repo = ?2 LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![owner, repo], |row| {
        Ok(Workspace {
            id: row.get(0)?,
            local_path: row.get(1)?,
            remote_owner: row.get(2)?,
            remote_repo: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

// --- Pull Requests ---

pub fn insert_pull_request(conn: &Connection, pr: &PullRequest) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO pull_requests (id, workspace_id, pr_number, title, author, base_branch, head_branch, url, diff_text, changed_files, fetched_at, diff_hash, platform_metadata_json, platform_metadata_fetched_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![pr.id, pr.workspace_id, pr.pr_number, pr.title, pr.author, pr.base_branch, pr.head_branch, pr.url, pr.diff_text, pr.changed_files, pr.fetched_at, pr.diff_hash, pr.platform_metadata_json, pr.platform_metadata_fetched_at],
    )?;
    Ok(())
}

pub fn get_pull_request(
    conn: &Connection,
    pr_id: &str,
) -> Result<Option<PullRequest>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, pr_number, title, author, base_branch, head_branch, url, diff_text, changed_files, fetched_at, diff_hash, platform_metadata_json, platform_metadata_fetched_at FROM pull_requests WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![pr_id], |row| {
        Ok(PullRequest {
            id: row.get(0)?,
            workspace_id: row.get(1)?,
            pr_number: row.get(2)?,
            title: row.get(3)?,
            author: row.get(4)?,
            base_branch: row.get(5)?,
            head_branch: row.get(6)?,
            url: row.get(7)?,
            diff_text: row.get(8)?,
            changed_files: row.get(9)?,
            fetched_at: row.get(10)?,
            diff_hash: row.get(11)?,
            platform_metadata_json: row.get(12)?,
            platform_metadata_fetched_at: row.get(13)?,
        })
    })?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

pub fn update_pull_request_diff(
    conn: &Connection,
    pr_id: &str,
    diff_text: &str,
    diff_hash: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE pull_requests SET diff_text = ?1, diff_hash = ?2, fetched_at = datetime('now') WHERE id = ?3",
        params![diff_text, diff_hash, pr_id],
    )?;
    Ok(())
}

pub fn update_pull_request_metadata(
    conn: &Connection,
    pr_id: &str,
    metadata_json: &str,
    fetched_at_rfc3339: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE pull_requests SET platform_metadata_json = ?1, platform_metadata_fetched_at = ?2 WHERE id = ?3",
        params![metadata_json, fetched_at_rfc3339, pr_id],
    )?;
    Ok(())
}

// --- Review Runs ---

pub fn insert_review_run(conn: &Connection, run: &ReviewRun) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO review_runs (id, pr_id, status, started_at, baseline_run_id, metrics_json, analysis_diff_hash, analysis_diff_text, context_pack_json, local_checks_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![run.id, run.pr_id, run.status, run.started_at, run.baseline_run_id, run.metrics_json, run.analysis_diff_hash, run.analysis_diff_text, run.context_pack_json, run.local_checks_json],
    )?;
    Ok(())
}

pub fn update_review_run_status(
    conn: &Connection,
    run_id: &str,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), rusqlite::Error> {
    if status == "ready" || status == "submitted" || status == "failed" {
        conn.execute(
            "UPDATE review_runs SET status = ?1, completed_at = datetime('now'), error_message = ?2 WHERE id = ?3",
            params![status, error_message, run_id],
        )?;
    } else {
        conn.execute(
            "UPDATE review_runs SET status = ?1 WHERE id = ?2",
            params![status, run_id],
        )?;
    }
    Ok(())
}

pub fn get_review_run(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ReviewRun>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, pr_id, status, started_at, completed_at, error_message, baseline_run_id, metrics_json, analysis_diff_hash, analysis_diff_text, context_pack_json, local_checks_json FROM review_runs WHERE id = ?1",
    )?;
    let mut rows = stmt.query_map(params![run_id], |row| {
        Ok(ReviewRun {
            id: row.get(0)?,
            pr_id: row.get(1)?,
            status: row.get(2)?,
            started_at: row.get(3)?,
            completed_at: row.get(4)?,
            error_message: row.get(5)?,
            baseline_run_id: row.get(6)?,
            metrics_json: row.get(7)?,
            analysis_diff_hash: row.get(8)?,
            analysis_diff_text: row.get(9)?,
            context_pack_json: row.get(10)?,
            local_checks_json: row.get(11)?,
        })
    })?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

pub fn get_incomplete_review_runs(conn: &Connection) -> Result<Vec<ReviewRun>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, pr_id, status, started_at, completed_at, error_message, baseline_run_id, metrics_json, analysis_diff_hash, analysis_diff_text, context_pack_json, local_checks_json FROM review_runs WHERE status NOT IN ('ready', 'submitted', 'failed') ORDER BY started_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ReviewRun {
            id: row.get(0)?,
            pr_id: row.get(1)?,
            status: row.get(2)?,
            started_at: row.get(3)?,
            completed_at: row.get(4)?,
            error_message: row.get(5)?,
            baseline_run_id: row.get(6)?,
            metrics_json: row.get(7)?,
            analysis_diff_hash: row.get(8)?,
            analysis_diff_text: row.get(9)?,
            context_pack_json: row.get(10)?,
            local_checks_json: row.get(11)?,
        })
    })?;
    rows.collect()
}

// --- Findings ---

pub fn insert_finding(conn: &Connection, f: &Finding) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO findings (id, review_run_id, agent_type, file_path, line_start, line_end, severity, confidence, title, body, evidence, status, user_edited_body, user_severity_override, is_anchored, created_at, cluster_id, lane_id, provider_name, diff_side, diff_new_line, fix_search, fix_replace, fix_explanation, fix_status, fingerprint, source_kind, source_id, explain_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28, ?29)",
        params![
            f.id, f.review_run_id, f.agent_type, f.file_path, f.line_start, f.line_end,
            f.severity, f.confidence, f.title, f.body, f.evidence, f.status,
            f.user_edited_body, f.user_severity_override, f.is_anchored, f.created_at,
            f.cluster_id, f.lane_id, f.provider_name, f.diff_side, f.diff_new_line,
            f.fix_search, f.fix_replace, f.fix_explanation, f.fix_status, f.fingerprint,
            f.source_kind, f.source_id, f.explain_json
        ],
    )?;
    Ok(())
}

pub fn update_finding(
    conn: &Connection,
    finding_id: &str,
    body: Option<&str>,
    severity: Option<&str>,
    status: Option<&str>,
) -> Result<(), rusqlite::Error> {
    if let Some(b) = body {
        conn.execute(
            "UPDATE findings SET user_edited_body = ?1 WHERE id = ?2",
            params![b, finding_id],
        )?;
    }
    if let Some(s) = severity {
        conn.execute(
            "UPDATE findings SET user_severity_override = ?1 WHERE id = ?2",
            params![s, finding_id],
        )?;
    }
    if let Some(st) = status {
        conn.execute(
            "UPDATE findings SET status = ?1 WHERE id = ?2",
            params![st, finding_id],
        )?;
    }
    Ok(())
}

pub fn update_finding_anchor(
    conn: &Connection,
    finding_id: &str,
    line_start: Option<i32>,
    line_end: Option<i32>,
    is_anchored: bool,
    diff_side: Option<&str>,
    diff_new_line: Option<i32>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE findings SET line_start = ?1, line_end = ?2, is_anchored = ?3, diff_side = ?4, diff_new_line = ?5 WHERE id = ?6",
        params![line_start, line_end, is_anchored, diff_side, diff_new_line, finding_id],
    )?;
    Ok(())
}

fn row_to_finding(row: &rusqlite::Row) -> Result<Finding, rusqlite::Error> {
    Ok(Finding {
        id: row.get(0)?,
        review_run_id: row.get(1)?,
        agent_type: row.get(2)?,
        file_path: row.get(3)?,
        line_start: row.get(4)?,
        line_end: row.get(5)?,
        severity: row.get(6)?,
        confidence: row.get(7)?,
        title: row.get(8)?,
        body: row.get(9)?,
        evidence: row.get(10)?,
        status: row.get(11)?,
        user_edited_body: row.get(12)?,
        user_severity_override: row.get(13)?,
        is_anchored: row.get(14)?,
        created_at: row.get(15)?,
        cluster_id: row.get(16)?,
        lane_id: row.get(17)?,
        provider_name: row.get(18)?,
        diff_side: row.get(19)?,
        diff_new_line: row.get(20)?,
        fix_search: row.get(21)?,
        fix_replace: row.get(22)?,
        fix_explanation: row.get(23)?,
        fix_status: row.get(24)?,
        fingerprint: row.get(25)?,
        source_kind: row.get(26)?,
        source_id: row.get(27)?,
        explain_json: row.get(28)?,
    })
}

const FINDING_COLUMNS: &str = "id, review_run_id, agent_type, file_path, line_start, line_end, severity, confidence, title, body, evidence, status, user_edited_body, user_severity_override, is_anchored, created_at, cluster_id, lane_id, provider_name, diff_side, diff_new_line, fix_search, fix_replace, fix_explanation, fix_status, fingerprint, source_kind, source_id, explain_json";

pub fn get_findings_for_run(
    conn: &Connection,
    review_run_id: &str,
) -> Result<Vec<Finding>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM findings WHERE review_run_id = ?1 ORDER BY CASE severity WHEN 'blocker' THEN 1 WHEN 'critical' THEN 2 WHEN 'warning' THEN 3 WHEN 'info' THEN 4 WHEN 'nitpick' THEN 5 ELSE 6 END, confidence DESC",
        FINDING_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![review_run_id], row_to_finding)?;
    rows.collect()
}

pub fn get_finding_by_id(
    conn: &Connection,
    finding_id: &str,
) -> Result<Option<Finding>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM findings WHERE id = ?1 LIMIT 1",
        FINDING_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![finding_id], row_to_finding)?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

// --- Agent Runs ---

pub fn insert_agent_run(conn: &Connection, ar: &AgentRun) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO agent_runs (id, review_run_id, lane_id, provider_name, status, started_at, completed_at, finding_count, error_message, governance_tier_at_run, provider_session_id, resume_cursor, checkpoint_metadata_json, cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![ar.id, ar.review_run_id, ar.lane_id, ar.provider_name, ar.status, ar.started_at, ar.completed_at, ar.finding_count, ar.error_message, ar.governance_tier_at_run, ar.provider_session_id, ar.resume_cursor, ar.checkpoint_metadata_json, ar.cost_usd],
    )?;
    Ok(())
}

pub fn update_agent_run(
    conn: &Connection,
    id: &str,
    status: &str,
    completed_at: Option<&str>,
    finding_count: i32,
    error_message: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE agent_runs SET status = ?1, completed_at = ?2, finding_count = ?3, error_message = ?4 WHERE id = ?5",
        params![status, completed_at, finding_count, error_message, id],
    )?;
    Ok(())
}

/// Update the session metadata fields on an agent_run after the provider returns.
pub fn update_agent_run_metadata(
    conn: &Connection,
    id: &str,
    provider_session_id: Option<&str>,
    resume_cursor: Option<&str>,
    checkpoint_metadata_json: Option<&str>,
    cost_usd: Option<f64>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE agent_runs SET provider_session_id = ?1, resume_cursor = ?2, checkpoint_metadata_json = ?3, cost_usd = ?4 WHERE id = ?5",
        params![provider_session_id, resume_cursor, checkpoint_metadata_json, cost_usd, id],
    )?;
    Ok(())
}

pub fn get_agent_runs_for_review(
    conn: &Connection,
    review_run_id: &str,
) -> Result<Vec<AgentRun>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, review_run_id, lane_id, provider_name, status, started_at, completed_at, finding_count, error_message, governance_tier_at_run, provider_session_id, resume_cursor, checkpoint_metadata_json, cost_usd FROM agent_runs WHERE review_run_id = ?1 ORDER BY started_at ASC, lane_id ASC",
    )?;
    let rows = stmt.query_map(params![review_run_id], |row| {
        Ok(AgentRun {
            id: row.get(0)?,
            review_run_id: row.get(1)?,
            lane_id: row.get(2)?,
            provider_name: row.get(3)?,
            status: row.get(4)?,
            started_at: row.get(5)?,
            completed_at: row.get(6)?,
            finding_count: row.get(7)?,
            error_message: row.get(8)?,
            governance_tier_at_run: row.get(9)?,
            provider_session_id: row.get(10)?,
            resume_cursor: row.get(11)?,
            checkpoint_metadata_json: row.get(12)?,
            cost_usd: row.get(13)?,
        })
    })?;
    rows.collect()
}

// --- Finding Clusters ---

pub fn insert_finding_cluster(
    conn: &Connection,
    fc: &FindingCluster,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO finding_clusters (id, review_run_id, label, representative_finding_id, member_count, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![fc.id, fc.review_run_id, fc.label, fc.representative_finding_id, fc.member_count, fc.created_at],
    )?;
    Ok(())
}

pub fn get_clusters_for_run(
    conn: &Connection,
    review_run_id: &str,
) -> Result<Vec<FindingCluster>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, review_run_id, label, representative_finding_id, member_count, created_at FROM finding_clusters WHERE review_run_id = ?1",
    )?;
    let rows = stmt.query_map(params![review_run_id], |row| {
        Ok(FindingCluster {
            id: row.get(0)?,
            review_run_id: row.get(1)?,
            label: row.get(2)?,
            representative_finding_id: row.get(3)?,
            member_count: row.get(4)?,
            created_at: row.get(5)?,
        })
    })?;
    rows.collect()
}

// --- Settings ---

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

pub fn upsert_setting(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO settings (key, value, updated_at) VALUES (?1, ?2, datetime('now'))",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_all_settings(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect::<Result<std::collections::HashMap<_, _>, _>>()
}

pub fn delete_setting(conn: &Connection, key: &str) -> Result<bool, rusqlite::Error> {
    let count = conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
    Ok(count > 0)
}

pub fn get_settings_by_prefix(
    conn: &Connection,
    prefix: &str,
) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let pattern = format!("{}%", prefix);
    let mut stmt = conn.prepare("SELECT key, value FROM settings WHERE key LIKE ?1")?;
    let rows = stmt.query_map(params![pattern], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    rows.collect()
}

// --- Submission Records ---

pub fn insert_submission(conn: &Connection, sub: &SubmissionRecord) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO submission_records (id, review_run_id, review_action, submitted_at, status, gh_review_id, error_message, idempotency_key, attempt_count, last_attempt_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![sub.id, sub.review_run_id, sub.review_action, sub.submitted_at, sub.status, sub.gh_review_id, sub.error_message, sub.idempotency_key, sub.attempt_count, sub.last_attempt_at],
    )?;
    Ok(())
}

pub fn update_submission_status(
    conn: &Connection,
    sub_id: &str,
    status: &str,
    gh_review_id: Option<&str>,
    error_message: Option<&str>,
    timestamp_rfc3339: &str,
) -> Result<(), rusqlite::Error> {
    if status == "submitted" {
        conn.execute(
            "UPDATE submission_records SET status = ?1, gh_review_id = ?2, error_message = ?3, last_attempt_at = ?4, submitted_at = ?4 WHERE id = ?5",
            params![status, gh_review_id, error_message, timestamp_rfc3339, sub_id],
        )?;
    } else {
        conn.execute(
            "UPDATE submission_records SET status = ?1, gh_review_id = ?2, error_message = ?3, last_attempt_at = ?4 WHERE id = ?5",
            params![status, gh_review_id, error_message, timestamp_rfc3339, sub_id],
        )?;
    }
    Ok(())
}

pub fn get_submission_for_run(
    conn: &Connection,
    review_run_id: &str,
) -> Result<Option<SubmissionRecord>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, review_run_id, review_action, submitted_at, status, gh_review_id, error_message, idempotency_key, attempt_count, last_attempt_at FROM submission_records WHERE review_run_id = ?1 AND status = 'submitted' LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![review_run_id], |row| {
        Ok(SubmissionRecord {
            id: row.get(0)?,
            review_run_id: row.get(1)?,
            review_action: row.get(2)?,
            submitted_at: row.get(3)?,
            status: row.get(4)?,
            gh_review_id: row.get(5)?,
            error_message: row.get(6)?,
            idempotency_key: row.get(7)?,
            attempt_count: row.get(8)?,
            last_attempt_at: row.get(9)?,
        })
    })?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

pub fn get_submission_history(
    conn: &Connection,
    review_run_id: &str,
) -> Result<Vec<SubmissionRecord>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, review_run_id, review_action, submitted_at, status, gh_review_id, error_message, idempotency_key, attempt_count, last_attempt_at FROM submission_records WHERE review_run_id = ?1 ORDER BY COALESCE(last_attempt_at, submitted_at) DESC",
    )?;
    let rows = stmt.query_map(params![review_run_id], |row| {
        Ok(SubmissionRecord {
            id: row.get(0)?,
            review_run_id: row.get(1)?,
            review_action: row.get(2)?,
            submitted_at: row.get(3)?,
            status: row.get(4)?,
            gh_review_id: row.get(5)?,
            error_message: row.get(6)?,
            idempotency_key: row.get(7)?,
            attempt_count: row.get(8)?,
            last_attempt_at: row.get(9)?,
        })
    })?;
    rows.collect()
}

// --- Tool Status ---

pub fn upsert_tool_status(conn: &Connection, ts: &ToolStatus) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO tool_status (tool_name, status, version, message, checked_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![ts.tool_name, ts.status, ts.version, ts.message, ts.checked_at],
    )?;
    Ok(())
}

// --- Reviewer Decisions ---

pub fn insert_decision(conn: &Connection, d: &ReviewerDecision) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO reviewer_decisions (id, finding_id, review_run_id, decision, original_severity, original_agent_type, category_tag, time_to_decision_ms, decided_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![d.id, d.finding_id, d.review_run_id, d.decision, d.original_severity, d.original_agent_type, d.category_tag, d.time_to_decision_ms, d.decided_at],
    )?;
    Ok(())
}

pub fn get_decisions_for_run(
    conn: &Connection,
    review_run_id: &str,
) -> Result<Vec<ReviewerDecision>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, finding_id, review_run_id, decision, original_severity, original_agent_type, category_tag, time_to_decision_ms, decided_at FROM reviewer_decisions WHERE review_run_id = ?1 ORDER BY decided_at DESC",
    )?;
    let rows = stmt.query_map(params![review_run_id], |row| {
        Ok(ReviewerDecision {
            id: row.get(0)?,
            finding_id: row.get(1)?,
            review_run_id: row.get(2)?,
            decision: row.get(3)?,
            original_severity: row.get(4)?,
            original_agent_type: row.get(5)?,
            category_tag: row.get(6)?,
            time_to_decision_ms: row.get(7)?,
            decided_at: row.get(8)?,
        })
    })?;
    rows.collect()
}

pub fn get_all_decisions(conn: &Connection) -> Result<Vec<ReviewerDecision>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, finding_id, review_run_id, decision, original_severity, original_agent_type, category_tag, time_to_decision_ms, decided_at FROM reviewer_decisions ORDER BY decided_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(ReviewerDecision {
            id: row.get(0)?,
            finding_id: row.get(1)?,
            review_run_id: row.get(2)?,
            decision: row.get(3)?,
            original_severity: row.get(4)?,
            original_agent_type: row.get(5)?,
            category_tag: row.get(6)?,
            time_to_decision_ms: row.get(7)?,
            decided_at: row.get(8)?,
        })
    })?;
    rows.collect()
}

// --- Preference Summaries ---

pub fn upsert_preference_summary(
    conn: &Connection,
    ps: &PreferenceSummary,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO preference_summaries (id, agent_type, category_tag, accept_rate, total_decisions, last_updated) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![ps.id, ps.agent_type, ps.category_tag, ps.accept_rate, ps.total_decisions, ps.last_updated],
    )?;
    Ok(())
}

pub fn get_preference_summaries(
    conn: &Connection,
) -> Result<Vec<PreferenceSummary>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, agent_type, category_tag, accept_rate, total_decisions, last_updated FROM preference_summaries ORDER BY agent_type, category_tag",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(PreferenceSummary {
            id: row.get(0)?,
            agent_type: row.get(1)?,
            category_tag: row.get(2)?,
            accept_rate: row.get(3)?,
            total_decisions: row.get(4)?,
            last_updated: row.get(5)?,
        })
    })?;
    rows.collect()
}

pub fn get_decisions_for_agent_type(
    conn: &Connection,
    agent_type: &str,
) -> Result<Vec<ReviewerDecision>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, finding_id, review_run_id, decision, original_severity, original_agent_type, category_tag, time_to_decision_ms, decided_at FROM reviewer_decisions WHERE original_agent_type = ?1 ORDER BY decided_at DESC",
    )?;
    let rows = stmt.query_map(params![agent_type], |row| {
        Ok(ReviewerDecision {
            id: row.get(0)?,
            finding_id: row.get(1)?,
            review_run_id: row.get(2)?,
            decision: row.get(3)?,
            original_severity: row.get(4)?,
            original_agent_type: row.get(5)?,
            category_tag: row.get(6)?,
            time_to_decision_ms: row.get(7)?,
            decided_at: row.get(8)?,
        })
    })?;
    rows.collect()
}

// --- Review Drafts ---

pub fn get_review_draft(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ReviewDraft>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT run_id, summary_markdown, review_action, updated_at FROM review_drafts WHERE run_id = ?1",
    )?;
    let mut rows = stmt.query_map(params![run_id], |row| {
        Ok(ReviewDraft {
            run_id: row.get(0)?,
            summary_markdown: row.get(1)?,
            review_action: row.get(2)?,
            updated_at: row.get(3)?,
        })
    })?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

pub fn save_review_draft(
    conn: &Connection,
    run_id: &str,
    summary_markdown: &str,
    review_action: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO review_drafts (run_id, summary_markdown, review_action, updated_at) VALUES (?1, ?2, ?3, datetime('now'))",
        params![run_id, summary_markdown, review_action],
    )?;
    Ok(())
}

// --- Inbox Overview Queries ---

pub fn list_recent_review_runs(
    conn: &Connection,
    limit: i32,
) -> Result<Vec<InboxReviewRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.pr_id, p.pr_number, p.title, p.author, p.url, r.status,
                COALESCE(r.completed_at, r.started_at, '') as last_updated,
                (SELECT COUNT(*) FROM findings f WHERE f.review_run_id = r.id AND f.status = 'active') as active_count,
                COALESCE(
                    (SELECT GROUP_CONCAT(DISTINCT ar.provider_name)
                     FROM agent_runs ar
                     WHERE ar.review_run_id = r.id AND ar.provider_name IS NOT NULL),
                    ''
                ) as providers_csv
         FROM review_runs r
         JOIN pull_requests p ON r.pr_id = p.id
         WHERE r.status IN ('ready', 'submitted', 'failed')
         ORDER BY last_updated DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        let providers_csv: String = row.get(9)?;
        let providers_used = if providers_csv.is_empty() {
            Vec::new()
        } else {
            providers_csv
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        };
        Ok(InboxReviewRow {
            run_id: row.get(0)?,
            pr_id: row.get(1)?,
            pr_number: row.get(2)?,
            title: row.get(3)?,
            author: row.get(4)?,
            pr_url: row.get(5)?,
            status: row.get(6)?,
            last_updated: row.get(7)?,
            active_finding_count: row.get(8)?,
            providers_used,
        })
    })?;
    rows.collect()
}

pub fn list_incomplete_review_runs_enriched(
    conn: &Connection,
    limit: i32,
) -> Result<Vec<InboxReviewRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.pr_id, p.pr_number, p.title, p.author, p.url, r.status,
                COALESCE(r.started_at, '') as last_updated,
                (SELECT COUNT(*) FROM findings f WHERE f.review_run_id = r.id AND f.status = 'active') as active_count,
                COALESCE(
                    (SELECT GROUP_CONCAT(DISTINCT ar.provider_name)
                     FROM agent_runs ar
                     WHERE ar.review_run_id = r.id AND ar.provider_name IS NOT NULL),
                    ''
                ) as providers_csv
         FROM review_runs r
         JOIN pull_requests p ON r.pr_id = p.id
         WHERE r.status NOT IN ('ready', 'submitted', 'failed')
         ORDER BY r.started_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        let providers_csv: String = row.get(9)?;
        let providers_used = if providers_csv.is_empty() {
            Vec::new()
        } else {
            providers_csv
                .split(',')
                .filter(|s| !s.trim().is_empty())
                .map(|s| s.trim().to_string())
                .collect()
        };
        Ok(InboxReviewRow {
            run_id: row.get(0)?,
            pr_id: row.get(1)?,
            pr_number: row.get(2)?,
            title: row.get(3)?,
            author: row.get(4)?,
            pr_url: row.get(5)?,
            status: row.get(6)?,
            last_updated: row.get(7)?,
            active_finding_count: row.get(8)?,
            providers_used,
        })
    })?;
    rows.collect()
}

pub fn list_recent_workspaces(
    conn: &Connection,
    limit: i32,
) -> Result<Vec<InboxWorkspaceRow>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT w.id, w.local_path, w.remote_owner, w.remote_repo,
                COALESCE(MAX(r.started_at), w.created_at) as last_reviewed_at
         FROM workspaces w
         LEFT JOIN pull_requests p ON p.workspace_id = w.id
         LEFT JOIN review_runs r ON r.pr_id = p.id
         GROUP BY w.id, w.local_path, w.remote_owner, w.remote_repo, w.created_at
         ORDER BY last_reviewed_at DESC
         LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], |row| {
        Ok(InboxWorkspaceRow {
            workspace_id: row.get(0)?,
            local_path: row.get(1)?,
            remote_owner: row.get(2)?,
            remote_repo: row.get(3)?,
            last_reviewed_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

pub fn get_all_tool_status(conn: &Connection) -> Result<Vec<ToolStatus>, rusqlite::Error> {
    let mut stmt =
        conn.prepare("SELECT tool_name, status, version, message, checked_at FROM tool_status")?;
    let rows = stmt.query_map([], |row| {
        Ok(ToolStatus {
            tool_name: row.get(0)?,
            status: row.get(1)?,
            version: row.get(2)?,
            message: row.get(3)?,
            checked_at: row.get(4)?,
        })
    })?;
    rows.collect()
}

// --- Workspaces (by ID) ---

pub fn get_workspace_by_id(
    conn: &Connection,
    workspace_id: &str,
) -> Result<Option<Workspace>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, local_path, remote_owner, remote_repo, created_at FROM workspaces WHERE id = ?1 LIMIT 1",
    )?;
    let mut rows = stmt.query_map(params![workspace_id], |row| {
        Ok(Workspace {
            id: row.get(0)?,
            local_path: row.get(1)?,
            remote_owner: row.get(2)?,
            remote_repo: row.get(3)?,
            created_at: row.get(4)?,
        })
    })?;
    match rows.next() {
        Some(result) => Ok(Some(result?)),
        None => Ok(None),
    }
}

// --- Review Run Metrics/Analysis ---

pub fn update_review_run_metrics(
    conn: &Connection,
    run_id: &str,
    metrics_json: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE review_runs SET metrics_json = ?1 WHERE id = ?2",
        params![metrics_json, run_id],
    )?;
    Ok(())
}

pub fn update_review_run_analysis_diff(
    conn: &Connection,
    run_id: &str,
    analysis_diff_text: &str,
    analysis_diff_hash: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE review_runs SET analysis_diff_text = ?1, analysis_diff_hash = ?2 WHERE id = ?3",
        params![analysis_diff_text, analysis_diff_hash, run_id],
    )?;
    Ok(())
}

// --- Review Run Artifacts (Phase 3) ---

pub fn update_review_run_context_pack(
    conn: &Connection,
    run_id: &str,
    context_pack_json: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE review_runs SET context_pack_json = ?1 WHERE id = ?2",
        params![context_pack_json, run_id],
    )?;
    Ok(())
}

pub fn update_review_run_local_checks(
    conn: &Connection,
    run_id: &str,
    local_checks_json: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE review_runs SET local_checks_json = ?1 WHERE id = ?2",
        params![local_checks_json, run_id],
    )?;
    Ok(())
}

pub fn update_finding_explain(
    conn: &Connection,
    finding_id: &str,
    explain_json: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE findings SET explain_json = ?1 WHERE id = ?2",
        params![explain_json, finding_id],
    )?;
    Ok(())
}

// --- Finding Fix Status ---

pub fn update_finding_fix_status(
    conn: &Connection,
    finding_id: &str,
    status: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE findings SET fix_status = ?1 WHERE id = ?2",
        params![status, finding_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;

    fn test_db() -> Connection {
        let db = init_db_in_memory().unwrap();
        db.0.into_inner().unwrap()
    }

    #[test]
    fn test_workspace_round_trip() {
        let conn = test_db();
        let ws = Workspace {
            id: "ws-1".into(),
            local_path: "/home/user/repo".into(),
            remote_owner: "octocat".into(),
            remote_repo: "hello-world".into(),
            created_at: "2026-03-27T00:00:00".into(),
        };
        insert_workspace(&conn, &ws).unwrap();
        let found = get_workspace_by_remote(&conn, "octocat", "hello-world")
            .unwrap()
            .expect("workspace should exist");
        assert_eq!(found.id, "ws-1");
        assert_eq!(found.local_path, "/home/user/repo");
    }

    #[test]
    fn test_workspace_not_found() {
        let conn = test_db();
        let found = get_workspace_by_remote(&conn, "nobody", "nothing").unwrap();
        assert!(found.is_none());
    }

    #[test]
    fn test_pull_request_round_trip() {
        let conn = test_db();
        let ws = Workspace {
            id: "ws-1".into(),
            local_path: "/tmp".into(),
            remote_owner: "o".into(),
            remote_repo: "r".into(),
            created_at: "2026-01-01T00:00:00".into(),
        };
        insert_workspace(&conn, &ws).unwrap();

        let pr = PullRequest {
            id: "pr-1".into(),
            workspace_id: "ws-1".into(),
            pr_number: 42,
            title: "Fix auth".into(),
            author: Some("alice".into()),
            base_branch: Some("main".into()),
            head_branch: Some("fix-auth".into()),
            url: "https://github.com/o/r/pull/42".into(),
            diff_text: Some("diff content".into()),
            changed_files: Some(r#"["src/auth.rs"]"#.into()),
            fetched_at: "2026-01-01T00:00:00".into(),
            diff_hash: None,
            platform_metadata_json: None,
            platform_metadata_fetched_at: None,
        };
        insert_pull_request(&conn, &pr).unwrap();
        let found = get_pull_request(&conn, "pr-1")
            .unwrap()
            .expect("PR should exist");
        assert_eq!(found.pr_number, 42);
        assert_eq!(found.title, "Fix auth");
    }

    #[test]
    fn test_update_pull_request_diff_updates_hash_and_text() {
        let conn = test_db();
        insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: Some("old".into()),
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00Z".into(),
                diff_hash: Some("h1".into()),
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();

        update_pull_request_diff(&conn, "pr", "new-diff", "h2").unwrap();
        let pr = get_pull_request(&conn, "pr").unwrap().unwrap();
        assert_eq!(pr.diff_text.as_deref(), Some("new-diff"));
        assert_eq!(pr.diff_hash.as_deref(), Some("h2"));
    }

    #[test]
    fn test_foreign_key_constraint_rejects_orphan_pr() {
        let conn = test_db();
        let pr = PullRequest {
            id: "pr-orphan".into(),
            workspace_id: "nonexistent-ws".into(),
            pr_number: 1,
            title: "orphan".into(),
            author: None,
            base_branch: None,
            head_branch: None,
            url: "https://example.com".into(),
            diff_text: None,
            changed_files: None,
            fetched_at: "2026-01-01T00:00:00".into(),
            diff_hash: None,
            platform_metadata_json: None,
            platform_metadata_fetched_at: None,
        };
        let result = insert_pull_request(&conn, &pr);
        assert!(result.is_err());
    }

    #[test]
    fn test_review_run_status_transitions() {
        let conn = test_db();
        // Setup workspace + PR
        insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();

        let run = ReviewRun {
            id: "run-1".into(),
            pr_id: "pr".into(),
            status: "created".into(),
            started_at: Some("2026-01-01T00:00:00".into()),
            completed_at: None,
            error_message: None,
            baseline_run_id: None,
            metrics_json: None,
            analysis_diff_hash: None,
            analysis_diff_text: None,
            context_pack_json: None,
            local_checks_json: None,
        };
        insert_review_run(&conn, &run).unwrap();

        update_review_run_status(&conn, "run-1", "running_agents", None).unwrap();
        let updated = get_review_run(&conn, "run-1").unwrap().unwrap();
        assert_eq!(updated.status, "running_agents");

        update_review_run_status(&conn, "run-1", "ready", None).unwrap();
        let updated = get_review_run(&conn, "run-1").unwrap().unwrap();
        assert_eq!(updated.status, "ready");
        assert!(updated.completed_at.is_some());
    }

    #[test]
    fn test_findings_crud() {
        let conn = test_db();
        // Setup chain: workspace → PR → review run
        insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();
        insert_review_run(
            &conn,
            &ReviewRun {
                id: "run".into(),
                pr_id: "pr".into(),
                status: "ready".into(),
                started_at: None,
                completed_at: None,
                error_message: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
            },
        )
        .unwrap();

        let finding = Finding {
            id: "f-1".into(),
            review_run_id: "run".into(),
            agent_type: "security".into(),
            file_path: Some("src/auth.rs".into()),
            line_start: Some(10),
            line_end: Some(20),
            severity: "blocker".into(),
            confidence: 0.95,
            title: "Token bypass".into(),
            body: "Auth is bypassed".into(),
            evidence: Some(r#"["middleware skipped"]"#.into()),
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: true,
            created_at: "2026-01-01T00:00:00".into(),
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
        insert_finding(&conn, &finding).unwrap();

        let findings = get_findings_for_run(&conn, "run").unwrap();
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Token bypass");

        // Update finding
        update_finding(&conn, "f-1", Some("Edited body"), None, None).unwrap();
        let findings = get_findings_for_run(&conn, "run").unwrap();
        assert_eq!(findings[0].user_edited_body.as_deref(), Some("Edited body"));

        // Suppress finding
        update_finding(&conn, "f-1", None, None, Some("suppressed")).unwrap();
        let findings = get_findings_for_run(&conn, "run").unwrap();
        assert_eq!(findings[0].status, "suppressed");

        // Demote anchor fields
        update_finding_anchor(&conn, "f-1", None, None, false, None, None).unwrap();
        let findings = get_findings_for_run(&conn, "run").unwrap();
        assert_eq!(findings[0].line_start, None);
        assert_eq!(findings[0].line_end, None);
        assert!(!findings[0].is_anchored);
    }

    #[test]
    fn test_findings_ordered_by_severity() {
        let conn = test_db();
        insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();
        insert_review_run(
            &conn,
            &ReviewRun {
                id: "run".into(),
                pr_id: "pr".into(),
                status: "ready".into(),
                started_at: None,
                completed_at: None,
                error_message: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
            },
        )
        .unwrap();

        // Insert in reverse severity order
        for (id, sev, conf) in [
            ("f1", "info", 0.5),
            ("f2", "blocker", 0.9),
            ("f3", "warning", 0.7),
        ] {
            insert_finding(
                &conn,
                &Finding {
                    id: id.into(),
                    review_run_id: "run".into(),
                    agent_type: "security".into(),
                    file_path: None,
                    line_start: None,
                    line_end: None,
                    severity: sev.into(),
                    confidence: conf,
                    title: format!("{} finding", sev),
                    body: "body".into(),
                    evidence: None,
                    status: "active".into(),
                    user_edited_body: None,
                    user_severity_override: None,
                    is_anchored: false,
                    created_at: "2026-01-01T00:00:00".into(),
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
                },
            )
            .unwrap();
        }

        let findings = get_findings_for_run(&conn, "run").unwrap();
        assert_eq!(findings[0].severity, "blocker");
        assert_eq!(findings[1].severity, "warning");
        assert_eq!(findings[2].severity, "info");
    }

    #[test]
    fn test_tool_status_upsert() {
        let conn = test_db();
        let ts = ToolStatus {
            tool_name: "gh".into(),
            status: "ready".into(),
            version: Some("2.50.0".into()),
            message: None,
            checked_at: "2026-01-01T00:00:00".into(),
        };
        upsert_tool_status(&conn, &ts).unwrap();

        let all = get_all_tool_status(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, "ready");

        // Upsert with new status
        let ts2 = ToolStatus {
            tool_name: "gh".into(),
            status: "unauthenticated".into(),
            version: Some("2.50.0".into()),
            message: Some("Run: gh auth login".into()),
            checked_at: "2026-01-01T01:00:00".into(),
        };
        upsert_tool_status(&conn, &ts2).unwrap();
        let all = get_all_tool_status(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].status, "unauthenticated");
    }

    #[test]
    fn test_update_submission_status_sets_last_attempt_and_only_sets_submitted_at_on_success() {
        let conn = test_db();
        insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00Z".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();
        insert_review_run(
            &conn,
            &ReviewRun {
                id: "run".into(),
                pr_id: "pr".into(),
                status: "ready".into(),
                started_at: None,
                completed_at: None,
                error_message: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
            },
        )
        .unwrap();

        insert_submission(
            &conn,
            &SubmissionRecord {
                id: "sub".into(),
                review_run_id: "run".into(),
                review_action: "comment".into(),
                submitted_at: None,
                status: "pending".into(),
                gh_review_id: None,
                error_message: None,
                idempotency_key: Some("k".into()),
                attempt_count: Some(1),
                last_attempt_at: None,
            },
        )
        .unwrap();

        update_submission_status(
            &conn,
            "sub",
            "failed",
            None,
            Some("nope"),
            "2026-01-01T00:00:05Z",
        )
        .unwrap();
        let hist = get_submission_history(&conn, "run").unwrap();
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].status, "failed");
        assert!(hist[0].last_attempt_at.is_some());
        assert!(hist[0].submitted_at.is_none());

        update_submission_status(
            &conn,
            "sub",
            "submitted",
            Some("gh1"),
            None,
            "2026-01-01T00:00:10Z",
        )
        .unwrap();
        let hist = get_submission_history(&conn, "run").unwrap();
        assert_eq!(hist[0].status, "submitted");
        assert!(hist[0].submitted_at.is_some());
        assert!(hist[0].last_attempt_at.is_some());
    }

    #[test]
    fn test_decision_insert_and_query() {
        let conn = test_db();
        // Setup chain
        insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();
        insert_review_run(
            &conn,
            &ReviewRun {
                id: "run".into(),
                pr_id: "pr".into(),
                status: "ready".into(),
                started_at: None,
                completed_at: None,
                error_message: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
            },
        )
        .unwrap();
        insert_finding(
            &conn,
            &Finding {
                id: "f1".into(),
                review_run_id: "run".into(),
                agent_type: "security".into(),
                file_path: None,
                line_start: None,
                line_end: None,
                severity: "warning".into(),
                confidence: 0.8,
                title: "Test".into(),
                body: "Body".into(),
                evidence: None,
                status: "active".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: false,
                created_at: "2026-01-01T00:00:00".into(),
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
            },
        )
        .unwrap();

        let decision = ReviewerDecision {
            id: "d1".into(),
            finding_id: "f1".into(),
            review_run_id: "run".into(),
            decision: "accept".into(),
            original_severity: "warning".into(),
            original_agent_type: "security".into(),
            category_tag: Some("auth".into()),
            time_to_decision_ms: Some(1500),
            decided_at: "2026-03-27T00:00:00".into(),
        };
        insert_decision(&conn, &decision).unwrap();

        let decisions = get_decisions_for_run(&conn, "run").unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].decision, "accept");
        assert_eq!(decisions[0].original_agent_type, "security");
        assert_eq!(decisions[0].category_tag.as_deref(), Some("auth"));

        let all = get_all_decisions(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_preference_summary_upsert() {
        let conn = test_db();
        let ps = PreferenceSummary {
            id: "ps1".into(),
            agent_type: "security".into(),
            category_tag: Some("auth".into()),
            accept_rate: 0.75,
            total_decisions: 10,
            last_updated: "2026-03-27T00:00:00".into(),
        };
        upsert_preference_summary(&conn, &ps).unwrap();

        let summaries = get_preference_summaries(&conn).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].accept_rate, 0.75);
        assert_eq!(summaries[0].total_decisions, 10);

        // Update accept_rate
        let ps2 = PreferenceSummary {
            id: "ps1".into(),
            agent_type: "security".into(),
            category_tag: Some("auth".into()),
            accept_rate: 0.85,
            total_decisions: 15,
            last_updated: "2026-03-28T00:00:00".into(),
        };
        upsert_preference_summary(&conn, &ps2).unwrap();
        let summaries = get_preference_summaries(&conn).unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].accept_rate, 0.85);
        assert_eq!(summaries[0].total_decisions, 15);
    }

    // --- V6: Review Draft tests ---

    fn setup_run(conn: &Connection) {
        insert_workspace(
            conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00".into(),
            },
        )
        .unwrap();
        insert_pull_request(
            conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 42,
                title: "Fix auth".into(),
                author: Some("alice".into()),
                base_branch: None,
                head_branch: None,
                url: "https://github.com/o/r/pull/42".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();
        insert_review_run(
            conn,
            &ReviewRun {
                id: "run".into(),
                pr_id: "pr".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00".into()),
                completed_at: Some("2026-01-01T00:01:00".into()),
                error_message: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
            },
        )
        .unwrap();
    }

    #[test]
    fn test_review_draft_round_trip() {
        let conn = test_db();
        setup_run(&conn);

        assert!(get_review_draft(&conn, "run").unwrap().is_none());

        save_review_draft(&conn, "run", "LGTM overall", "approve").unwrap();
        let draft = get_review_draft(&conn, "run")
            .unwrap()
            .expect("draft should exist");
        assert_eq!(draft.run_id, "run");
        assert_eq!(draft.summary_markdown, "LGTM overall");
        assert_eq!(draft.review_action, "approve");

        save_review_draft(&conn, "run", "Actually, needs fixes", "request-changes").unwrap();
        let draft = get_review_draft(&conn, "run").unwrap().unwrap();
        assert_eq!(draft.summary_markdown, "Actually, needs fixes");
        assert_eq!(draft.review_action, "request-changes");
    }

    #[test]
    fn test_list_recent_review_runs() {
        let conn = test_db();
        setup_run(&conn);

        let recent = list_recent_review_runs(&conn, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].run_id, "run");
        assert_eq!(recent[0].pr_number, 42);
        assert_eq!(recent[0].title, "Fix auth");
        assert_eq!(recent[0].author.as_deref(), Some("alice"));
        assert_eq!(recent[0].status, "ready");
    }

    #[test]
    fn test_list_incomplete_review_runs_enriched() {
        let conn = test_db();
        setup_run(&conn);

        let incomplete = list_incomplete_review_runs_enriched(&conn, 10).unwrap();
        assert!(incomplete.is_empty(), "ready run should not be incomplete");

        insert_review_run(
            &conn,
            &ReviewRun {
                id: "run2".into(),
                pr_id: "pr".into(),
                status: "running_agents".into(),
                started_at: Some("2026-01-02T00:00:00".into()),
                completed_at: None,
                error_message: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
            },
        )
        .unwrap();

        let incomplete = list_incomplete_review_runs_enriched(&conn, 10).unwrap();
        assert_eq!(incomplete.len(), 1);
        assert_eq!(incomplete[0].run_id, "run2");
        assert_eq!(incomplete[0].status, "running_agents");
    }

    #[test]
    fn test_inbox_queries_include_finding_counts() {
        let conn = test_db();
        setup_run(&conn);

        insert_finding(
            &conn,
            &Finding {
                id: "f1".into(),
                review_run_id: "run".into(),
                agent_type: "security".into(),
                file_path: None,
                line_start: None,
                line_end: None,
                severity: "warning".into(),
                confidence: 0.8,
                title: "Test".into(),
                body: "Body".into(),
                evidence: None,
                status: "active".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: false,
                created_at: "2026-01-01T00:00:00".into(),
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
            },
        )
        .unwrap();
        insert_finding(
            &conn,
            &Finding {
                id: "f2".into(),
                review_run_id: "run".into(),
                agent_type: "security".into(),
                file_path: None,
                line_start: None,
                line_end: None,
                severity: "info".into(),
                confidence: 0.5,
                title: "Test2".into(),
                body: "Body2".into(),
                evidence: None,
                status: "suppressed".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: false,
                created_at: "2026-01-01T00:00:00".into(),
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
            },
        )
        .unwrap();

        let recent = list_recent_review_runs(&conn, 10).unwrap();
        assert_eq!(recent[0].active_finding_count, 1);
    }

    #[test]
    fn test_list_recent_workspaces() {
        let conn = test_db();
        setup_run(&conn);

        let workspaces = list_recent_workspaces(&conn, 10).unwrap();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].workspace_id, "ws");
        assert_eq!(workspaces[0].remote_owner, "o");
        assert_eq!(workspaces[0].remote_repo, "r");
    }
}
