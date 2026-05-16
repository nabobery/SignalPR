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
use crate::integrations::cache::{
    build_cache_key, get_issue_cache, put_issue_cache, CacheStatus, CachedIssue,
};
use crate::issues::extract::extract_all_external_issues;
use crate::issues::types::IssueExtractionConfig;
use crate::local_checks::{self, LocalChecksRunner};
use crate::orchestration::engine;
use crate::orchestration::lane::{AgentLaneConfig, LaneSnapshot};
use crate::preferences::scoring;
use crate::providers::control_plane::{
    build_provider_control_plane_snapshot, load_provider_control_inputs,
    ProviderControlPlaneSnapshot, ProviderSelectionTrace,
};
use crate::providers::github::{
    resolve_api_from_app, GitHubApiError, MAX_ISSUES, MAX_ISSUE_BODY_EXCERPT_BYTES,
    MAX_ISSUE_CONTEXT_BYTES_TOTAL,
};
use crate::providers::prompts::{self, AgentFocus};
use crate::providers::traits::{RawFinding, ReviewProvider};
use crate::review_delta::{self, ReviewDeltaCounts, ReviewDeltaSnapshot};
use crate::secrets::integrations::{self as integration_secrets, IntegrationSecretField};
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
    pub pr_id: String,
    pub workspace_id: String,
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
    pub review_freshness: ReviewFreshnessSummary,
    pub decisions_by_finding_id: Option<HashMap<String, String>>,
    pub context_pack_summary: Option<serde_json::Value>,
    pub local_checks_summary: Option<serde_json::Value>,
    pub platform_metadata: Option<serde_json::Value>,
    pub platform_metadata_fetched_at: Option<String>,
    pub platform_capabilities: Option<serde_json::Value>,
    pub platform_capabilities_fetched_at: Option<String>,
    pub provider_selection: Option<ProviderSelectionTrace>,
    pub provider_control: Option<ProviderControlPlaneSnapshot>,
}

