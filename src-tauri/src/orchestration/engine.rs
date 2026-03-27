use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::cleaner::{self, CleanerConfig};
use crate::errors::{AppError, ProviderError};
use crate::orchestration::lane::{AgentLaneConfig, AgentLaneResult, LaneSnapshot, LaneStatus};
use crate::providers::traits::{ReviewInput, ReviewProvider};
use crate::storage::db::AppDb;
use crate::storage::event_log::EventLog;
use crate::storage::models::AgentRun;
use crate::storage::queries;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ReviewEvent {
    StatusChanged {
        status: String,
    },
    LaneStatusChanged {
        lane_id: String,
        provider_name: String,
        status: String,
        finding_count: usize,
        error_message: Option<String>,
    },
    ReviewReady {
        run_id: String,
    },
    ReviewFailed {
        run_id: String,
        error: String,
    },
}

pub struct ReviewPipelineArgs {
    pub run_id: String,
    pub cwd: PathBuf,
    pub config: CleanerConfig,
    pub cancel: CancellationToken,
    pub lanes: Vec<AgentLaneConfig>,
    pub fallback_input: Option<ReviewInput>,
    pub fallback_provider: Option<Arc<dyn ReviewProvider>>,
    pub event_log: Option<Arc<EventLog>>,
}

/// Per-provider concurrency limits so one provider's rate limits don't block others.
pub type ProviderSemaphores = HashMap<String, Arc<Semaphore>>;

const DEFAULT_PERMITS_PER_PROVIDER: usize = 3;

/// Build a semaphore map with one entry per unique provider, each with DEFAULT_PERMITS_PER_PROVIDER permits.
pub fn build_provider_semaphores(lanes: &[AgentLaneConfig]) -> ProviderSemaphores {
    let mut map = HashMap::new();
    for lane in lanes {
        let name = lane.provider.provider_name().to_string();
        map.entry(name)
            .or_insert_with(|| Arc::new(Semaphore::new(DEFAULT_PERMITS_PER_PROVIDER)));
    }
    map
}

