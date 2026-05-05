use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use rusqlite::Connection;

use crate::agents::definition::AgentDefinition;
use crate::config;
use crate::context_pack::ContextPackBuilder;
use crate::local_checks::{self, LocalChecksRunner};
use crate::orchestration::engine;
use crate::orchestration::lane::{AgentLaneConfig, LaneSnapshot};
use crate::preferences::scoring;
use crate::providers::prompts::{self, AgentFocus};
use crate::providers::traits::{RawFinding, ReviewProvider};
use crate::review_delta::{self, ReviewDeltaCounts, ReviewDeltaSnapshot};
use crate::storage::db::AppDb;
use crate::storage::models::{Finding, FindingCluster, PullRequest, ReviewRun, ReviewerDecision};
use crate::storage::queries;

#[derive(Debug, Serialize)]
pub struct FindingSnapshot {
    #[serde(flatten)]
    pub finding: Finding,
    pub delta_state: Option<String>,
    pub baseline_finding_id: Option<String>,
    pub baseline_decision: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewSnapshot {
    pub run_id: String,
    pub status: String,
    pub pr_title: String,
    pub pr_number: i32,
    pub pr_url: String,
    pub diff_text: Option<String>,
    pub changed_files: Vec<String>,
    pub findings: Vec<FindingSnapshot>,
    pub error_message: Option<String>,
    pub lane_statuses: Vec<LaneSnapshot>,
    pub clusters: Vec<FindingCluster>,
    pub baseline_run_id: Option<String>,
    pub metrics: Option<crate::metrics::RunScorecard>,
    pub delta: Option<ReviewDeltaSnapshot>,
    pub decisions_by_finding_id: Option<HashMap<String, String>>,
    pub context_pack_summary: Option<serde_json::Value>,
    pub local_checks_summary: Option<serde_json::Value>,
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
        config::resolve_config(&conn, repo_config.as_ref(), Some(Path::new(&cwd)))
    };

    // Choose provider based on preference
    let provider: Arc<dyn ReviewProvider> =
        config::select_provider(&app, &resolved.preferred_provider).await;

    let cwd_path = PathBuf::from(&cwd);