#[derive(Debug, Serialize)]
pub struct ReviewFreshnessSummary {
    pub is_rerun: bool,
    pub baseline_run_id: Option<String>,
    pub reviewed_head_sha: Option<String>,
    pub current_head_sha: Option<String>,
    pub head_changed_since_review: bool,
    pub rerun_trigger_source: Option<String>,
    pub rerun_reason: Option<String>,
    pub rerun_scope: Option<String>,
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

fn extract_head_sha_from_pr(pr: &Option<PullRequest>) -> Option<String> {
    let pr = pr.as_ref()?;
    let json = pr.platform_metadata_json.as_deref()?;
    if let Ok(meta) = serde_json::from_str::<crate::platform::adapter::PlatformMetadata>(json) {
        return match meta {
            crate::platform::adapter::PlatformMetadata::GitHub(g) => Some(g.head_sha),
            crate::platform::adapter::PlatformMetadata::GitLab(g) => Some(g.head_sha),
            crate::platform::adapter::PlatformMetadata::Bitbucket(b) => Some(b.head_sha),
        };
    }
    serde_json::from_str::<crate::providers::github::PlatformMetadataSnapshot>(json)
        .ok()
        .map(|meta| meta.head_sha)
}

fn build_review_freshness_summary(
    conn: &Connection,
    run: &ReviewRun,
    pr: &PullRequest,
) -> Result<ReviewFreshnessSummary, rusqlite::Error> {
    let baseline_run = run
        .baseline_run_id
        .as_deref()
        .map(|baseline_run_id| queries::get_review_run(conn, baseline_run_id))
        .transpose()?
        .flatten();
    let submission = queries::get_submission_for_run(conn, &run.id)?;
    let reviewed_head_sha = if run.baseline_run_id.is_some() {
        baseline_run
            .as_ref()
            .and_then(|baseline| baseline.head_sha_at_run.clone())
            .or_else(|| run.head_sha_at_run.clone())
    } else if run.status == "submitted" {
        submission
            .as_ref()
            .and_then(|record| record.commit_id_at_submission.clone())
            .or_else(|| run.head_sha_at_run.clone())
    } else {
        run.head_sha_at_run.clone().or_else(|| {
            submission
                .as_ref()
                .and_then(|record| record.commit_id_at_submission.clone())
        })
    };
    let current_head_sha =
        extract_head_sha_from_pr(&Some(pr.clone())).or_else(|| run.head_sha_at_run.clone());
    let head_changed_since_review = reviewed_head_sha
        .as_deref()
        .zip(current_head_sha.as_deref())
        .is_some_and(|(reviewed, current)| reviewed != current);

    Ok(ReviewFreshnessSummary {
        is_rerun: run.baseline_run_id.is_some(),
        baseline_run_id: run.baseline_run_id.clone(),
        reviewed_head_sha,
        current_head_sha,
        head_changed_since_review,
        rerun_trigger_source: run.rerun_trigger_source.clone(),
        rerun_reason: run.rerun_reason.clone(),
        rerun_scope: run.rerun_scope.clone(),
    })
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

    // Choose the provider from the resolved preference and persist the selection trace.
    let selected_provider =
        config::select_provider_with_trace(&app, &resolved.preferred_provider).await;
    let provider: Arc<dyn ReviewProvider> = selected_provider.provider.clone();
    let provider_selection_json = serde_json::to_string(&selected_provider.trace)
        .map_err(|e| AppError::InvalidInput(e.to_string()))?;

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
                head_sha_at_run: extract_head_sha_from_pr(&queries::get_pull_request(
                    &conn, &pr_id,
                )?),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: Some(provider_selection_json),
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
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

        // Build context pack
        let preference_text = {
            let conn = db.0.lock().ok();
            conn.and_then(|c| {
                let decisions = queries::get_all_decisions(&c).ok()?;
                let summaries = scoring::compute_preference_summaries(&decisions);
                scoring::build_preference_prompt_section(&summaries)
            })
        };

        // Resolve issue refs and base-branch CODEOWNERS through the platform adapter.
        let (issue_refs, base_branch_codeowners, codeowners_source) =
            resolve_issue_refs_and_codeowners(&app_clone, &db, &pr_id).await;

        let context_pack = ContextPackBuilder::new(&context_pack_config, &cwd_path, &changed_files)
            .with_preferences(preference_text)
            .with_codeowners_content(base_branch_codeowners.clone(), codeowners_source)
            .with_issues(issue_refs)
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

        // Run local checks if enabled
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

        // Resolve CODEOWNERS from base branch (fallback to local workspace)
        let owners_by_path = {
            let raw = base_branch_codeowners
                .or_else(|| crate::context_pack::read_local_codeowners(&cwd_path));
            raw.map(|content| {
                crate::context_pack::resolve_codeowners(&content, &changed_files)
                    .into_iter()
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default()
        };

        let (issue_context_included_count, issue_context_sources) = {
            let issue_items: Vec<_> = context_pack
                .items
                .iter()
                .filter(|i| i.kind == "issue" && i.included)
                .collect();
            let count = issue_items.len();
            let sources: Vec<String> = issue_items.iter().map(|i| i.source.clone()).collect();
            (count, sources)
        };

        if let Some(ref el) = event_log {
            let omitted: Vec<_> = context_pack
                .items
                .iter()
                .filter(|i| i.kind == "issue" && !i.included)
                .map(|i| {
                    serde_json::json!({
                        "label": i.label,
                        "source": i.source,
                        "omit_reason": i.omit_reason,
                    })
                })
                .collect();
            let _ = el.append(
                &run_id_clone,
                "issue_context_summary",
                serde_json::json!({
                    "included_count": issue_context_included_count,
                    "sources": &issue_context_sources,
                    "omitted_count": omitted.len(),
                    "omitted": omitted,
                }),
            );
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
                owners_by_path,
                issue_context_included_count,
                issue_context_sources,
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
    app: AppHandle,
    db: tauri::State<'_, AppDb>,
) -> Result<ReviewSnapshot, crate::errors::AppError> {
    use crate::errors::AppError;

    let (
        run,
        pr,
        provider_selection,
        changed_files,
        lane_statuses,
        clusters,
        metrics,
        decisions_by_finding_id,
        review_freshness,
        provider_control_inputs,
        finding_snapshots,
        delta,
    ) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;

        let run = queries::get_review_run(&conn, &run_id)?
            .ok_or_else(|| AppError::NotFound("Review run not found".into()))?;

        let pr = queries::get_pull_request(&conn, &run.pr_id)?
            .ok_or_else(|| AppError::NotFound("PR not found".into()))?;
        let provider_selection = run
            .provider_selection_json
            .as_deref()
            .and_then(|s| serde_json::from_str::<ProviderSelectionTrace>(s).ok());

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
        let metrics: Option<crate::metrics::RunScorecard> = run
            .metrics_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok());
        let decisions_by_finding_id =
            latest_decisions_by_finding(&queries::get_decisions_for_run(&conn, &run_id)?);
        let review_freshness = build_review_freshness_summary(&conn, &run, &pr)?;
        let provider_control_inputs =
            load_provider_control_inputs(&conn, Some(&pr.workspace_id)).ok();

        let mut finding_snapshots: Vec<FindingSnapshot> = findings
            .into_iter()
            .map(|finding| FindingSnapshot {
                finding,
                delta_state: None,
                baseline_finding_id: None,
                baseline_decision: None,
            })
            .collect();

        let delta = if let Some(baseline_run_id) = run.baseline_run_id.as_deref() {
            compute_delta_for_snapshot(&conn, baseline_run_id, &run, &pr, &mut finding_snapshots)?
        } else {
            None
        };

        (
            run,
            pr,
            provider_selection,
            changed_files,
            lane_statuses,
            clusters,
            metrics,
            decisions_by_finding_id,
            review_freshness,
            provider_control_inputs,
            finding_snapshots,
            delta,
        )
    };

    let provider_control = if let Some((preferred_provider, recent_runs)) = provider_control_inputs
    {
        build_provider_control_plane_snapshot(
            &app,
            preferred_provider,
            recent_runs,
            Some(pr.workspace_id.clone()),
        )
        .await
        .ok()
    } else {
        None
    };

    Ok(ReviewSnapshot {
        run_id: run.id,
        pr_id: run.pr_id,
        workspace_id: pr.workspace_id.clone(),
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
        review_freshness,
        decisions_by_finding_id,
        context_pack_summary: run
            .context_pack_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        local_checks_summary: run
            .local_checks_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        platform_metadata: pr
            .platform_metadata_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        platform_metadata_fetched_at: pr.platform_metadata_fetched_at,
        platform_capabilities: pr
            .platform_capabilities_json
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok()),
        platform_capabilities_fetched_at: pr.platform_capabilities_fetched_at,
        provider_selection,
        provider_control,
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

fn build_issue_refs_from_metadata(
    metadata: Option<&crate::providers::github::PlatformMetadataSnapshot>,
    default_owner: &str,
    default_repo: &str,
) -> Vec<crate::context_pack::IssueRef> {
    let Some(meta) = metadata else {
        return Vec::new();
    };

    let mut dedup = std::collections::HashSet::new();
    let mut refs = Vec::new();

    for num in &meta.linked_issue_numbers {
        let number = num.to_string();
        let key = format!("{}/{}#{}", default_owner, default_repo, number);
        if dedup.insert(key) {
            refs.push(crate::context_pack::IssueRef {
                number,
                title: format!("Linked issue #{}", num),
                body_excerpt: String::new(),
                owner: Some(default_owner.to_string()),
                repo: Some(default_repo.to_string()),
                labels: Vec::new(),
                state: None,
                omit_reason: None,
                tracker: Some("github".to_string()),
                confidence: Some("high".to_string()),
                url: None,
                origin: Some("platform_link".to_string()),
            });
        }
    }

    for text_ref in &meta.text_issue_refs {
        if let Some((owner, repo, number)) = parse_issue_ref(text_ref, default_owner, default_repo)
        {
            let key = format!("{}/{}#{}", owner, repo, number);
            if dedup.insert(key) {
                refs.push(crate::context_pack::IssueRef {
                    number: number.clone(),
                    title: format!("Referenced issue #{}", number),
                    body_excerpt: String::new(),
                    owner: Some(owner),
                    repo: Some(repo),
                    labels: Vec::new(),
                    state: None,
                    omit_reason: None,
                    tracker: Some("github".to_string()),
                    confidence: Some("medium".to_string()),
                    url: None,
                    origin: Some("text_ref".to_string()),
                });
            }
        }
    }

    refs
}

/// Build issue refs from platform metadata.
fn build_issue_refs_from_platform_metadata(
    metadata: Option<&crate::platform::adapter::PlatformMetadata>,
    default_owner: &str,
    default_repo: &str,
) -> Vec<crate::context_pack::IssueRef> {
    let Some(meta) = metadata else {
        return Vec::new();
    };

    let mut dedup = std::collections::HashSet::new();
    let mut refs = Vec::new();

    match meta {
        crate::platform::adapter::PlatformMetadata::Bitbucket(b) => {
            for key in &b.jira_issue_keys {
                if dedup.insert(key.clone()) {
                    refs.push(crate::context_pack::IssueRef {
                        number: key.clone(),
                        title: format!("Jira issue {}", key),
                        body_excerpt: String::new(),
                        owner: None,
                        repo: None,
                        labels: Vec::new(),
                        state: None,
                        omit_reason: None,
                        tracker: Some("jira".to_string()),
                        confidence: Some("high".to_string()),
                        url: None,
                        origin: Some("platform_link".to_string()),
                    });
                }
            }
        }
        _ => {
            let (linked_ids, tracker_name): (&[i64], &str) = match meta {
                crate::platform::adapter::PlatformMetadata::GitHub(g) => {
                    (&g.linked_issue_numbers, "github")
                }
                crate::platform::adapter::PlatformMetadata::GitLab(g) => {
                    (&g.closes_issues, "gitlab")
                }
                crate::platform::adapter::PlatformMetadata::Bitbucket(_) => unreachable!(),
            };

            for num in linked_ids {
                let number = num.to_string();
                let key = format!("{}/{}#{}", default_owner, default_repo, number);
                if dedup.insert(key) {
                    refs.push(crate::context_pack::IssueRef {
                        number,
                        title: format!("Linked issue #{}", num),
                        body_excerpt: String::new(),
                        owner: Some(default_owner.to_string()),
                        repo: Some(default_repo.to_string()),
                        labels: Vec::new(),
                        state: None,
                        omit_reason: None,
                        tracker: Some(tracker_name.to_string()),
                        confidence: Some("high".to_string()),
                        url: None,
                        origin: Some("platform_link".to_string()),
                    });
                }
            }

            if let crate::platform::adapter::PlatformMetadata::GitHub(g) = meta {
                for text_ref in &g.text_issue_refs {
                    if let Some((owner, repo, number)) =
                        parse_issue_ref(text_ref, default_owner, default_repo)
                    {
                        let key = format!("{}/{}#{}", owner, repo, number);
                        if dedup.insert(key) {
                            refs.push(crate::context_pack::IssueRef {
                                number: number.clone(),
                                title: format!("Referenced issue #{}", number),
                                body_excerpt: String::new(),
                                owner: Some(owner),
                                repo: Some(repo),
                                labels: Vec::new(),
                                state: None,
                                omit_reason: None,
                                tracker: Some("github".to_string()),
                                confidence: Some("medium".to_string()),
                                url: None,
                                origin: Some("text_ref".to_string()),
                            });
                        }
                    }
                }
            }
        }
    }

    refs
}

async fn hydrate_issue_refs_from_github(
    app: &AppHandle,
    refs: Vec<crate::context_pack::IssueRef>,
) -> Vec<crate::context_pack::IssueRef> {
    if refs.is_empty() {
        return refs;
    }

    let (api, _) = match resolve_api_from_app(app).await {
        Ok(api) => api,
        Err(_) => {
            return refs
                .into_iter()
                .map(|mut issue_ref| {
                    issue_ref.omit_reason = Some("fetch_failed".to_string());
                    issue_ref
                })
                .collect();
        }
    };

    let mut hydrated = Vec::new();
    let mut total_issue_bytes = 0usize;

    for (index, issue_ref) in refs.into_iter().enumerate() {
        if index >= MAX_ISSUES {
            hydrated.push(crate::context_pack::IssueRef {
                omit_reason: Some("budget_exceeded".to_string()),
                ..issue_ref
            });
            continue;
        }

        let owner = issue_ref.owner.clone().unwrap_or_default();
        let repo = issue_ref.repo.clone().unwrap_or_default();
        let issue_number = issue_ref.number.parse::<i64>().ok();
        if owner.is_empty() || repo.is_empty() || issue_number.is_none() {
            hydrated.push(crate::context_pack::IssueRef {
                omit_reason: Some("fetch_failed".to_string()),
                ..issue_ref
            });
            continue;
        }
        let issue_number = issue_number.unwrap_or_default();

        match api.get_issue(&owner, &repo, issue_number).await {
            Ok(issue) => {
                let body_excerpt = truncate_to_bytes(
                    issue.body.as_deref().unwrap_or_default(),
                    MAX_ISSUE_BODY_EXCERPT_BYTES,
                );
                let labels: Vec<String> = issue
                    .labels
                    .unwrap_or_default()
                    .into_iter()
                    .map(|label| label.name)
                    .collect();
                let estimated_bytes = issue.title.len()
                    + body_excerpt.len()
                    + labels.iter().map(String::len).sum::<usize>()
                    + 64;
                if total_issue_bytes + estimated_bytes > MAX_ISSUE_CONTEXT_BYTES_TOTAL {
                    hydrated.push(crate::context_pack::IssueRef {
                        omit_reason: Some("budget_exceeded".to_string()),
                        ..issue_ref
                    });
                    continue;
                }
                total_issue_bytes += estimated_bytes;
                hydrated.push(crate::context_pack::IssueRef {
                    title: issue.title,
                    body_excerpt,
                    labels,
                    state: Some(issue.state),
                    omit_reason: None,
                    ..issue_ref
                });
            }
            Err(err) => {
                hydrated.push(crate::context_pack::IssueRef {
                    omit_reason: Some(issue_fetch_omit_reason(&err).to_string()),
                    ..issue_ref
                });
            }
        }
    }

    hydrated
}

async fn fetch_base_branch_codeowners(
    app: &AppHandle,
    owner: &str,
    repo: &str,
    base_ref: &str,
) -> Option<String> {
    let (api, _) = resolve_api_from_app(app).await.ok()?;
    match api.get_codeowners(owner, repo, base_ref).await {
        Ok(content) => content,
        Err(err) => {
            tracing::warn!("Failed to fetch base-branch CODEOWNERS: {}", err);
            None
        }
    }
}

/// Platform-aware CODEOWNERS fetch using the adapter trait.
async fn fetch_codeowners_via_adapter(
    adapter: &dyn crate::platform::adapter::PlatformAdapter,
    base_ref: &str,
    platform_name: &str,
) -> Option<String> {
    let locations = match platform_name {
        "gitlab" => crate::context_pack::CODEOWNERS_LOCATIONS_GITLAB,
        _ => crate::context_pack::CODEOWNERS_LOCATIONS_GITHUB,
    };
    for path in locations {
        match adapter.fetch_file_content(path, base_ref).await {
            Ok(Some(content)) => return Some(content),
            Ok(None) => continue,
            Err(err) => {
                tracing::warn!("Failed to fetch {} from base branch: {}", path, err);
                continue;
            }
        }
    }
    None
}

async fn hydrate_issue_refs_via_adapter(
    adapter: &dyn crate::platform::adapter::PlatformAdapter,
    refs: Vec<crate::context_pack::IssueRef>,
) -> Vec<crate::context_pack::IssueRef> {
    if refs.is_empty() {
        return refs;
    }

    let issue_ids: Vec<i64> = refs
        .iter()
        .filter_map(|r| r.number.parse::<i64>().ok())
        .collect();
    let contexts = match adapter.fetch_issue_context(&issue_ids, MAX_ISSUES).await {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(
                "Failed to fetch issue context via platform adapter: {}",
                err
            );
            return refs
                .into_iter()
                .map(|mut issue_ref| {
                    issue_ref.omit_reason = Some("fetch_failed".to_string());
                    issue_ref
                })
                .collect();
        }
    };

    let mut by_number: HashMap<String, crate::platform::adapter::IssueContext> = contexts
        .into_iter()
        .map(|c| (c.number.to_string(), c))
        .collect();
    let mut hydrated = Vec::new();
    let mut total_issue_bytes = 0usize;

    for (index, issue_ref) in refs.into_iter().enumerate() {
        if index >= MAX_ISSUES {
            hydrated.push(crate::context_pack::IssueRef {
                omit_reason: Some("budget_exceeded".to_string()),
                ..issue_ref
            });
            continue;
        }

        if let Some(context) = by_number.remove(&issue_ref.number) {
            let body_excerpt = context
                .body_excerpt
                .as_deref()
                .map(|b| truncate_to_bytes(b, MAX_ISSUE_BODY_EXCERPT_BYTES))
                .unwrap_or_default();
            let estimated_bytes = context.title.len()
                + body_excerpt.len()
                + context.labels.iter().map(String::len).sum::<usize>()
                + 64;
            if total_issue_bytes + estimated_bytes > MAX_ISSUE_CONTEXT_BYTES_TOTAL {
                hydrated.push(crate::context_pack::IssueRef {
                    omit_reason: Some("budget_exceeded".to_string()),
                    ..issue_ref
                });
                continue;
            }
            total_issue_bytes += estimated_bytes;
            hydrated.push(crate::context_pack::IssueRef {
                title: context.title,
                body_excerpt,
                labels: context.labels,
                state: context.state,
                omit_reason: None,
                ..issue_ref
            });
        } else {
            hydrated.push(crate::context_pack::IssueRef {
                omit_reason: Some("fetch_failed".to_string()),
                ..issue_ref
            });
        }
    }

    hydrated
}

fn hydrate_issue_ref_from_cached(
    mut issue_ref: crate::context_pack::IssueRef,
    status: CacheStatus,
    cached: Option<CachedIssue>,
) -> crate::context_pack::IssueRef {
    match status {
        CacheStatus::Ok => {
            if let Some(cached) = cached {
                issue_ref.title = cached.title;
                issue_ref.body_excerpt = cached.body_excerpt.unwrap_or_default();
                issue_ref.labels = cached.labels;
                issue_ref.state = Some(cached.state);
                issue_ref.url = cached.url;
                issue_ref.omit_reason = None;
            } else {
                issue_ref.omit_reason = Some("fetch_failed".to_string());
            }
            issue_ref
        }
        CacheStatus::NotFound => {
            issue_ref.omit_reason = Some("not_found".to_string());
            issue_ref
        }
        CacheStatus::Unauthorized => {
            issue_ref.omit_reason = Some("unauthorized".to_string());
            issue_ref
        }
        CacheStatus::TransientError => {
            issue_ref.omit_reason = Some("transient_error".to_string());
            issue_ref
        }
    }
}

fn cache_issue_ref(
    db: &AppDb,
    tracker: &str,
    scope: Option<&str>,
    issue_key: &str,
    status: CacheStatus,
    issue: Option<CachedIssue>,
) {
    let cache_key = build_cache_key(tracker, scope, issue_key);
    if let Ok(conn) = db.0.lock() {
        put_issue_cache(
            &conn,
            &cache_key,
            tracker,
            scope,
            issue_key,
            &status,
            issue.as_ref(),
        );
    }
}

fn jira_error_cache_status(err: &crate::integrations::jira::JiraApiError) -> CacheStatus {
    match err {
        crate::integrations::jira::JiraApiError::HttpError {
            status: 401 | 403, ..
        } => CacheStatus::Unauthorized,
        crate::integrations::jira::JiraApiError::HttpError { status: 404, .. } => {
            CacheStatus::NotFound
        }
        crate::integrations::jira::JiraApiError::RateLimited { .. } => CacheStatus::TransientError,
        _ => CacheStatus::TransientError,
    }
}

fn linear_error_cache_status(err: &crate::integrations::linear::LinearApiError) -> CacheStatus {
    match err {
        crate::integrations::linear::LinearApiError::HttpError {
            status: 401 | 403, ..
        } => CacheStatus::Unauthorized,
        crate::integrations::linear::LinearApiError::HttpError { status: 404, .. } => {
            CacheStatus::NotFound
        }
        crate::integrations::linear::LinearApiError::GraphQL(message)
            if message.to_ascii_lowercase().contains("not found") =>
        {
            CacheStatus::NotFound
        }
        crate::integrations::linear::LinearApiError::RateLimited { .. } => {
            CacheStatus::TransientError
        }
        _ => CacheStatus::TransientError,
    }
}

async fn hydrate_jira_issue_refs(
    refs: Vec<crate::context_pack::IssueRef>,
    integration_config: &IntegrationResolutionConfig,
    db: &AppDb,
) -> Vec<crate::context_pack::IssueRef> {
    if refs.is_empty() {
        return refs;
    }

    let jira_env_override = std::env::var("JIRA_API_TOKEN")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if !integration_config.jira_enabled && !jira_env_override {
        return refs
            .into_iter()
            .map(|mut issue_ref| {
                if issue_ref.omit_reason.is_none() {
                    issue_ref.omit_reason = Some("integration_disabled".to_string());
                }
                issue_ref
            })
            .collect();
    }

    let credentials = match crate::integrations::jira::resolve_jira_credentials(
        integration_config.jira_base_url.clone(),
        integration_config.jira_email.clone(),
        integration_config.jira_api_token.clone(),
    ) {
        Some(c) => c,
        None => {
            return refs
                .into_iter()
                .map(|mut issue_ref| {
                    if issue_ref.omit_reason.is_none() {
                        issue_ref.omit_reason = Some("missing_credentials".to_string());
                    }
                    issue_ref
                })
                .collect();
        }
    };

    let jira_scope = credentials.base_url.clone();
    let client = match crate::integrations::jira::JiraClient::try_new(credentials) {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("Failed to initialize Jira client: {}", err);
            return refs
                .into_iter()
                .map(|mut issue_ref| {
                    if issue_ref.omit_reason.is_none() {
                        issue_ref.omit_reason = Some("fetch_failed".to_string());
                    }
                    issue_ref
                })
                .collect();
        }
    };
    let mut hydrated = Vec::new();
    let mut total_bytes = 0usize;

    for (index, issue_ref) in refs.into_iter().enumerate() {
        if issue_ref.omit_reason.is_some() {
            hydrated.push(issue_ref);
            continue;
        }
        if index >= MAX_ISSUES {
            hydrated.push(crate::context_pack::IssueRef {
                omit_reason: Some("budget_exceeded".to_string()),
                ..issue_ref
            });
            continue;
        }

        let cache_key = build_cache_key("jira", Some(jira_scope.as_str()), &issue_ref.number);
        if let Ok(conn) = db.0.lock() {
            if let Some((status, cached)) = get_issue_cache(&conn, &cache_key) {
                hydrated.push(hydrate_issue_ref_from_cached(issue_ref, status, cached));
                continue;
            }
        }

        let mut attempt = 0usize;
        let fetched = loop {
            match client.get_issue(&issue_ref.number).await {
                Err(crate::integrations::jira::JiraApiError::RateLimited {
                    retry_after_secs,
                    ..
                }) if attempt < 2 => {
                    let delay_secs = retry_after_secs.unwrap_or(1u64 << attempt).clamp(1, 8);
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                    attempt += 1;
                }
                other => break other,
            }
        };

        match fetched {
            Ok(info) => {
                let body_excerpt = info
                    .body_excerpt
                    .as_deref()
                    .map(|b| truncate_to_bytes(b, MAX_ISSUE_BODY_EXCERPT_BYTES))
                    .unwrap_or_default();
                let estimated_bytes = info.title.len()
                    + body_excerpt.len()
                    + info.labels.iter().map(String::len).sum::<usize>()
                    + 64;
                if total_bytes + estimated_bytes > MAX_ISSUE_CONTEXT_BYTES_TOTAL {
                    hydrated.push(crate::context_pack::IssueRef {
                        omit_reason: Some("budget_exceeded".to_string()),
                        ..issue_ref
                    });
                    continue;
                }
                total_bytes += estimated_bytes;
                let issue_number = issue_ref.number.clone();
                let issue_url = issue_ref.url.clone();
                let hydrated_issue = crate::context_pack::IssueRef {
                    number: info.key,
                    title: info.title,
                    body_excerpt,
                    labels: info.labels,
                    state: Some(info.state),
                    omit_reason: None,
                    ..issue_ref
                };
                let cached_issue = CachedIssue {
                    tracker: "jira".to_string(),
                    issue_key: issue_number.clone(),
                    title: hydrated_issue.title.clone(),
                    body_excerpt: if hydrated_issue.body_excerpt.is_empty() {
                        None
                    } else {
                        Some(hydrated_issue.body_excerpt.clone())
                    },
                    labels: hydrated_issue.labels.clone(),
                    state: hydrated_issue
                        .state
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    url: issue_url,
                };
                cache_issue_ref(
                    db,
                    "jira",
                    Some(jira_scope.as_str()),
                    &issue_number,
                    CacheStatus::Ok,
                    Some(cached_issue),
                );
                hydrated.push(hydrated_issue);
            }
            Err(err) => {
                tracing::warn!("Failed to hydrate Jira issue {}: {}", issue_ref.number, err);
                let issue_number = issue_ref.number.clone();
                let cache_status = jira_error_cache_status(&err);
                let failed_issue = crate::context_pack::IssueRef {
                    omit_reason: Some(cache_status.as_str().to_string()),
                    ..issue_ref
                };
                cache_issue_ref(
                    db,
                    "jira",
                    Some(jira_scope.as_str()),
                    &issue_number,
                    cache_status,
                    None,
                );
                hydrated.push(failed_issue);
            }
        }
    }

    hydrated
}

async fn hydrate_linear_issue_refs(
    refs: Vec<crate::context_pack::IssueRef>,
    integration_config: &IntegrationResolutionConfig,
    db: &AppDb,
) -> Vec<crate::context_pack::IssueRef> {
    if refs.is_empty() {
        return refs;
    }

    let linear_env_override = std::env::var("LINEAR_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    if !integration_config.linear_enabled && !linear_env_override {
        return refs
            .into_iter()
            .map(|mut issue_ref| {
                if issue_ref.omit_reason.is_none() {
                    issue_ref.omit_reason = Some("integration_disabled".to_string());
                }
                issue_ref
            })
            .collect();
    }

    let credentials = match crate::integrations::linear::resolve_linear_credentials(
        integration_config.linear_api_key.clone(),
    ) {
        Some(c) => c,
        None => {
            return refs
                .into_iter()
                .map(|mut issue_ref| {
                    if issue_ref.omit_reason.is_none() {
                        issue_ref.omit_reason = Some("missing_credentials".to_string());
                    }
                    issue_ref
                })
                .collect();
        }
    };

    let client = match crate::integrations::linear::LinearClient::try_new(credentials) {
        Ok(client) => client,
        Err(err) => {
            tracing::warn!("Failed to initialize Linear client: {}", err);
            return refs
                .into_iter()
                .map(|mut issue_ref| {
                    if issue_ref.omit_reason.is_none() {
                        issue_ref.omit_reason = Some("fetch_failed".to_string());
                    }
                    issue_ref
                })
                .collect();
        }
    };

    let scope = integration_config
        .linear_workspace
        .as_deref()
        .unwrap_or("linear");
    let mut hydrated = Vec::new();
    let mut total_bytes = 0usize;

    for (index, issue_ref) in refs.into_iter().enumerate() {
        if issue_ref.omit_reason.is_some() {
            hydrated.push(issue_ref);
            continue;
        }
        if index >= MAX_ISSUES {
            hydrated.push(crate::context_pack::IssueRef {
                omit_reason: Some("budget_exceeded".to_string()),
                ..issue_ref
            });
            continue;
        }

        let cache_key = build_cache_key("linear", Some(scope), &issue_ref.number);
        if let Ok(conn) = db.0.lock() {
            if let Some((status, cached)) = get_issue_cache(&conn, &cache_key) {
                hydrated.push(hydrate_issue_ref_from_cached(issue_ref, status, cached));
                continue;
            }
        }

        let mut attempt = 0usize;
        let fetched = loop {
            match client.get_issue(&issue_ref.number).await {
                Err(crate::integrations::linear::LinearApiError::RateLimited {
                    retry_after_secs,
                }) if attempt < 2 => {
                    let delay_secs = retry_after_secs.unwrap_or(1u64 << attempt).clamp(1, 8);
                    tokio::time::sleep(std::time::Duration::from_secs(delay_secs)).await;
                    attempt += 1;
                }
                other => break other,
            }
        };

        match fetched {
            Ok(info) => {
                let body_excerpt = info
                    .body_excerpt
                    .as_deref()
                    .map(|b| truncate_to_bytes(b, MAX_ISSUE_BODY_EXCERPT_BYTES))
                    .unwrap_or_default();
                let estimated_bytes = info.title.len()
                    + body_excerpt.len()
                    + info.labels.iter().map(String::len).sum::<usize>()
                    + 64;
                if total_bytes + estimated_bytes > MAX_ISSUE_CONTEXT_BYTES_TOTAL {
                    hydrated.push(crate::context_pack::IssueRef {
                        omit_reason: Some("budget_exceeded".to_string()),
                        ..issue_ref
                    });
                    continue;
                }
                total_bytes += estimated_bytes;
                let issue_number = issue_ref.number.clone();
                let hydrated_issue = crate::context_pack::IssueRef {
                    number: info.identifier,
                    title: info.title,
                    body_excerpt,
                    labels: info.labels,
                    state: Some(info.state),
                    url: info.url,
                    omit_reason: None,
                    ..issue_ref
                };
                let cached_issue = CachedIssue {
                    tracker: "linear".to_string(),
                    issue_key: issue_number.clone(),
                    title: hydrated_issue.title.clone(),
                    body_excerpt: if hydrated_issue.body_excerpt.is_empty() {
                        None
                    } else {
                        Some(hydrated_issue.body_excerpt.clone())
                    },
                    labels: hydrated_issue.labels.clone(),
                    state: hydrated_issue
                        .state
                        .clone()
                        .unwrap_or_else(|| "unknown".to_string()),
                    url: hydrated_issue.url.clone(),
                };
                cache_issue_ref(
                    db,
                    "linear",
                    Some(scope),
                    &issue_number,
                    CacheStatus::Ok,
                    Some(cached_issue),
                );
                hydrated.push(hydrated_issue);
            }
            Err(err) => {
                tracing::warn!(
                    "Failed to hydrate Linear issue {}: {}",
                    issue_ref.number,
                    err
                );
                let issue_number = issue_ref.number.clone();
                let cache_status = linear_error_cache_status(&err);
                let failed_issue = crate::context_pack::IssueRef {
                    omit_reason: Some(cache_status.as_str().to_string()),
                    ..issue_ref
                };
                cache_issue_ref(db, "linear", Some(scope), &issue_number, cache_status, None);
                hydrated.push(failed_issue);
            }
        }
    }

    hydrated
}

#[derive(Debug, Clone, Default)]
struct IntegrationResolutionConfig {
    jira_enabled: bool,
    jira_base_url: Option<String>,
    jira_email: Option<String>,
    jira_api_token: Option<String>,
    jira_project_keys: Vec<String>,
    linear_enabled: bool,
    linear_workspace: Option<String>,
    linear_api_key: Option<String>,
    linear_team_keys: Vec<String>,
}

fn parse_bool_setting(value: Option<String>) -> bool {
    value
        .map(|v| {
            let normalized = v.trim().to_ascii_lowercase();
            normalized == "true" || normalized == "1" || normalized == "yes" || normalized == "on"
        })
        .unwrap_or(false)
}

fn parse_csv_setting(value: Option<String>) -> Vec<String> {
    value
        .unwrap_or_default()
        .split(',')
        .map(|v| v.trim().to_ascii_uppercase())
        .filter(|v| !v.is_empty())
        .collect()
}

fn load_integration_resolution_config(db: &AppDb) -> IntegrationResolutionConfig {
    let mut config = IntegrationResolutionConfig::default();

    if let Ok(conn) = db.0.lock() {
        config.jira_enabled = parse_bool_setting(
            queries::get_setting(&conn, "integration_jira_enabled")
                .ok()
                .flatten(),
        );
        config.jira_base_url = queries::get_setting(&conn, "integration_jira_base_url")
            .ok()
            .flatten()
            .map(|v| v.trim().trim_end_matches('/').to_string())
            .filter(|v| !v.is_empty());
        config.jira_email = queries::get_setting(&conn, "integration_jira_email")
            .ok()
            .flatten()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        config.jira_project_keys = parse_csv_setting(
            queries::get_setting(&conn, "integration_jira_project_keys")
                .ok()
                .flatten(),
        );

        config.linear_enabled = parse_bool_setting(
            queries::get_setting(&conn, "integration_linear_enabled")
                .ok()
                .flatten(),
        );
        config.linear_workspace = queries::get_setting(&conn, "integration_linear_workspace")
            .ok()
            .flatten()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        config.linear_team_keys = parse_csv_setting(
            queries::get_setting(&conn, "integration_linear_team_keys")
                .ok()
                .flatten(),
        );
    }

    config.jira_api_token =
        integration_secrets::resolve_secret(IntegrationSecretField::JiraApiToken)
            .ok()
            .and_then(|(value, _source)| value);
    config.linear_api_key =
        integration_secrets::resolve_secret(IntegrationSecretField::LinearApiKey)
            .ok()
            .and_then(|(value, _source)| value);

    config
}

fn build_external_issue_refs(
    platform_metadata: Option<&crate::platform::adapter::PlatformMetadata>,
    pr_title: &str,
    pr_head_branch: Option<&str>,
    integration_config: &IntegrationResolutionConfig,
) -> Vec<crate::context_pack::IssueRef> {
    let mut text_parts = Vec::new();
    if !pr_title.trim().is_empty() {
        text_parts.push(pr_title.trim().to_string());
    }
    if let Some(head_branch) = pr_head_branch.map(str::trim).filter(|b| !b.is_empty()) {
        text_parts.push(head_branch.to_string());
    }

    if let Some(metadata) = platform_metadata {
        match metadata {
            crate::platform::adapter::PlatformMetadata::GitHub(g) => {
                if let Some(body) = g
                    .pr_body
                    .as_deref()
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                {
                    text_parts.push(body.to_string());
                }
                if !g.head_ref.trim().is_empty() {
                    text_parts.push(g.head_ref.trim().to_string());
                }
            }
            crate::platform::adapter::PlatformMetadata::GitLab(g) => {
                if let Some(body) = g
                    .mr_body
                    .as_deref()
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                {
                    text_parts.push(body.to_string());
                }
                if !g.head_ref.trim().is_empty() {
                    text_parts.push(g.head_ref.trim().to_string());
                }
            }
            crate::platform::adapter::PlatformMetadata::Bitbucket(b) => {
                if let Some(body) = b
                    .pr_body
                    .as_deref()
                    .map(str::trim)
                    .filter(|b| !b.is_empty())
                {
                    text_parts.push(body.to_string());
                }
                if !b.head_ref.trim().is_empty() {
                    text_parts.push(b.head_ref.trim().to_string());
                }
            }
        }
    }

    let extraction_input = text_parts.join("\n");
    if extraction_input.trim().is_empty() {
        return Vec::new();
    }

    let extraction_config = IssueExtractionConfig {
        jira_project_keys: integration_config.jira_project_keys.clone(),
        linear_team_keys: integration_config.linear_team_keys.clone(),
        jira_enabled: integration_config.jira_enabled,
        linear_enabled: integration_config.linear_enabled,
    };
    let candidates = extract_all_external_issues(&extraction_input, &extraction_config);
    candidates
        .into_iter()
        .map(|candidate| {
            let tracker = candidate.tracker.clone();
            let title_prefix = match tracker.as_str() {
                "jira" => "Jira issue",
                "linear" => "Linear issue",
                _ => "Issue",
            };
            crate::context_pack::IssueRef {
                number: candidate.key.clone(),
                title: format!("{title_prefix} {}", candidate.key),
                body_excerpt: String::new(),
                owner: candidate.owner,
                repo: candidate.repo,
                labels: Vec::new(),
                state: None,
                omit_reason: candidate.omit_reason,
                tracker: Some(tracker),
                confidence: Some(candidate.confidence),
                url: candidate.url,
                origin: Some(candidate.origin),
            }
        })
        .collect()
}

fn dedupe_issue_refs(
    refs: Vec<crate::context_pack::IssueRef>,
) -> Vec<crate::context_pack::IssueRef> {
    let mut dedup = std::collections::HashSet::new();
    refs.into_iter()
        .filter(|issue_ref| {
            let tracker = issue_ref.tracker.as_deref().unwrap_or("github");
            let key = format!(
                "{tracker}:{}:{}:{}",
                issue_ref.owner.as_deref().unwrap_or(""),
                issue_ref.repo.as_deref().unwrap_or(""),
                issue_ref.number
            );
            dedup.insert(key)
        })
        .collect()
}

fn apply_global_issue_budget(refs: &mut [crate::context_pack::IssueRef], max_issues: usize) {
    let mut hydrate_budget = 0usize;
    for issue_ref in refs.iter_mut() {
        if issue_ref.omit_reason.is_some() {
            continue;
        }
        hydrate_budget += 1;
        if hydrate_budget > max_issues {
            issue_ref.omit_reason = Some("budget_exceeded".to_string());
        }
    }
}

async fn hydrate_issue_refs_by_tracker(
    app: &AppHandle,
    adapter: Option<&dyn crate::platform::adapter::PlatformAdapter>,
    refs: Vec<crate::context_pack::IssueRef>,
    integration_config: &IntegrationResolutionConfig,
    db: &AppDb,
) -> Vec<crate::context_pack::IssueRef> {
    let mut passthrough: Vec<(usize, crate::context_pack::IssueRef)> = Vec::new();
    let mut github_refs: Vec<(usize, crate::context_pack::IssueRef)> = Vec::new();
    let mut gitlab_refs: Vec<(usize, crate::context_pack::IssueRef)> = Vec::new();
    let mut jira_refs: Vec<(usize, crate::context_pack::IssueRef)> = Vec::new();
    let mut linear_refs: Vec<(usize, crate::context_pack::IssueRef)> = Vec::new();

    for (idx, issue_ref) in refs.into_iter().enumerate() {
        if issue_ref.omit_reason.is_some() {
            passthrough.push((idx, issue_ref));
            continue;
        }

        match issue_ref.tracker.as_deref().unwrap_or("github") {
            "gitlab" => gitlab_refs.push((idx, issue_ref)),
            "jira" => jira_refs.push((idx, issue_ref)),
            "linear" => linear_refs.push((idx, issue_ref)),
            _ => github_refs.push((idx, issue_ref)),
        }
    }

    let mut hydrated: Vec<(usize, crate::context_pack::IssueRef)> = passthrough;

    if !github_refs.is_empty() {
        let indexes: Vec<usize> = github_refs.iter().map(|(idx, _)| *idx).collect();
        let payload: Vec<crate::context_pack::IssueRef> = github_refs
            .into_iter()
            .map(|(_, issue_ref)| issue_ref)
            .collect();
        let hydrated_payload = hydrate_issue_refs_from_github(app, payload).await;
        hydrated.extend(indexes.into_iter().zip(hydrated_payload));
    }

    if !gitlab_refs.is_empty() {
        let indexes: Vec<usize> = gitlab_refs.iter().map(|(idx, _)| *idx).collect();
        let payload: Vec<crate::context_pack::IssueRef> = gitlab_refs
            .into_iter()
            .map(|(_, issue_ref)| issue_ref)
            .collect();
        let hydrated_payload = if let Some(active_adapter) = adapter {
            if active_adapter.platform_name() == "gitlab" {
                hydrate_issue_refs_via_adapter(active_adapter, payload).await
            } else {
                payload
                    .into_iter()
                    .map(|mut issue_ref| {
                        issue_ref.omit_reason = Some("fetch_failed".to_string());
                        issue_ref
                    })
                    .collect()
            }
        } else {
            payload
                .into_iter()
                .map(|mut issue_ref| {
                    issue_ref.omit_reason = Some("fetch_failed".to_string());
                    issue_ref
                })
                .collect()
        };
        hydrated.extend(indexes.into_iter().zip(hydrated_payload));
    }

    if !jira_refs.is_empty() {
        let indexes: Vec<usize> = jira_refs.iter().map(|(idx, _)| *idx).collect();
        let payload: Vec<crate::context_pack::IssueRef> = jira_refs
            .into_iter()
            .map(|(_, issue_ref)| issue_ref)
            .collect();
        let hydrated_payload = hydrate_jira_issue_refs(payload, integration_config, db).await;
        hydrated.extend(indexes.into_iter().zip(hydrated_payload));
    }

    if !linear_refs.is_empty() {
        let indexes: Vec<usize> = linear_refs.iter().map(|(idx, _)| *idx).collect();
        let payload: Vec<crate::context_pack::IssueRef> = linear_refs
            .into_iter()
            .map(|(_, issue_ref)| issue_ref)
            .collect();
        let hydrated_payload = hydrate_linear_issue_refs(payload, integration_config, db).await;
        hydrated.extend(indexes.into_iter().zip(hydrated_payload));
    }

    hydrated.sort_by_key(|(idx, _)| *idx);
    hydrated
        .into_iter()
        .map(|(_, issue_ref)| issue_ref)
        .collect()
}

pub(crate) async fn resolve_issue_refs_and_codeowners(
    app: &AppHandle,
    db: &AppDb,
    pr_id: &str,
) -> (
    Vec<crate::context_pack::IssueRef>,
    Option<String>,
    Option<String>,
) {
    let (pr_url, metadata_json, pr_title, pr_head_branch) = {
        let conn = db.0.lock().ok();
        conn.and_then(|c| {
            let pr = queries::get_pull_request(&c, pr_id).ok()??;
            Some((pr.url, pr.platform_metadata_json, pr.title, pr.head_branch))
        })
        .unwrap_or_default()
    };
    if pr_url.is_empty() {
        return (Vec::new(), None, None);
    }

    let review_url = match crate::platform::parse_review_url(&pr_url) {
        Ok(url) => url,
        Err(err) => {
            tracing::warn!("Unable to parse review URL for context hydration: {}", err);
            return (Vec::new(), None, None);
        }
    };
    let (default_owner, default_repo) = review_url.owner_and_repo();

    let platform_metadata = metadata_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<crate::platform::adapter::PlatformMetadata>(j).ok());
    let legacy_github_metadata = metadata_json.as_deref().and_then(|j| {
        serde_json::from_str::<crate::providers::github::PlatformMetadataSnapshot>(j).ok()
    });

    let platform_issue_refs_seed = if platform_metadata.is_some() {
        build_issue_refs_from_platform_metadata(
            platform_metadata.as_ref(),
            &default_owner,
            &default_repo,
        )
    } else {
        build_issue_refs_from_metadata(
            legacy_github_metadata.as_ref(),
            &default_owner,
            &default_repo,
        )
    };

    let integration_config = load_integration_resolution_config(db);
    if let Ok(conn) = db.0.lock() {
        crate::integrations::cache::prune_issue_cache(&conn);
    }
    let external_issue_refs_seed = build_external_issue_refs(
        platform_metadata.as_ref(),
        &pr_title,
        pr_head_branch.as_deref(),
        &integration_config,
    );
    let mut issue_refs_seed = dedupe_issue_refs(
        platform_issue_refs_seed
            .into_iter()
            .chain(external_issue_refs_seed)
            .collect(),
    );
    apply_global_issue_budget(&mut issue_refs_seed, MAX_ISSUES);

    let adapter = crate::platform::factory::build_adapter(app, &review_url)
        .await
        .ok();
    let issue_refs = hydrate_issue_refs_by_tracker(
        app,
        adapter.as_deref(),
        issue_refs_seed,
        &integration_config,
        db,
    )
    .await;

    let base_ref = if let Some(meta) = platform_metadata.as_ref() {
        match meta {
            crate::platform::adapter::PlatformMetadata::GitHub(g) => g.base_ref.as_str(),
            crate::platform::adapter::PlatformMetadata::GitLab(g) => g.base_ref.as_str(),
            crate::platform::adapter::PlatformMetadata::Bitbucket(b) => b.base_ref.as_str(),
        }
    } else {
        legacy_github_metadata
            .as_ref()
            .map(|m| m.base_ref.as_str())
            .unwrap_or("main")
    };

    if let Some(adapter) = adapter.as_ref() {
        let platform_name = adapter.platform_name();
        let codeowners =
            fetch_codeowners_via_adapter(adapter.as_ref(), base_ref, platform_name).await;
        (
            issue_refs,
            codeowners,
            Some(format!("{platform_name}:base-branch:CODEOWNERS")),
        )
    } else if let crate::platform::ParsedReviewUrl::GitHub { .. } = review_url {
        let codeowners =
            fetch_base_branch_codeowners(app, &default_owner, &default_repo, base_ref).await;
        (
            issue_refs,
            codeowners,
            Some("github:base-branch:CODEOWNERS".to_string()),
        )
    } else {
        (issue_refs, None, None)
    }
}

fn parse_issue_ref(
    text_ref: &str,
    default_owner: &str,
    default_repo: &str,
) -> Option<(String, String, String)> {
    let trimmed = text_ref
        .trim()
        .trim_end_matches(|c: char| ",.;)]".contains(c));
    if trimmed.is_empty() {
        return None;
    }

    if let Some((repo_ref, num)) = trimmed.rsplit_once('#') {
        let number = num.trim();
        if !number.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        if repo_ref.contains('/') {
            let (owner, repo) = repo_ref.split_once('/')?;
            if owner.is_empty() || repo.is_empty() {
                return None;
            }
            return Some((owner.to_string(), repo.to_string(), number.to_string()));
        }
        return Some((
            default_owner.to_string(),
            default_repo.to_string(),
            number.to_string(),
        ));
    }

    let bare = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if bare.chars().all(|c| c.is_ascii_digit()) {
        return Some((
            default_owner.to_string(),
            default_repo.to_string(),
            bare.to_string(),
        ));
    }

    None
}

fn issue_fetch_omit_reason(error: &GitHubApiError) -> &'static str {
    match error {
        GitHubApiError::HttpError { status: 404, .. } => "no_access_404",
        GitHubApiError::HttpError { status: 301, .. } => "transferred_301",
        GitHubApiError::HttpError { status: 410, .. } => "gone_410",
        _ => "fetch_failed",
    }
}