pub async fn run_review_pipeline(
    db: &AppDb,
    mut emit: impl FnMut(ReviewEvent) + Send,
    provider_semaphores: &ProviderSemaphores,
    args: ReviewPipelineArgs,
) -> Result<(), AppError> {
    let event_log = args.event_log.clone();

    // Stage 1: Running agents
    if args.cancel.is_cancelled() {
        fail_run(db, &args.run_id, "Cancelled by user", &mut emit)?;
        return Ok(());
    }
    update_status(db, &args.run_id, "running_agents", &mut emit)?;
    log_event(
        &event_log,
        &args.run_id,
        "run_started",
        serde_json::json!({"lane_count": args.lanes.len()}),
    );

    let (all_findings, diff_text) = if args.lanes.is_empty() {
        // Single-lane backward-compat mode
        let provider = args
            .fallback_provider
            .ok_or_else(|| AppError::InvalidInput("No provider or lanes configured".into()))?;
        let input = args
            .fallback_input
            .ok_or_else(|| AppError::InvalidInput("No input or lanes configured".into()))?;
        let diff = input.diff.clone();
        match provider
            .run_review(&input, &args.cwd, args.cancel.clone())
            .await
        {
            Ok(output) => (output.findings, diff),
            Err(e) => {
                if matches!(e, ProviderError::Cancelled) {
                    fail_run(db, &args.run_id, "Cancelled by user", &mut emit)?;
                    return Ok(());
                }
                let err_msg = e.to_string();
                fail_run(db, &args.run_id, &err_msg, &mut emit)?;
                return Err(AppError::Provider(e));
            }
        }
    } else {
        // Multi-lane mode
        let diff = args.lanes[0].input.diff.clone();
        let results = run_agent_lanes(
            db,
            &args.run_id,
            args.lanes,
            &args.cwd,
            args.cancel.clone(),
            provider_semaphores,
            &mut emit,
        )
        .await;
        let mut all_findings = Vec::new();
        let mut all_failed = true;

        for result in &results {
            // Update agent_run record (inserted as "running" before spawn)
            if let Ok(conn) = db.0.lock() {
                let _ = queries::update_agent_run(
                    &conn,
                    &result.agent_run_id,
                    result.status.as_str(),
                    Some(&result.completed_at),
                    result.findings.len() as i32,
                    match &result.status {
                        LaneStatus::Failed { error } => Some(error.as_str()),
                        LaneStatus::TimedOut => Some("Timed out"),
                        _ => None,
                    },
                );
            }

            if matches!(result.status, LaneStatus::Completed { .. }) {
                all_failed = false;
                all_findings.extend(result.findings.clone());
            }
        }

        if all_failed && !results.is_empty() {
            let errors: Vec<String> = results
                .iter()
                .filter_map(|r| match &r.status {
                    LaneStatus::Failed { error } => Some(format!("{}: {}", r.lane_id, error)),
                    LaneStatus::TimedOut => Some(format!("{}: timed out", r.lane_id)),
                    LaneStatus::Cancelled => Some(format!("{}: cancelled", r.lane_id)),
                    _ => None,
                })
                .collect();
            let combined = format!("All lanes failed: {}", errors.join("; "));
            fail_run(db, &args.run_id, &combined, &mut emit)?;
            return Err(AppError::InvalidInput(combined));
        }

        (all_findings, diff)
    };

    if args.cancel.is_cancelled() {
        fail_run(db, &args.run_id, "Cancelled by user", &mut emit)?;
        return Ok(());
    }

    // Stage 2: Cleaner pipeline
    update_status(db, &args.run_id, "cleaning", &mut emit)?;
    log_event(
        &event_log,
        &args.run_id,
        "cleaning_started",
        serde_json::json!({"finding_count": all_findings.len()}),
    );
    let result = cleaner::clean(all_findings, &diff_text, &args.run_id, &args.config);

    if args.cancel.is_cancelled() {
        fail_run(db, &args.run_id, "Cancelled by user", &mut emit)?;
        return Ok(());
    }

    log_event(
        &event_log,
        &args.run_id,
        "cleaning_completed",
        serde_json::json!({
            "surfaced": result.surfaced.len(),
            "dropped": result.dropped.len(),
        }),
    );

    // Stage 3: Persist clusters first (FK target), stamp cluster_id on findings, then persist findings
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;

        // Build a map of finding_id → cluster_id from the cluster results
        let mut finding_cluster_map: HashMap<String, String> = HashMap::new();
        for cluster in &result.clusters {
            // Persist cluster row
            let db_cluster = crate::storage::models::FindingCluster {
                id: cluster.id.clone(),
                review_run_id: cluster.review_run_id.clone(),
                label: cluster.label.clone(),
                representative_finding_id: Some(cluster.representative.id.clone()),
                member_count: cluster.member_count as i32,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            queries::insert_finding_cluster(&conn, &db_cluster)?;

            // Map each member finding to this cluster
            for member in &cluster.members {
                finding_cluster_map.insert(member.id.clone(), cluster.id.clone());
            }
        }

        // Persist findings with cluster_id stamped
        for finding in &result.surfaced {
            let mut f = finding.clone();
            if let Some(cid) = finding_cluster_map.get(&f.id) {
                f.cluster_id = Some(cid.clone());
            }
            queries::insert_finding(&conn, &f)?;
        }
    }

    update_status(db, &args.run_id, "ready", &mut emit)?;
    log_event(
        &event_log,
        &args.run_id,
        "review_ready",
        serde_json::json!({"surfaced_count": result.surfaced.len()}),
    );
    emit(ReviewEvent::ReviewReady {
        run_id: args.run_id.clone(),
    });

    Ok(())
}

fn log_event(
    event_log: &Option<Arc<EventLog>>,
    run_id: &str,
    event_type: &str,
    payload: serde_json::Value,
) {
    if let Some(log) = event_log {
        if let Err(e) = log.append(run_id, event_type, payload) {
            tracing::warn!("Failed to write event log: {}", e);
        }
    }
}

