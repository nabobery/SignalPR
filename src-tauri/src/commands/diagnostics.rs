use std::sync::Arc;

use serde::Serialize;

use crate::errors::AppError;
use crate::storage::db::AppDb;
use crate::storage::event_log::{EventLog, TimestampedEvent};
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct DiagnosticBundle {
    pub run_id: String,
    pub status: String,
    pub pr_number: i32,
    pub pr_title: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub agent_runs: Vec<AgentRunSummary>,
    pub finding_count: usize,
    pub events: Vec<TimestampedEvent>,
}

#[derive(Debug, Serialize)]
pub struct AgentRunSummary {
    pub lane_id: String,
    pub provider_name: String,
    pub status: String,
    pub finding_count: i32,
    pub error_message: Option<String>,
}

#[tauri::command]
pub async fn export_diagnostic_bundle(
    run_id: String,
    db: tauri::State<'_, AppDb>,
    event_log: tauri::State<'_, Arc<EventLog>>,
) -> Result<DiagnosticBundle, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let run = queries::get_review_run(&conn, &run_id)?
        .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;

    let pr = queries::get_pull_request(&conn, &run.pr_id)?
        .ok_or_else(|| AppError::NotFound("PR not found".into()))?;

    let agent_runs = queries::get_agent_runs_for_review(&conn, &run_id)?;
    let findings = queries::get_findings_for_run(&conn, &run_id)?;

    let agent_summaries: Vec<AgentRunSummary> = agent_runs
        .iter()
        .map(|ar| AgentRunSummary {
            lane_id: ar.lane_id.clone(),
            provider_name: ar.provider_name.clone(),
            status: ar.status.clone(),
            finding_count: ar.finding_count,
            error_message: ar.error_message.clone(),
        })
        .collect();

    // Read event log (best effort — missing file is OK)
    let events = event_log.read(&run_id).unwrap_or_default();

    Ok(DiagnosticBundle {
        run_id: run.id,
        status: run.status,
        pr_number: pr.pr_number,
        pr_title: pr.title,
        started_at: run.started_at,
        completed_at: run.completed_at,
        error_message: run.error_message,
        agent_runs: agent_summaries,
        finding_count: findings.len(),
        events,
    })
}

#[tauri::command]
pub async fn get_event_log(
    run_id: String,
    event_log: tauri::State<'_, Arc<EventLog>>,
) -> Result<Vec<TimestampedEvent>, AppError> {
    event_log.read(&run_id).map_err(AppError::Io)
}
