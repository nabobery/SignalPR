use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::config;
use crate::orchestration::engine;
use crate::orchestration::lane::{AgentLaneConfig, LaneSnapshot};
use crate::providers::prompts::{self, AgentFocus};
use crate::providers::traits::ReviewProvider;
use crate::storage::db::AppDb;
use crate::storage::models::{Finding, FindingCluster, ReviewRun};
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct ReviewSnapshot {
    pub run_id: String,
    pub status: String,
    pub pr_title: String,
    pub pr_number: i32,
    pub pr_url: String,
    pub diff_text: Option<String>,
    pub changed_files: Vec<String>,
    pub findings: Vec<Finding>,
    pub error_message: Option<String>,
    pub lane_statuses: Vec<LaneSnapshot>,
    pub clusters: Vec<FindingCluster>,
}

pub struct ActiveReviews(pub Mutex<HashMap<String, CancellationToken>>);

fn require_non_empty_diff(diff: Option<String>) -> Result<String, crate::errors::AppError> {
    use crate::errors::AppError;
    let diff = diff.unwrap_or_default();
    if diff.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "PR diff is missing. Re-open the PR from Intake to refetch the diff.".into(),
        ));
    }
    Ok(diff)
}

#[tauri::command]
pub async fn start_review(
    app: AppHandle,
    pr_id: String,
    db: tauri::State<'_, AppDb>,
    active: tauri::State<'_, ActiveReviews>,
) -> Result<String, crate::errors::AppError> {
    use crate::errors::AppError;

    // Get PR data
    let (diff, cwd) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let ws = conn.query_row(
            "SELECT local_path FROM workspaces WHERE id = ?1",
            rusqlite::params![pr.workspace_id],
            |row| row.get::<_, String>(0),
        )?;
        let diff = require_non_empty_diff(pr.diff_text)?;
        (diff, ws)
    };

    if cwd.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Workspace local path is not set. Confirm workspace before starting review.".into(),
        ));
    }

    // Load repo-level config if available
    let repo_config = config::load_repo_config(&PathBuf::from(&cwd));

    // Resolve config from DB settings + repo config
    let resolved = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        config::resolve_config(&conn, repo_config.as_ref())
    };

    // Choose provider based on preference
    let provider: Arc<dyn ReviewProvider> =
        config::select_provider(&app, &resolved.preferred_provider).await;

    let cwd_path = PathBuf::from(&cwd);

    // Build multi-lane configs: Security, Architecture, Performance
    let lanes = build_agent_lanes(&diff, provider.clone(), &resolved);
    let cleaner_config = resolved.cleaner;

    // Create run_id + DB record BEFORE spawning so the UI can navigate immediately.
    let run_id = uuid::Uuid::new_v4().to_string();
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: run_id.clone(),
                pr_id: pr_id.clone(),
                status: "created".into(),
                started_at: Some(chrono::Utc::now().to_rfc3339()),
                completed_at: None,
                error_message: None,
            },
        )?;
    }

    // Register cancellation token
    let token = CancellationToken::new();
    {
        let mut map = active
            .0
            .lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        map.insert(run_id.clone(), token.clone());
    }

    // Spawn WITHOUT awaiting: return run_id immediately.
    let app_clone = app.clone();
    let run_id_clone = run_id.clone();

    tauri::async_runtime::spawn(async move {
        let db = app_clone.state::<AppDb>();
        let event_log = app_clone
            .try_state::<std::sync::Arc<crate::storage::event_log::EventLog>>()
            .map(|s| s.inner().clone());
        let sems = engine::build_provider_semaphores(&lanes);
        let result = engine::run_review_pipeline(
            &db,
            |event| {
                let _ = app_clone.emit("review_progress", event);
            },
            &sems,
            engine::ReviewPipelineArgs {
                run_id: run_id_clone.clone(),
                cwd: cwd_path,
                config: cleaner_config,
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log,
            },
        )
        .await;

        // Always cleanup the active token map
        let active = app_clone.state::<ActiveReviews>();
        if let Ok(mut map) = active.0.lock() {
            map.remove(&run_id_clone);
        }

        if let Err(e) = result {
            tracing::error!("Review run {} failed: {}", run_id_clone, e);
        }
    });

    Ok(run_id)
}