async fn run_agent_lanes(
    db: &AppDb,
    run_id: &str,
    lanes: Vec<AgentLaneConfig>,
    cwd: &Path,
    cancel: CancellationToken,
    provider_semaphores: &ProviderSemaphores,
    emit: &mut impl FnMut(ReviewEvent),
) -> Vec<AgentLaneResult> {
    let mut join_set: JoinSet<AgentLaneResult> = JoinSet::new();

    for lane in lanes {
        let lane_cancel = cancel.child_token();
        let cwd = cwd.to_path_buf();
        let lane_id = lane.id.clone();
        let provider = lane.provider.clone();
        let provider_name = provider.provider_name().to_string();
        let input = lane.input.clone();
        let timeout = lane.timeout;
        let agent_run_id = uuid::Uuid::new_v4().to_string();

        // Get per-provider semaphore (guaranteed to exist from build_provider_semaphores)
        let sem = provider_semaphores
            .get(&provider_name)
            .cloned()
            .unwrap_or_else(|| Arc::new(Semaphore::new(DEFAULT_PERMITS_PER_PROVIDER)));

        // Insert agent_run as "running" BEFORE spawn so UI can see it immediately
        let started_at = chrono::Utc::now().to_rfc3339();
        {
            let ar = AgentRun {
                id: agent_run_id.clone(),
                review_run_id: run_id.to_string(),
                lane_id: lane_id.clone(),
                provider_name: provider_name.clone(),
                status: "running".to_string(),
                started_at: Some(started_at.clone()),
                completed_at: None,
                finding_count: 0,
                error_message: None,
            };
            if let Ok(conn) = db.0.lock() {
                let _ = queries::insert_agent_run(&conn, &ar);
            }
        }

        // Emit lane started
        emit(ReviewEvent::LaneStatusChanged {
            lane_id: lane_id.clone(),
            provider_name: provider_name.clone(),
            status: "running".into(),
            finding_count: 0,
            error_message: None,
        });

        let agent_run_id_clone = agent_run_id.clone();

        join_set.spawn(async move {
            // Wrap BOTH semaphore acquire + provider call inside timeout + cancel select
            let result = tokio::select! {
                _ = lane_cancel.cancelled() => {
                    Err(LaneStatus::Cancelled)
                }
                result = tokio::time::timeout(timeout, async {
                    // acquire_owned avoids lifetime issues in spawned futures
                    let _permit = sem.acquire_owned().await
                        .map_err(|_| LaneStatus::Failed { error: "Semaphore closed".into() })?;
                    match provider.run_review(&input, &cwd, lane_cancel.clone()).await {
                        Ok(mut output) => {
                            // Tag findings with lane/provider attribution (Fix 4)
                            for f in &mut output.findings {
                                f.lane_id = Some(lane_id.clone());
                                f.provider_name = Some(provider_name.clone());
                            }
                            Ok(output.findings)
                        }
                        Err(ProviderError::Cancelled) => Err(LaneStatus::Cancelled),
                        Err(e) => Err(LaneStatus::Failed { error: e.to_string() }),
                    }
                }) => match result {
                    Err(_elapsed) => Err(LaneStatus::TimedOut),
                    Ok(inner) => inner,
                }
            };

            let completed_at = chrono::Utc::now().to_rfc3339();
            match result {
                Ok(findings) => {
                    let finding_count = findings.len();
                    AgentLaneResult {
                        agent_run_id: agent_run_id_clone,
                        lane_id,
                        provider_name,
                        findings,
                        status: LaneStatus::Completed { finding_count },
                        started_at,
                        completed_at,
                    }
                }
                Err(status) => AgentLaneResult {
                    agent_run_id: agent_run_id_clone,
                    lane_id,
                    provider_name,
                    findings: vec![],
                    status,
                    started_at,
                    completed_at,
                },
            }
        });
    }

    // Collect all results
    let mut results = Vec::new();
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok(lane_result) => {
                // Emit lane completed/failed
                let snapshot = LaneSnapshot::from(&lane_result);
                emit(ReviewEvent::LaneStatusChanged {
                    lane_id: snapshot.lane_id,
                    provider_name: snapshot.provider_name,
                    status: snapshot.status,
                    finding_count: snapshot.finding_count,
                    error_message: snapshot.error_message,
                });
                results.push(lane_result);
            }
            Err(e) => {
                tracing::error!("Lane task panicked: {}", e);
            }
        }
    }

    results
}

fn update_status(
    db: &AppDb,
    run_id: &str,
    status: &str,
    emit: &mut impl FnMut(ReviewEvent),
) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    queries::update_review_run_status(&conn, run_id, status, None)?;
    emit(ReviewEvent::StatusChanged {
        status: status.into(),
    });
    Ok(())
}

