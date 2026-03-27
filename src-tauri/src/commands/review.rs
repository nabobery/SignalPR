use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::cleaner::CleanerConfig;
use crate::orchestration::engine;
use crate::providers::codex::{CodexProvider, MockProvider};
use crate::providers::traits::ReviewProvider;
use crate::storage::db::AppDb;
use crate::storage::models::{Finding, ReviewRun};
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
}

pub struct ActiveReviews(pub Mutex<HashMap<String, CancellationToken>>);

#[tauri::command]
pub async fn start_review(
    app: AppHandle,
    pr_id: String,
    db: tauri::State<'_, AppDb>,
    active: tauri::State<'_, ActiveReviews>,
) -> Result<String, String> {
    // Get PR data
    let (diff, cwd) = {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
        let pr = queries::get_pull_request(&conn, &pr_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "PR not found".to_string())?;
        let ws = conn
            .query_row(
                "SELECT local_path FROM workspaces WHERE id = ?1",
                rusqlite::params![pr.workspace_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|e| e.to_string())?;
        let diff = pr.diff_text.unwrap_or_default();
        (diff, ws)
    };

    if cwd.trim().is_empty() {
        return Err(
            "Workspace local path is not set. Confirm workspace before starting review."
                .to_string(),
        );
    }

    // Choose provider: codex if available, mock otherwise
    let provider: Arc<dyn ReviewProvider> = {
        let codex = CodexProvider::new(app.clone());
        let health = codex.health_check().await;
        if health.available {
            Arc::new(codex)
        } else {
            tracing::info!("Codex not available, using mock provider");
            Arc::new(MockProvider::with_default_fixture())
        }
    };

    let config = CleanerConfig::default();
    let cwd_path = PathBuf::from(&cwd);

    // Create run_id + DB record BEFORE spawning so the UI can navigate immediately.
    let run_id = uuid::Uuid::new_v4().to_string();
    {
        let conn = db.0.lock().map_err(|e| e.to_string())?;
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
        )
        .map_err(|e| e.to_string())?;
    }

    // Register cancellation token
    let token = CancellationToken::new();
    {
        let mut map = active.0.lock().map_err(|e| e.to_string())?;
        map.insert(run_id.clone(), token.clone());
    }

    // Spawn WITHOUT awaiting: return run_id immediately.
    let app_clone = app.clone();
    let run_id_clone = run_id.clone();
    let diff_clone = diff.clone();
    let cwd_path_clone = cwd_path.clone();
    let config_clone = config.clone();
    let provider_clone = provider.clone();
    let token_clone = token.clone();

    tauri::async_runtime::spawn(async move {
        let db = app_clone.state::<AppDb>();
        let result = engine::run_review_pipeline(
            &db,
            provider_clone,
            |event| {
                let _ = app_clone.emit("review_progress", event);
            },
            engine::ReviewPipelineArgs {
                run_id: &run_id_clone,
                diff: &diff_clone,
                cwd: &cwd_path_clone,
                config: &config_clone,
                cancel: token_clone,
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

#[tauri::command]
pub async fn cancel_review(
    run_id: String,
    db: tauri::State<'_, AppDb>,
    active: tauri::State<'_, ActiveReviews>,
) -> Result<(), String> {
    // Trigger cancellation for any in-flight review.
    if let Ok(mut map) = active.0.lock() {
        if let Some(token) = map.remove(&run_id) {
            token.cancel();
        }
    }

    // Also mark failed if still in progress.
    let conn = db.0.lock().map_err(|e| e.to_string())?;
    let run = queries::get_review_run(&conn, &run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Review run not found".to_string())?;

    match run.status.as_str() {
        "created" | "running_agents" | "cleaning" | "submitting" => {
            queries::update_review_run_status(&conn, &run_id, "failed", Some("Cancelled by user"))
                .map_err(|e| e.to_string())?;
        }
        _ => {}
    }
    Ok(())
}

#[tauri::command]
pub async fn get_review_snapshot(
    run_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<ReviewSnapshot, String> {
    let conn = db.0.lock().map_err(|e| e.to_string())?;

    let run = queries::get_review_run(&conn, &run_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Review run not found".to_string())?;

    let pr = queries::get_pull_request(&conn, &run.pr_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "PR not found".to_string())?;

    let findings = queries::get_findings_for_run(&conn, &run_id).map_err(|e| e.to_string())?;

    let changed_files: Vec<String> = pr
        .changed_files
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();

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
    })
}