    // Build multi-lane configs: Security, Architecture, Performance
    let lanes = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        build_agent_lanes(&diff, provider.clone(), &resolved, &conn)
    };
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
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
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

    // Build changed_files list for context pack
    let changed_files: Vec<String> = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        pr.changed_files
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    };

    let context_pack_config = resolved.context_pack;
    let local_checks_config = resolved.local_checks;

    // Spawn WITHOUT awaiting: return run_id immediately.
    let app_clone = app.clone();
    let run_id_clone = run_id.clone();

    tauri::async_runtime::spawn(async move {
        let db = app_clone.state::<AppDb>();
        let event_log = app_clone
            .try_state::<std::sync::Arc<crate::storage::event_log::EventLog>>()
            .map(|s| s.inner().clone());

        // Phase 3: Build context pack
        let preference_text = {
            let conn = db.0.lock().ok();
            conn.and_then(|c| {
                let decisions = queries::get_all_decisions(&c).ok()?;
                let summaries = scoring::compute_preference_summaries(&decisions);
                scoring::build_preference_prompt_section(&summaries)
            })
        };

        let context_pack = ContextPackBuilder::new(&context_pack_config, &cwd_path, &changed_files)
            .with_preferences(preference_text)
            .build();

        let context_suffix = if context_pack.prompt_suffix.is_empty() {
            None
        } else {
            Some(context_pack.prompt_suffix.clone())
        };

        // Persist context pack JSON
        if let Ok(json) = serde_json::to_string(&context_pack) {
            if let Ok(conn) = db.0.lock() {
                let _ = queries::update_review_run_context_pack(&conn, &run_id_clone, &json);
            }
        }

        // Phase 3: Run local checks if enabled
        let mut extra_raw_findings: Vec<RawFinding> = Vec::new();
        let local_checks_summary = {
            let runner = LocalChecksRunner::new(&cwd_path, &changed_files, token.clone())
                .with_config(Some(&local_checks_config));
            let summary = runner.run().await;
            if !summary.items.is_empty() {
                extra_raw_findings = local_checks::items_to_raw_findings(&summary.items);
            }
            summary
        };

        // Persist local checks JSON
        if let Ok(json) = serde_json::to_string(&local_checks_summary) {
            if let Ok(conn) = db.0.lock() {
                let _ = queries::update_review_run_local_checks(&conn, &run_id_clone, &json);
            }
        }

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
                context_suffix,
                extra_raw_findings,
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

pub(crate) fn build_agent_lanes(
    diff: &str,
    provider: Arc<dyn ReviewProvider>,
    resolved: &config::ResolvedConfig,
    conn: &Connection,
) -> Vec<AgentLaneConfig> {
    // Load reviewer preference decisions and build prompt section
    let preferences = queries::get_all_decisions(conn).ok().and_then(|decisions| {
        let summaries = scoring::compute_preference_summaries(&decisions);
        scoring::build_preference_prompt_section(&summaries)
    });

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

    let mut lanes: Vec<AgentLaneConfig> = focuses
        .into_iter()
        .map(|(id, focus)| {
            let input = prompts::build_review_input(focus.clone(), diff, preferences.as_deref());
            AgentLaneConfig {
                id,
                focus,
                provider: provider.clone(),
                input,
                timeout: resolved.lane_timeout,
            }
        })
        .collect();

    // Append custom agent lanes
    let custom_lanes = build_custom_agent_lanes(
        &resolved.custom_agents,
        diff,
        preferences.as_deref(),
        provider,
        resolved.lane_timeout,
    );
    lanes.extend(custom_lanes);

    lanes
}

/// Build lane configs for custom agents defined in repo/project configuration.
fn build_custom_agent_lanes(
    custom_agents: &[AgentDefinition],
    diff: &str,
    preferences: Option<&str>,
    provider: Arc<dyn ReviewProvider>,
    timeout: std::time::Duration,
) -> Vec<AgentLaneConfig> {
    custom_agents
        .iter()
        .map(|def| {
            let focus = AgentFocus::Custom(def.agent_type.clone());
            let input = prompts::build_review_input_with_custom_prompt(
                focus.clone(),
                diff,
                preferences,
                Some(&def.system_prompt),
            );
            AgentLaneConfig {
                id: def.name.clone(),
                focus,
                provider: provider.clone(),
                input,
                timeout,
            }
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

    let mut findings = queries::get_findings_for_run(&conn, &run_id)?;
    for finding in &mut findings {
        if finding.fingerprint.is_none() {
            finding.fingerprint = Some(review_delta::compute_finding_fingerprint(finding));
        }
    }

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

    // Phase 2: Deserialize cached metrics
    let metrics: Option<crate::metrics::RunScorecard> = run
        .metrics_json
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok());

    // Phase 2: Build decisions_by_finding_id (latest decision per finding)
    let decisions_by_finding_id =
        latest_decisions_by_finding(&queries::get_decisions_for_run(&conn, &run_id)?);

    let mut finding_snapshots: Vec<FindingSnapshot> = findings
        .into_iter()
        .map(|finding| FindingSnapshot {
            finding,
            delta_state: None,
            baseline_finding_id: None,
            baseline_decision: None,
        })
        .collect();

    // Phase 2: Compute delta for baseline-linked reruns.
    let delta = if let Some(baseline_run_id) = run.baseline_run_id.as_deref() {
        compute_delta_for_snapshot(&conn, baseline_run_id, &run, &pr, &mut finding_snapshots)?
    } else {
        None
    };

    Ok(ReviewSnapshot {
        run_id: run.id,
        status: run.status,
        pr_title: pr.title,
        pr_number: pr.pr_number,
        pr_url: pr.url,
        diff_text: pr.diff_text,
        changed_files,
        findings: finding_snapshots,
        error_message: run.error_message,
        lane_statuses,
        clusters,
        baseline_run_id: run.baseline_run_id,
        metrics,
        delta,
        decisions_by_finding_id,
        context_pack_summary: run
            .context_pack_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        local_checks_summary: run
            .local_checks_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
    })
}

fn compute_delta_for_snapshot(
    conn: &Connection,
    baseline_run_id: &str,
    current_run: &ReviewRun,
    current_pr: &PullRequest,
    finding_snapshots: &mut [FindingSnapshot],
) -> Result<Option<ReviewDeltaSnapshot>, crate::errors::AppError> {
    let Some(baseline_run) = queries::get_review_run(conn, baseline_run_id)? else {
        return Ok(None);
    };

    let baseline_pr = queries::get_pull_request(conn, &baseline_run.pr_id)?;
    let old_diff = baseline_run
        .analysis_diff_text
        .clone()
        .or_else(|| baseline_pr.and_then(|pr| pr.diff_text))
        .unwrap_or_default();
    let new_diff = current_run
        .analysis_diff_text
        .clone()
        .or_else(|| current_pr.diff_text.clone())
        .unwrap_or_default();

    let diff_summary = if old_diff.is_empty() && new_diff.is_empty() {
        review_delta::DeltaDiffSummary {
            changed_files: vec![],
            changed_hunks_by_file: HashMap::new(),
        }
    } else {
        review_delta::compute_changed_files_and_hunks(&old_diff, &new_diff)
    };

    let mut baseline_findings = queries::get_findings_for_run(conn, baseline_run_id)?;
    for finding in &mut baseline_findings {
        if finding.fingerprint.is_none() {
            finding.fingerprint = Some(review_delta::compute_finding_fingerprint(finding));
        }
    }
    for snapshot in finding_snapshots.iter_mut() {
        if snapshot.finding.fingerprint.is_none() {
            snapshot.finding.fingerprint =
                Some(review_delta::compute_finding_fingerprint(&snapshot.finding));
        }
    }

    let current_findings: Vec<Finding> = finding_snapshots
        .iter()
        .map(|snapshot| snapshot.finding.clone())
        .collect();
    let changed_files_set: HashSet<String> = diff_summary.changed_files.iter().cloned().collect();
    let classification =
        review_delta::classify_findings(&baseline_findings, &current_findings, &changed_files_set);

    let new_ids: HashSet<String> = classification.new_ids.iter().cloned().collect();
    let unchanged_ids: HashSet<String> = classification.unchanged_ids.iter().cloned().collect();
    let stale_ids: HashSet<String> = classification.stale_ids.iter().cloned().collect();

    let baseline_by_fingerprint: HashMap<String, &Finding> = baseline_findings
        .iter()
        .filter_map(|finding| {
            finding
                .fingerprint
                .as_ref()
                .map(|fingerprint| (fingerprint.clone(), finding))
        })
        .collect();
    let mut baseline_decision_by_finding_id: HashMap<String, String> = HashMap::new();
    for decision in queries::get_decisions_for_run(conn, baseline_run_id)? {
        baseline_decision_by_finding_id
            .entry(decision.finding_id)
            .or_insert(decision.decision);
    }

    for snapshot in finding_snapshots.iter_mut() {
        if new_ids.contains(&snapshot.finding.id) {
            snapshot.delta_state = Some("new".into());
        } else if stale_ids.contains(&snapshot.finding.id) {
            snapshot.delta_state = Some("stale".into());
        } else if unchanged_ids.contains(&snapshot.finding.id) {
            snapshot.delta_state = Some("unchanged".into());
        }

        if let Some(fingerprint) = snapshot.finding.fingerprint.as_ref() {
            if let Some(baseline) = baseline_by_fingerprint.get(fingerprint) {
                snapshot.baseline_finding_id = Some(baseline.id.clone());
                snapshot.baseline_decision =
                    baseline_decision_by_finding_id.get(&baseline.id).cloned();
            }
        }
    }

    Ok(Some(ReviewDeltaSnapshot {
        changed_files: diff_summary.changed_files,
        changed_hunks_by_file: diff_summary.changed_hunks_by_file,
        counts: ReviewDeltaCounts {
            new: classification.new_ids.len(),
            unchanged: classification.unchanged_ids.len(),
            stale: classification.stale_ids.len(),
            resolved: classification.resolved.len(),
        },
        resolved: classification.resolved.into_iter().take(50).collect(),
    }))
}

fn latest_decisions_by_finding(decisions: &[ReviewerDecision]) -> Option<HashMap<String, String>> {
    if decisions.is_empty() {
        return None;
    }

    let mut latest: HashMap<String, String> = HashMap::new();
    for decision in decisions {
        latest
            .entry(decision.finding_id.clone())
            .or_insert_with(|| decision.decision.clone());
    }
    Some(latest)
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
        config::resolve_config(&conn, repo_config.as_ref(), Some(Path::new(&cwd)))
    };
    let provider: Arc<dyn ReviewProvider> =
        config::select_provider(&app, &resolved.preferred_provider).await;

    let cwd_path = PathBuf::from(&cwd);
    let lanes = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        build_agent_lanes(&diff, provider.clone(), &resolved, &conn)
    };
    let cleaner_config = resolved.cleaner;
    let context_pack_config = resolved.context_pack;
    let local_checks_config = resolved.local_checks;

    // Build changed_files list
    let changed_files: Vec<String> = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let pr = queries::get_pull_request(&conn, &pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        pr.changed_files
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    };

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
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
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

        // Phase 3: Build context pack
        let preference_text = {
            let conn = db.0.lock().ok();
            conn.and_then(|c| {
                let decisions = queries::get_all_decisions(&c).ok()?;
                let summaries = scoring::compute_preference_summaries(&decisions);
                scoring::build_preference_prompt_section(&summaries)
            })
        };

        let context_pack = ContextPackBuilder::new(&context_pack_config, &cwd_path, &changed_files)
            .with_preferences(preference_text)
            .build();

        let context_suffix = if context_pack.prompt_suffix.is_empty() {
            None
        } else {
            Some(context_pack.prompt_suffix.clone())
        };

        if let Ok(json) = serde_json::to_string(&context_pack) {
            if let Ok(conn) = db.0.lock() {
                let _ = queries::update_review_run_context_pack(&conn, &run_id_clone, &json);
            }
        }

        // Phase 3: Run local checks if enabled
        let mut extra_raw_findings: Vec<RawFinding> = Vec::new();
        let local_checks_summary = {
            let runner = LocalChecksRunner::new(&cwd_path, &changed_files, token.clone())
                .with_config(Some(&local_checks_config));
            let summary = runner.run().await;
            if !summary.items.is_empty() {
                extra_raw_findings = local_checks::items_to_raw_findings(&summary.items);
            }
            summary
        };

        if let Ok(json) = serde_json::to_string(&local_checks_summary) {
            if let Ok(conn) = db.0.lock() {
                let _ = queries::update_review_run_local_checks(&conn, &run_id_clone, &json);
            }
        }

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
                context_suffix,
                extra_raw_findings,
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
    use crate::agents::definition::AgentDefinition;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::{Finding, PullRequest, ReviewRun, Workspace};
    use crate::storage::queries;

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

    #[test]
    fn build_custom_agent_lanes_empty_agents_returns_empty() {
        let provider = crate::providers::mock::mock_provider();
        let lanes = build_custom_agent_lanes(
            &[],
            "diff --git a/x b/x",
            None,
            provider,
            std::time::Duration::from_secs(60),
        );
        assert!(lanes.is_empty());
    }

    #[test]
    fn build_custom_agent_lanes_creates_lanes_from_definitions() {
        let provider = crate::providers::mock::mock_provider();
        let agents = vec![
            AgentDefinition {
                name: "a11y-checker".into(),
                system_prompt: "You review accessibility issues.".into(),
                agent_type: "accessibility".into(),
                severity_rules: None,
                provider: None,
            },
            AgentDefinition {
                name: "i18n-checker".into(),
                system_prompt: "You review internationalization.".into(),
                agent_type: "i18n".into(),
                severity_rules: None,
                provider: None,
            },
        ];
        let lanes = build_custom_agent_lanes(
            &agents,
            "diff --git a/x b/x",
            None,
            provider,
            std::time::Duration::from_secs(120),
        );

        assert_eq!(lanes.len(), 2);

        // First lane
        assert_eq!(lanes[0].id, "a11y-checker");
        assert_eq!(lanes[0].focus, AgentFocus::Custom("accessibility".into()));
        assert!(lanes[0]
            .input
            .system_prompt
            .contains("You review accessibility issues."));
        assert_eq!(lanes[0].timeout, std::time::Duration::from_secs(120));

        // Second lane
        assert_eq!(lanes[1].id, "i18n-checker");
        assert_eq!(lanes[1].focus, AgentFocus::Custom("i18n".into()));
        assert!(lanes[1]
            .input
            .system_prompt
            .contains("You review internationalization."));
    }

    #[test]
    fn build_custom_agent_lanes_includes_preferences() {
        let provider = crate::providers::mock::mock_provider();
        let agents = vec![AgentDefinition {
            name: "test-agent".into(),
            system_prompt: "Custom prompt.".into(),
            agent_type: "custom".into(),
            severity_rules: None,
            provider: None,
        }];
        let lanes = build_custom_agent_lanes(
            &agents,
            "diff",
            Some("Prefer short findings"),
            provider,
            std::time::Duration::from_secs(60),
        );

        assert_eq!(lanes.len(), 1);
        assert!(lanes[0].input.system_prompt.contains("Custom prompt."));
        assert!(lanes[0]
            .input
            .system_prompt
            .contains("Reviewer Preferences"));
        assert!(lanes[0]
            .input
            .system_prompt
            .contains("Prefer short findings"));
    }

    #[test]
    fn latest_decisions_prefers_first_entry_for_same_finding() {
        let decisions = vec![
            ReviewerDecision {
                id: "new".into(),
                finding_id: "finding-1".into(),
                review_run_id: "run-1".into(),
                decision: "accept".into(),
                original_severity: "warning".into(),
                original_agent_type: "security".into(),
                category_tag: None,
                time_to_decision_ms: None,
                decided_at: "2026-01-02T00:00:00Z".into(),
            },
            ReviewerDecision {
                id: "old".into(),
                finding_id: "finding-1".into(),
                review_run_id: "run-1".into(),
                decision: "skip".into(),
                original_severity: "warning".into(),
                original_agent_type: "security".into(),
                category_tag: None,
                time_to_decision_ms: None,
                decided_at: "2026-01-01T00:00:00Z".into(),
            },
        ];

        let latest = latest_decisions_by_finding(&decisions).expect("map should exist");
        assert_eq!(latest.get("finding-1").map(String::as_str), Some("accept"));
    }

    #[test]
    fn compute_delta_for_snapshot_sets_delta_state_and_baseline_decision() {
        let db = init_db_in_memory().expect("in-memory DB");
        let conn = db.0.into_inner().expect("owned connection");

        let workspace = Workspace {
            id: "ws-1".into(),
            local_path: "/tmp/repo".into(),
            remote_owner: "o".into(),
            remote_repo: "r".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
        };
        queries::insert_workspace(&conn, &workspace).unwrap();

        let baseline_diff = "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n";

        let baseline_pr = PullRequest {
            id: "pr-base".into(),
            workspace_id: workspace.id.clone(),
            pr_number: 1,
            title: "Base".into(),
            author: None,
            base_branch: None,
            head_branch: None,
            url: "https://github.com/o/r/pull/1".into(),
            diff_text: Some(baseline_diff.into()),
            changed_files: Some("[\"src/lib.rs\"]".into()),
            fetched_at: "2026-01-01T00:00:00Z".into(),
            diff_hash: None,
        };
        queries::insert_pull_request(&conn, &baseline_pr).unwrap();

        let current_pr = PullRequest {
            id: "pr-current".into(),
            workspace_id: workspace.id.clone(),
            pr_number: 1,
            title: "Current".into(),
            author: None,
            base_branch: None,
            head_branch: None,
            url: "https://github.com/o/r/pull/1".into(),
            diff_text: Some(baseline_diff.into()),
            changed_files: Some("[\"src/lib.rs\"]".into()),
            fetched_at: "2026-01-02T00:00:00Z".into(),
            diff_hash: None,
        };
        queries::insert_pull_request(&conn, &current_pr).unwrap();

        let baseline_run = ReviewRun {
            id: "run-base".into(),
            pr_id: baseline_pr.id.clone(),
            status: "ready".into(),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            completed_at: Some("2026-01-01T00:01:00Z".into()),
            error_message: None,
            baseline_run_id: None,
            metrics_json: None,
            analysis_diff_hash: None,
            analysis_diff_text: Some(baseline_diff.into()),
            context_pack_json: None,
            local_checks_json: None,
        };
        queries::insert_review_run(&conn, &baseline_run).unwrap();

        let current_run = ReviewRun {
            id: "run-current".into(),
            pr_id: current_pr.id.clone(),
            status: "ready".into(),
            started_at: Some("2026-01-02T00:00:00Z".into()),
            completed_at: Some("2026-01-02T00:01:00Z".into()),
            error_message: None,
            baseline_run_id: Some(baseline_run.id.clone()),
            metrics_json: None,
            analysis_diff_hash: None,
            analysis_diff_text: Some(baseline_diff.into()),
            context_pack_json: None,
            local_checks_json: None,
        };
        queries::insert_review_run(&conn, &current_run).unwrap();

        let mut baseline_finding = Finding {
            id: "base-f1".into(),
            review_run_id: baseline_run.id.clone(),
            agent_type: "security".into(),
            file_path: Some("src/lib.rs".into()),
            line_start: Some(10),
            line_end: Some(12),
            severity: "warning".into(),
            confidence: 0.8,
            title: "Shared issue".into(),
            body: "Shared body".into(),
            evidence: None,
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: true,
            created_at: "2026-01-01T00:00:00Z".into(),
            cluster_id: None,
            lane_id: Some("security".into()),
            provider_name: Some("codex".into()),
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
        baseline_finding.fingerprint =
            Some(review_delta::compute_finding_fingerprint(&baseline_finding));
        queries::insert_finding(&conn, &baseline_finding).unwrap();

        queries::insert_decision(
            &conn,
            &ReviewerDecision {
                id: "dec-base".into(),
                finding_id: baseline_finding.id.clone(),
                review_run_id: baseline_run.id.clone(),
                decision: "accept".into(),
                original_severity: baseline_finding.severity.clone(),
                original_agent_type: baseline_finding.agent_type.clone(),
                category_tag: None,
                time_to_decision_ms: None,
                decided_at: "2026-01-01T00:00:30Z".into(),
            },
        )
        .unwrap();

        let current_findings = vec![
            Finding {
                id: "cur-f1".into(),
                review_run_id: current_run.id.clone(),
                agent_type: "security".into(),
                file_path: Some("src/lib.rs".into()),
                line_start: Some(20),
                line_end: Some(22),
                severity: "warning".into(),
                confidence: 0.8,
                title: "Shared issue".into(),
                body: "Shared body".into(),
                evidence: None,
                status: "active".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: true,
                created_at: "2026-01-02T00:00:00Z".into(),
                cluster_id: None,
                lane_id: Some("security".into()),
                provider_name: Some("codex".into()),
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
            Finding {
                id: "cur-f2".into(),
                review_run_id: current_run.id.clone(),
                agent_type: "security".into(),
                file_path: Some("src/lib.rs".into()),
                line_start: Some(30),
                line_end: Some(31),
                severity: "warning".into(),
                confidence: 0.7,
                title: "New issue".into(),
                body: "New body".into(),
                evidence: None,
                status: "active".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: true,
                created_at: "2026-01-02T00:00:00Z".into(),
                cluster_id: None,
                lane_id: Some("security".into()),
                provider_name: Some("codex".into()),
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
        ];

        let mut snapshots: Vec<FindingSnapshot> = current_findings
            .into_iter()
            .map(|finding| FindingSnapshot {
                finding,
                delta_state: None,
                baseline_finding_id: None,
                baseline_decision: None,
            })
            .collect();

        let delta = compute_delta_for_snapshot(
            &conn,
            &baseline_run.id,
            &current_run,
            &current_pr,
            &mut snapshots,
        )
        .expect("delta computation should succeed")
        .expect("delta payload should be present");

        assert_eq!(delta.counts.new, 1);
        assert_eq!(delta.counts.unchanged, 1);
        assert_eq!(delta.counts.stale, 0);
        assert_eq!(delta.counts.resolved, 0);

        let unchanged = snapshots
            .iter()
            .find(|snapshot| snapshot.finding.id == "cur-f1")
            .expect("unchanged finding");
        assert_eq!(unchanged.delta_state.as_deref(), Some("unchanged"));
        assert_eq!(unchanged.baseline_finding_id.as_deref(), Some("base-f1"));
        assert_eq!(unchanged.baseline_decision.as_deref(), Some("accept"));

        let new_finding = snapshots
            .iter()
            .find(|snapshot| snapshot.finding.id == "cur-f2")
            .expect("new finding");
        assert_eq!(new_finding.delta_state.as_deref(), Some("new"));
        assert!(new_finding.baseline_finding_id.is_none());
        assert!(new_finding.baseline_decision.is_none());
    }
}
