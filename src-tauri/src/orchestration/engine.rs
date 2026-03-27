use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use tokio_util::sync::CancellationToken;

use crate::cleaner::{self, CleanerConfig};
use crate::errors::AppError;
use crate::providers::traits::ReviewProvider;
use crate::storage::db::AppDb;
use crate::storage::queries;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ReviewEvent {
    StatusChanged { status: String },
    ReviewReady { run_id: String },
    ReviewFailed { run_id: String, error: String },
}

pub struct ReviewPipelineArgs<'a> {
    pub run_id: &'a str,
    pub diff: &'a str,
    pub cwd: &'a Path,
    pub config: &'a CleanerConfig,
    pub cancel: CancellationToken,
}

pub async fn run_review_pipeline(
    db: &AppDb,
    provider: Arc<dyn ReviewProvider>,
    mut emit: impl FnMut(ReviewEvent) + Send,
    args: ReviewPipelineArgs<'_>,
) -> Result<(), AppError> {
    // Stage 1: Running agents
    if args.cancel.is_cancelled() {
        fail_run(db, args.run_id, "Cancelled by user", &mut emit)?;
        return Ok(());
    }
    update_status(db, args.run_id, "running_agents", &mut emit)?;

    let raw_output = match provider
        .run_review(args.diff, args.cwd, args.cancel.clone())
        .await
    {
        Ok(output) => output,
        Err(e) => {
            if matches!(e, crate::errors::ProviderError::Cancelled) {
                fail_run(db, args.run_id, "Cancelled by user", &mut emit)?;
                return Ok(());
            }
            let err_msg = e.to_string();
            fail_run(db, args.run_id, &err_msg, &mut emit)?;
            return Err(AppError::Provider(e));
        }
    };

    if args.cancel.is_cancelled() {
        fail_run(db, args.run_id, "Cancelled by user", &mut emit)?;
        return Ok(());
    }

    // Stage 2: Cleaner pipeline
    update_status(db, args.run_id, "cleaning", &mut emit)?;
    let result = cleaner::clean(raw_output.findings, args.diff, args.run_id, args.config);

    if args.cancel.is_cancelled() {
        fail_run(db, args.run_id, "Cancelled by user", &mut emit)?;
        return Ok(());
    }

    // Stage 3: Persist surfaced findings + mark ready
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        for finding in &result.surfaced {
            queries::insert_finding(&conn, finding)?;
        }
    }

    update_status(db, args.run_id, "ready", &mut emit)?;
    emit(ReviewEvent::ReviewReady {
        run_id: args.run_id.to_string(),
    });

    Ok(())
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
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::{PullRequest, ReviewRun, Workspace};
    use crate::storage::queries::{
        get_review_run, insert_pull_request, insert_review_run, insert_workspace,
    };
    use async_trait::async_trait;
    use std::path::Path;
    use tokio::time::{sleep, Duration};

    struct SlowProvider;

    #[async_trait]
    impl ReviewProvider for SlowProvider {
        async fn health_check(&self) -> crate::providers::traits::ProviderHealth {
            crate::providers::traits::ProviderHealth {
                available: true,
                version: Some("slow".into()),
                message: None,
            }
        }

        async fn run_review(
            &self,
            _diff: &str,
            _cwd: &Path,
            cancel: CancellationToken,
        ) -> Result<crate::providers::traits::CodexReviewOutput, crate::errors::ProviderError>
        {
            tokio::select! {
                _ = cancel.cancelled() => Err(crate::errors::ProviderError::Cancelled),
                _ = sleep(Duration::from_millis(200)) => Ok(crate::providers::traits::CodexReviewOutput {
                    findings: vec![],
                    overall_assessment: None,
                    overall_confidence: None,
                })
            }
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

    #[tokio::test]
    async fn test_pipeline_pre_cancelled_marks_failed() {
        let db = init_db_in_memory().unwrap();
        seed_db(&db, "run", "pr");

        let token = CancellationToken::new();
        token.cancel();

        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider);
        let mut events = Vec::<ReviewEvent>::new();
        let config = CleanerConfig::default();
        run_review_pipeline(
            &db,
            provider,
            |e| events.push(e),
            ReviewPipelineArgs {
                run_id: "run",
                diff: "diff",
                cwd: Path::new("/tmp"),
                config: &config,
                cancel: token,
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

        let provider: Arc<dyn ReviewProvider> = Arc::new(SlowProvider);
        let mut events = Vec::<ReviewEvent>::new();
        let config = CleanerConfig::default();
        let canceller = tokio::spawn(async move {
            sleep(Duration::from_millis(50)).await;
            token2.cancel();
        });

        run_review_pipeline(
            &db,
            provider,
            |e| events.push(e),
            ReviewPipelineArgs {
                run_id: "run2",
                diff: "diff",
                cwd: Path::new("/tmp"),
                config: &config,
                cancel: token,
            },
        )
        .await
        .unwrap();

        canceller.await.unwrap();

        let conn = db.0.lock().unwrap();
        let run = get_review_run(&conn, "run2").unwrap().unwrap();
        assert_eq!(run.status, "failed");
        assert!(events
            .iter()
            .any(|e| matches!(e, ReviewEvent::ReviewFailed { .. })));
    }
}