fn build_agent_lanes(
    diff: &str,
    provider: Arc<dyn ReviewProvider>,
    resolved: &config::ResolvedConfig,
) -> Vec<AgentLaneConfig> {
    let focuses: Vec<(String, AgentFocus)> = resolved
        .lanes
        .iter()
        .filter_map(|id| match id.as_str() {
            "security" => Some((id.clone(), AgentFocus::Security)),
            "architecture" => Some((id.clone(), AgentFocus::Architecture)),
            "performance" => Some((id.clone(), AgentFocus::Performance)),
            _ => None,
        })
        .collect();

    focuses
        .into_iter()
        .map(|(id, focus)| AgentLaneConfig {
            id,
            focus,
            provider: provider.clone(),
            input: prompts::build_review_input(focus, diff),
            timeout: resolved.lane_timeout,
        })
        .collect()
}

#[tauri::command]
pub async fn cancel_review(
    run_id: String,
    db: tauri::State<'_, AppDb>,
    active: tauri::State<'_, ActiveReviews>,
) -> Result<(), crate::errors::AppError> {
    use crate::errors::AppError;

    // Trigger cancellation for any in-flight review.
    if let Ok(mut map) = active.0.lock() {
        if let Some(token) = map.remove(&run_id) {
            token.cancel();
        }
    }

    // Also mark failed if still in progress.
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    let run = queries::get_review_run(&conn, &run_id)?
        .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;

    match run.status.as_str() {
        "created" | "running_agents" | "cleaning" | "submitting" => {
            queries::update_review_run_status(&conn, &run_id, "failed", Some("Cancelled by user"))?;
        }
        _ => {}
    }
    Ok(())
}