fn truncate_to_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
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
                head_sha_at_run: extract_head_sha_from_pr(&queries::get_pull_request(
                    &conn, &pr_id,
                )?),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
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

        // Build context pack
        let preference_text = {
            let conn = db.0.lock().ok();
            conn.and_then(|c| {
                let decisions = queries::get_all_decisions(&c).ok()?;
                let summaries = scoring::compute_preference_summaries(&decisions);
                scoring::build_preference_prompt_section(&summaries)
            })
        };

        let (issue_refs, base_branch_codeowners, codeowners_source) =
            resolve_issue_refs_and_codeowners(&app_clone, &db, &pr_id).await;

        let context_pack = ContextPackBuilder::new(&context_pack_config, &cwd_path, &changed_files)
            .with_preferences(preference_text)
            .with_codeowners_content(base_branch_codeowners.clone(), codeowners_source)
            .with_issues(issue_refs)
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

        // Run local checks if enabled
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

        let owners_by_path = {
            let raw = base_branch_codeowners
                .or_else(|| crate::context_pack::read_local_codeowners(&cwd_path));
            raw.map(|content| {
                crate::context_pack::resolve_codeowners(&content, &changed_files)
                    .into_iter()
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default()
        };

        let (issue_context_included_count, issue_context_sources) = {
            let issue_items: Vec<_> = context_pack
                .items
                .iter()
                .filter(|i| i.kind == "issue" && i.included)
                .collect();
            let count = issue_items.len();
            let sources: Vec<String> = issue_items.iter().map(|i| i.source.clone()).collect();
            (count, sources)
        };

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
                owners_by_path,
                issue_context_included_count,
                issue_context_sources,
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
    use crate::storage::models::{Finding, PullRequest, ReviewRun, SubmissionRecord, Workspace};
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
            remote_host: "github.com".into(),
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
            platform_metadata_json: None,
            platform_metadata_fetched_at: None,
            platform_capabilities_json: None,
            platform_capabilities_fetched_at: None,
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
            platform_metadata_json: None,
            platform_metadata_fetched_at: None,
            platform_capabilities_json: None,
            platform_capabilities_fetched_at: None,
        };
        queries::insert_pull_request(&conn, &current_pr).unwrap();

        let baseline_run = ReviewRun {
            id: "run-base".into(),
            pr_id: baseline_pr.id.clone(),
            status: "ready".into(),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            completed_at: Some("2026-01-01T00:01:00Z".into()),
            error_message: None,
            head_sha_at_run: None,
            baseline_run_id: None,
            metrics_json: None,
            analysis_diff_hash: None,
            analysis_diff_text: Some(baseline_diff.into()),
            context_pack_json: None,
            local_checks_json: None,
            provider_selection_json: None,
            rerun_trigger_source: None,
            rerun_reason: None,
            rerun_scope: None,
        };
        queries::insert_review_run(&conn, &baseline_run).unwrap();

        let current_run = ReviewRun {
            id: "run-current".into(),
            pr_id: current_pr.id.clone(),
            status: "ready".into(),
            started_at: Some("2026-01-02T00:00:00Z".into()),
            completed_at: Some("2026-01-02T00:01:00Z".into()),
            error_message: None,
            head_sha_at_run: None,
            baseline_run_id: Some(baseline_run.id.clone()),
            metrics_json: None,
            analysis_diff_hash: None,
            analysis_diff_text: Some(baseline_diff.into()),
            context_pack_json: None,
            local_checks_json: None,
            provider_selection_json: None,
            rerun_trigger_source: None,
            rerun_reason: None,
            rerun_scope: None,
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

    #[test]
    fn build_review_freshness_summary_prefers_submission_commit_for_submitted_run() {
        let db = init_db_in_memory().expect("in-memory DB");
        let conn = db.0.into_inner().expect("owned connection");

        let workspace = Workspace {
            id: "ws-submitted".into(),
            local_path: "/tmp/repo".into(),
            remote_owner: "o".into(),
            remote_repo: "r".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            remote_host: "github.com".into(),
        };
        queries::insert_workspace(&conn, &workspace).unwrap();

        let metadata_json = r#"{"platform":"github","pr_body":null,"head_sha":"sha-current","base_sha":"sha-base","base_ref":"main","head_ref":"feature","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#;
        let pr = PullRequest {
            id: "pr-submitted".into(),
            workspace_id: workspace.id.clone(),
            pr_number: 7,
            title: "Submitted".into(),
            author: None,
            base_branch: None,
            head_branch: None,
            url: "https://github.com/o/r/pull/7".into(),
            diff_text: Some("diff".into()),
            changed_files: Some("[\"src/lib.rs\"]".into()),
            fetched_at: "2026-01-02T00:00:00Z".into(),
            diff_hash: Some("hash".into()),
            platform_metadata_json: Some(metadata_json.into()),
            platform_metadata_fetched_at: Some("2026-01-02T00:00:00Z".into()),
            platform_capabilities_json: None,
            platform_capabilities_fetched_at: None,
        };
        queries::insert_pull_request(&conn, &pr).unwrap();

        let run = ReviewRun {
            id: "run-submitted".into(),
            pr_id: pr.id.clone(),
            status: "submitted".into(),
            started_at: Some("2026-01-02T00:00:00Z".into()),
            completed_at: Some("2026-01-02T00:05:00Z".into()),
            error_message: None,
            head_sha_at_run: Some("sha-reviewed".into()),
            baseline_run_id: None,
            metrics_json: None,
            analysis_diff_hash: Some("hash".into()),
            analysis_diff_text: Some("diff".into()),
            context_pack_json: None,
            local_checks_json: None,
            provider_selection_json: None,
            rerun_trigger_source: None,
            rerun_reason: None,
            rerun_scope: None,
        };
        queries::insert_review_run(&conn, &run).unwrap();
        queries::insert_submission(
            &conn,
            &SubmissionRecord {
                id: "sub-submitted".into(),
                review_run_id: run.id.clone(),
                review_action: "comment".into(),
                submitted_at: Some("2026-01-02T00:05:00Z".into()),
                status: "submitted".into(),
                commit_id_at_submission: Some("sha-submitted".into()),
                platform_review_id: None,
                error_message: None,
                idempotency_key: None,
                attempt_count: Some(1),
                last_attempt_at: Some("2026-01-02T00:05:00Z".into()),
            },
        )
        .unwrap();

        let freshness = build_review_freshness_summary(&conn, &run, &pr).unwrap();
        assert_eq!(
            freshness.reviewed_head_sha.as_deref(),
            Some("sha-submitted")
        );
        assert_eq!(freshness.current_head_sha.as_deref(), Some("sha-current"));
        assert!(freshness.head_changed_since_review);
    }

    // ---- build_issue_refs_from_metadata tests ----

    #[test]
    fn test_build_issue_refs_none_metadata() {
        let refs = build_issue_refs_from_metadata(None, "octo", "repo");
        assert!(refs.is_empty());
    }

    #[test]
    fn test_build_issue_refs_linked_only() {
        let meta = crate::providers::github::PlatformMetadataSnapshot {
            pr_body: None,
            linked_issue_numbers: vec![10, 20],
            text_issue_refs: vec![],
            requested_reviewers: vec![],
            requested_teams: vec![],
            review_state_summary: vec![],
            labels: vec![],
            draft: false,
            head_sha: "abc".into(),
            base_sha: "def".into(),
            base_ref: "main".into(),
            head_ref: "feature".into(),
        };
        let refs = build_issue_refs_from_metadata(Some(&meta), "octo", "repo");
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].number, "10");
        assert_eq!(refs[1].number, "20");
        assert_eq!(refs[0].owner.as_deref(), Some("octo"));
        assert_eq!(refs[0].repo.as_deref(), Some("repo"));
    }

    #[test]
    fn test_build_issue_refs_text_refs_deduped() {
        let meta = crate::providers::github::PlatformMetadataSnapshot {
            pr_body: None,
            linked_issue_numbers: vec![5],
            text_issue_refs: vec!["5".into(), "#5".into(), "#99".into()],
            requested_reviewers: vec![],
            requested_teams: vec![],
            review_state_summary: vec![],
            labels: vec![],
            draft: false,
            head_sha: "abc".into(),
            base_sha: "def".into(),
            base_ref: "main".into(),
            head_ref: "feature".into(),
        };
        let refs = build_issue_refs_from_metadata(Some(&meta), "octo", "repo");
        assert_eq!(refs.len(), 2, "linked #5 and text #99; text #5 deduped");
        assert!(refs.iter().any(|r| r.number == "5"));
        assert!(refs.iter().any(|r| r.number == "99"));
    }

    #[test]
    fn test_build_issue_refs_cross_repo() {
        let meta = crate::providers::github::PlatformMetadataSnapshot {
            pr_body: None,
            linked_issue_numbers: vec![],
            text_issue_refs: vec!["owner/repo#42".into()],
            requested_reviewers: vec![],
            requested_teams: vec![],
            review_state_summary: vec![],
            labels: vec![],
            draft: false,
            head_sha: "abc".into(),
            base_sha: "def".into(),
            base_ref: "main".into(),
            head_ref: "feature".into(),
        };
        let refs = build_issue_refs_from_metadata(Some(&meta), "octo", "repo");
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].number, "42");
        assert_eq!(refs[0].owner.as_deref(), Some("owner"));
        assert_eq!(refs[0].repo.as_deref(), Some("repo"));
    }

    #[test]
    fn test_parse_issue_ref_accepts_bare_and_hash() {
        assert_eq!(
            parse_issue_ref("123", "a", "b"),
            Some(("a".into(), "b".into(), "123".into()))
        );
        assert_eq!(
            parse_issue_ref("#456", "a", "b"),
            Some(("a".into(), "b".into(), "456".into()))
        );
    }
}