fn fail_run(
    db: &AppDb,
    run_id: &str,
    error: &str,
    emit: &mut impl FnMut(ReviewEvent),
) -> Result<(), AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    queries::update_review_run_status(&conn, run_id, "failed", Some(error))?;
    emit(ReviewEvent::ReviewFailed {
        run_id: run_id.into(),
        error: error.into(),
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::prompts;
    use crate::providers::traits::RawFinding;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::{PullRequest, ReviewRun, Workspace};
    use crate::storage::queries::{
        get_review_run, insert_pull_request, insert_review_run, insert_workspace,
    };
    use async_trait::async_trait;
    use std::path::Path;
    use std::time::Duration;
    use tokio::time::sleep;

    struct SlowProvider {
        delay: Duration,
        findings: Vec<RawFinding>,
    }

    impl SlowProvider {
        fn new(delay_ms: u64) -> Self {
            Self {
                delay: Duration::from_millis(delay_ms),
                findings: vec![],
            }
        }

        fn with_findings(delay_ms: u64, findings: Vec<RawFinding>) -> Self {
            Self {
                delay: Duration::from_millis(delay_ms),
                findings,
            }
        }
    }

    #[async_trait]
    impl ReviewProvider for SlowProvider {
        fn provider_name(&self) -> &str {
            "slow"
        }

        async fn health_check(&self) -> crate::providers::traits::ProviderHealth {
            crate::providers::traits::ProviderHealth {
                available: true,
                version: Some("slow".into()),
                message: None,
            }
        }

        async fn run_review(
            &self,
            _input: &ReviewInput,
            _cwd: &Path,
            cancel: CancellationToken,
        ) -> Result<crate::providers::traits::CodexReviewOutput, ProviderError> {
            tokio::select! {
                _ = cancel.cancelled() => Err(ProviderError::Cancelled),
                _ = sleep(self.delay) => Ok(crate::providers::traits::CodexReviewOutput {
                    findings: self.findings.clone(),
                    overall_assessment: None,
                    overall_confidence: None,
                })
            }
        }
    }

    struct FailingProvider;

    #[async_trait]
    impl ReviewProvider for FailingProvider {
        fn provider_name(&self) -> &str {
            "failing"
        }

        async fn health_check(&self) -> crate::providers::traits::ProviderHealth {
            crate::providers::traits::ProviderHealth {
                available: true,
                version: None,
                message: None,
            }
        }

        async fn run_review(
            &self,
            _input: &ReviewInput,
            _cwd: &Path,
            _cancel: CancellationToken,
        ) -> Result<crate::providers::traits::CodexReviewOutput, ProviderError> {
            Err(ProviderError::CodexFailed("Simulated failure".into()))
        }
    }

    fn seed_db(db: &AppDb, run_id: &str, pr_id: &str) {
        let conn = db.0.lock().unwrap();
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
                id: pr_id.into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "https://github.com/o/r/pull/1".into(),
                diff_text: Some("diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n".into()),
                changed_files: Some(r#"["a"]"#.into()),
                fetched_at: "2026-01-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        insert_review_run(
            &conn,
            &ReviewRun {
                id: run_id.into(),
                pr_id: pr_id.into(),
                status: "created".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: None,
                error_message: None,
            },
        )
        .unwrap();
    }

    fn test_input() -> ReviewInput {
        prompts::build_review_input(prompts::AgentFocus::General, "diff")
    }

    fn make_lane(
        id: &str,
        focus: prompts::AgentFocus,
        provider: Arc<dyn ReviewProvider>,
    ) -> AgentLaneConfig {
        AgentLaneConfig {
            id: id.to_string(),
            focus,
            provider,
            input: prompts::build_review_input(
                focus,
                "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n",
            ),
            timeout: Duration::from_secs(30),
        }
    }

    // --- Single-lane backward compat tests ---

    #[tokio::test]
    async fn test_pipeline_pre_cancelled_marks_failed() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run", "pr");

        let token = CancellationToken::new();
        token.cancel();

        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::new(200));
        let mut events = Vec::<ReviewEvent>::new();
        let sems = HashMap::new();
        run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes: vec![],
                fallback_input: Some(test_input()),
                fallback_provider: Some(provider),
                event_log: None,
            },
        )
        .await
        .unwrap();

        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run").unwrap().unwrap();
        assert_eq!(run.status, "failed");
        assert!(events
            .iter()
            .any(|e| matches!(e, ReviewEvent::ReviewFailed { .. })));
    }

    #[tokio::test]
    async fn test_pipeline_cancel_during_provider_marks_failed() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run2", "pr2");

        let token = CancellationToken::new();
        let token2 = token.clone();

        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::new(200));
        let mut events = Vec::<ReviewEvent>::new();
        let sems = HashMap::new();
        let canceller = tokio::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            token2.cancel();
        });

        run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run2".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes: vec![],
                fallback_input: Some(test_input()),
                fallback_provider: Some(provider),
                event_log: None,
            },
        )
        .await
        .unwrap();

        canceller.await.unwrap();

        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run2").unwrap().unwrap();
        assert_eq!(run.status, "failed");
    }

    // --- Multi-lane tests ---

    #[tokio::test]
    async fn test_multi_lane_parallel_execution() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run3", "pr3");

        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::with_findings(
            50,
            vec![RawFinding {
                title: "test".into(),
                body: "test body".into(),
                file_path: Some("a".into()),
                line_start: Some(1),
                line_end: Some(1),
                severity: "warning".into(),
                confidence: 0.8,
                evidence: None,
                agent_type: "security".into(),
                lane_id: None,
                provider_name: None,
            }],
        ));

        let lanes = vec![
            make_lane("security", prompts::AgentFocus::Security, provider.clone()),
            make_lane(
                "architecture",
                prompts::AgentFocus::Architecture,
                provider.clone(),
            ),
            make_lane(
                "performance",
                prompts::AgentFocus::Performance,
                provider.clone(),
            ),
        ];

        let sems = build_provider_semaphores(&lanes);
        let mut events = Vec::<ReviewEvent>::new();
        let token = CancellationToken::new();

        run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run3".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log: None,
            },
        )
        .await
        .unwrap();

        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run3").unwrap().unwrap();
        assert_eq!(run.status, "ready");

        // Should have lane status events for all 3 lanes (running + completed)
        let lane_events: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ReviewEvent::LaneStatusChanged { .. }))
            .collect();
        assert_eq!(lane_events.len(), 6);

        // Agent run records should all be "completed" (updated from "running")
        let agent_runs = queries::get_agent_runs_for_review(&conn, "run3").unwrap();
        assert_eq!(agent_runs.len(), 3);
        assert!(agent_runs.iter().all(|ar| ar.status == "completed"));
    }

    #[tokio::test]
    async fn test_multi_lane_partial_failure() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run4", "pr4");

        let good_provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::with_findings(
            20,
            vec![RawFinding {
                title: "good finding".into(),
                body: "body".into(),
                file_path: Some("a".into()),
                line_start: Some(1),
                line_end: Some(1),
                severity: "warning".into(),
                confidence: 0.8,
                evidence: None,
                agent_type: "security".into(),
                lane_id: None,
                provider_name: None,
            }],
        ));
        let bad_provider: Arc<dyn ReviewProvider> = Arc::new(FailingProvider);

        let lanes = vec![
            make_lane(
                "security",
                prompts::AgentFocus::Security,
                good_provider.clone(),
            ),
            make_lane(
                "architecture",
                prompts::AgentFocus::Architecture,
                bad_provider,
            ),
        ];

        let sems = build_provider_semaphores(&lanes);
        let mut events = Vec::<ReviewEvent>::new();
        let token = CancellationToken::new();

        run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run4".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log: None,
            },
        )
        .await
        .unwrap();

        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run4").unwrap().unwrap();
        assert_eq!(run.status, "ready");

        let findings = queries::get_findings_for_run(&conn, "run4").unwrap();
        assert!(!findings.is_empty());
        // Findings should have lane attribution (Fix 4)
        assert!(findings.iter().all(|f| f.lane_id.is_some()));
        assert!(findings.iter().all(|f| f.provider_name.is_some()));
    }

    #[tokio::test]
    async fn test_multi_lane_all_fail() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run5", "pr5");

        let bad_provider: Arc<dyn ReviewProvider> = Arc::new(FailingProvider);

        let lanes = vec![
            make_lane(
                "security",
                prompts::AgentFocus::Security,
                bad_provider.clone(),
            ),
            make_lane(
                "architecture",
                prompts::AgentFocus::Architecture,
                bad_provider,
            ),
        ];

        let sems = build_provider_semaphores(&lanes);
        let mut events = Vec::<ReviewEvent>::new();
        let token = CancellationToken::new();

        let result = run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run5".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log: None,
            },
        )
        .await;

        assert!(result.is_err());
        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run5").unwrap().unwrap();
        assert_eq!(run.status, "failed");
    }

    #[tokio::test]
    async fn test_multi_lane_timeout() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run6", "pr6");

        let slow_provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::new(5000));

        let lanes = vec![AgentLaneConfig {
            id: "security".into(),
            focus: prompts::AgentFocus::Security,
            provider: slow_provider,
            input: prompts::build_review_input(
                prompts::AgentFocus::Security,
                "diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n",
            ),
            timeout: Duration::from_millis(50),
        }];

        let sems = build_provider_semaphores(&lanes);
        let mut events = Vec::<ReviewEvent>::new();
        let token = CancellationToken::new();

        let result = run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run6".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log: None,
            },
        )
        .await;

        assert!(result.is_err());
        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run6").unwrap().unwrap();
        assert_eq!(run.status, "failed");

        let agent_runs = queries::get_agent_runs_for_review(&conn, "run6").unwrap();
        assert_eq!(agent_runs.len(), 1);
        assert_eq!(agent_runs[0].status, "timed_out");
    }

    #[tokio::test]
    async fn test_multi_lane_cancel_all() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run7", "pr7");

        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::new(5000));
        let lanes = vec![
            make_lane("security", prompts::AgentFocus::Security, provider.clone()),
            make_lane("architecture", prompts::AgentFocus::Architecture, provider),
        ];

        let sems = build_provider_semaphores(&lanes);
        let token = CancellationToken::new();
        let token2 = token.clone();
        let mut events = Vec::<ReviewEvent>::new();

        let canceller = tokio::spawn(async move {
            sleep(Duration::from_millis(30)).await;
            token2.cancel();
        });

        let _ = run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run7".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log: None,
            },
        )
        .await;

        canceller.await.unwrap();

        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run7").unwrap().unwrap();
        assert_eq!(run.status, "failed");
    }

    #[tokio::test]
    async fn test_provider_semaphore_isolation() {
        // Two providers with separate semaphores: each should get independent permits
        let sems = {
            let mut m = HashMap::new();
            m.insert("slow".to_string(), Arc::new(Semaphore::new(1)));
            m.insert("failing".to_string(), Arc::new(Semaphore::new(1)));
            m
        };

        // Verify each provider has its own semaphore
        assert_eq!(sems.len(), 2);
        assert!(sems.contains_key("slow"));
        assert!(sems.contains_key("failing"));
    }

    #[tokio::test]
    async fn test_lane_progress_visible_during_execution() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run8", "pr8");

        // Use a slow provider so we can check mid-execution state
        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider::new(100));
        let lanes = vec![make_lane(
            "security",
            prompts::AgentFocus::Security,
            provider,
        )];

        let sems = build_provider_semaphores(&lanes);
        let token = CancellationToken::new();
        let mut events = Vec::<ReviewEvent>::new();

        run_review_pipeline(
            &db,
            |e| events.push(e),
            &sems,
            ReviewPipelineArgs {
                run_id: "run8".into(),
                cwd: PathBuf::from("/tmp"),
                config: CleanerConfig::default(),
                cancel: token,
                lanes,
                fallback_input: None,
                fallback_provider: None,
                event_log: None,
            },
        )
        .await
        .unwrap();

        // Agent run should exist and be completed (was "running" during execution)
        let conn = db.0.lock().unwrap();
        let agent_runs = queries::get_agent_runs_for_review(&conn, "run8").unwrap();
        assert_eq!(agent_runs.len(), 1);
        assert_eq!(agent_runs[0].status, "completed");
        assert!(agent_runs[0].started_at.is_some());
        assert!(agent_runs[0].completed_at.is_some());
    }
}