#[tauri::command]
pub async fn get_review_snapshot(
    run_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<ReviewSnapshot, crate::errors::AppError> {
    use crate::errors::AppError;

    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let run = queries::get_review_run(&conn, &run_id)?
        .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;

    let pr = queries::get_pull_request(&conn, &run.pr_id)?
        .ok_or_else(|| AppError::NotFound("PR not found".into()))?;

    let findings = queries::get_findings_for_run(&conn, &run_id)?;

    let changed_files: Vec<String> = pr
        .changed_files
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

    // Build lane statuses from agent_runs
    let agent_runs = queries::get_agent_runs_for_review(&conn, &run_id)?;
    let lane_statuses: Vec<LaneSnapshot> = agent_runs
        .iter()
        .map(|ar| LaneSnapshot {
            lane_id: ar.lane_id.clone(),
            status: ar.status.clone(),
            finding_count: ar.finding_count as usize,
            provider_name: ar.provider_name.clone(),
            error_message: ar.error_message.clone(),
        })
        .collect();

    let clusters = queries::get_clusters_for_run(&conn, &run_id)?;

    Ok(ReviewSnapshot {
        run_id: run.id,
        status: run.status,
        pr_title: pr.title,
        pr_number: pr.pr_number,
        pr_url: pr.url,
        diff_text: pr.diff_text,
        changed_files,
        findings,
        error_message: run.error_message,
        lane_statuses,
        clusters,
    })
}

#[tauri::command]
pub async fn get_incomplete_reviews(
    db: tauri::State<'_, AppDb>,
) -> Result<Vec<ReviewRun>, crate::errors::AppError> {
    use crate::errors::AppError;
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(queries::get_incomplete_review_runs(&conn)?)
}

#[tauri::command]
pub async fn resume_review(
    app: AppHandle,
    run_id: String,
    db: tauri::State<'_, AppDb>,
    active: tauri::State<'_, ActiveReviews>,
) -> Result<String, crate::errors::AppError> {
    use crate::errors::AppError;

    // If there's an in-flight pipeline for this run, cancel it before superseding.
    if let Ok(mut map) = active.0.lock() {
        if let Some(token) = map.remove(&run_id) {
            token.cancel();
        }
    }

    // Read the old run from DB
    let (old_status, pr_id) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let run = queries::get_review_run(&conn, &run_id)?
            .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;
        (run.status, run.pr_id)
    };

    // Terminal states: nothing to resume
    match old_status.as_str() {
        "ready" | "submitted" | "failed" => return Ok(run_id),
        _ => {}
    }

    // For incomplete states (created, running_agents, cleaning):
    // Mark the old run as failed and start a fresh review with the same PR data.
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::update_review_run_status(&conn, &run_id, "failed", Some("Superseded by resume"))?;
    }

    // Get PR data (same logic as start_review)
    let (diff, cwd) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let ws = conn.query_row(
            "SELECT local_path FROM workspaces WHERE id = ?1",
            rusqlite::params![pr.workspace_id],
            |row| row.get::<_, String>(0),
        )?;
        let diff = require_non_empty_diff(pr.diff_text)?;
        (diff, ws)
    };

    if cwd.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Workspace local path is not set. Confirm workspace before resuming review.".into(),
        ));
    }

    // Load repo-level config if available
    let repo_config = config::load_repo_config(&PathBuf::from(&cwd));

    // Resolve config and choose provider
    let resolved = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        config::resolve_config(&conn, repo_config.as_ref())
    };
    let provider: Arc<dyn ReviewProvider> =
        config::select_provider(&app, &resolved.preferred_provider).await;

    let cwd_path = PathBuf::from(&cwd);
    let lanes = build_agent_lanes(&diff, provider.clone(), &resolved);
    let cleaner_config = resolved.cleaner;

    // Create new run
    let new_run_id = uuid::Uuid::new_v4().to_string();
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: new_run_id.clone(),
                pr_id: pr_id.clone(),
                status: "created".into(),
                started_at: Some(chrono::Utc::now().to_rfc3339()),
                completed_at: None,
                error_message: None,
            },
        )?;
    }

    // Register cancellation token
    let token = CancellationToken::new();
    {
        let mut map = active
            .0
            .lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        map.insert(new_run_id.clone(), token.clone());
    }

    // Spawn the pipeline
    let app_clone = app.clone();
    let run_id_clone = new_run_id.clone();

    tauri::async_runtime::spawn(async move {
        let db = app_clone.state::<AppDb>();
        let event_log = app_clone
            .try_state::<std::sync::Arc<crate::storage::event_log::EventLog>>()
            .map(|s| s.inner().clone());
        let sems = engine::build_provider_semaphores(&lanes);
        let result = engine::run_review_pipeline(
            &db,
            |event| {
                let _ = app_clone.emit("review_progress", event);
            },
            &sems,
            engine::ReviewPipelineArgs {
                run_id: run_id_clone.clone(),
                cwd: cwd_path,
                config: cleaner_config,
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log,
            },
        )
        .await;

        // Cleanup the active token map
        let active = app_clone.state::<ActiveReviews>();
        if let Ok(mut map) = active.0.lock() {
            map.remove(&run_id_clone);
        }

        if let Err(e) = result {
            tracing::error!("Resumed review run {} failed: {}", run_id_clone, e);
        }
    });

    Ok(new_run_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_non_empty_diff_rejects_none() {
        assert!(require_non_empty_diff(None).is_err());
    }

    #[test]
    fn require_non_empty_diff_rejects_empty_string() {
        assert!(require_non_empty_diff(Some("   ".into())).is_err());
    }

    #[test]
    fn require_non_empty_diff_accepts_diff() {
        let diff = require_non_empty_diff(Some("diff --git a/a b/a".into())).unwrap();
        assert!(diff.contains("diff --git"));
    }
}
