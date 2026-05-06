use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager};
use tokio_util::sync::CancellationToken;

use crate::config;
use crate::context_pack::ContextPackBuilder;
use crate::errors::AppError;
use crate::local_checks::{self, LocalChecksRunner};
use crate::orchestration::engine;
use crate::preferences::scoring;
use crate::providers::traits::{RawFinding, ReviewProvider};
use crate::storage::db::AppDb;
use crate::storage::hashing::sha256_hex;
use crate::storage::models::{PullRequest, ReviewRun};
use crate::storage::queries;

use super::review::ActiveReviews;

/// Start an incremental rerun linked to a baseline review run.
/// Fetches the latest diff from GitHub, creates a new PR snapshot and review run,
/// then spawns the review pipeline.
#[tauri::command]
pub async fn rerun_review(
    baseline_run_id: String,
    app: AppHandle,
    db: tauri::State<'_, AppDb>,
) -> Result<String, AppError> {
    let (pr, workspace_path, new_run_id, new_pr_id) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;

        let baseline_run = queries::get_review_run(&conn, &baseline_run_id)?
            .ok_or_else(|| AppError::InvalidInput("Baseline run not found".into()))?;

        if baseline_run.status != "ready" && baseline_run.status != "submitted" {
            return Err(AppError::InvalidInput(
                "Can only rerun from a completed (ready/submitted) review".into(),
            ));
        }

        let pr = queries::get_pull_request(&conn, &baseline_run.pr_id)?
            .ok_or_else(|| AppError::InvalidInput("PR not found for baseline run".into()))?;

        let workspace_path: String = conn
            .query_row(
                "SELECT local_path FROM workspaces WHERE id = ?1",
                rusqlite::params![pr.workspace_id],
                |row| row.get(0),
            )
            .map_err(|_| AppError::InvalidInput("Workspace not found".into()))?;

        let new_run_id = uuid::Uuid::new_v4().to_string();
        let new_pr_id = uuid::Uuid::new_v4().to_string();

        drop(conn);
        (pr, workspace_path, new_run_id, new_pr_id)
    };

    // Fetch latest diff via gh CLI
    let diff_text = fetch_latest_diff(&workspace_path, pr.pr_number, &app).await?;
    let diff_hash = sha256_hex(&diff_text);

    let changed_files = extract_changed_files_from_diff(&diff_text);
    let changed_files_json = serde_json::to_string(&changed_files).unwrap_or_default();

    // Load config from workspace path
    let cwd_path = PathBuf::from(&workspace_path);
    let repo_config = config::load_repo_config(&cwd_path);
    let resolved = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        config::resolve_config(&conn, repo_config.as_ref(), Some(cwd_path.as_path()))
    };

    let provider: Arc<dyn ReviewProvider> =
        config::select_provider(&app, &resolved.preferred_provider).await;

    let lanes = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        super::review::build_agent_lanes(&diff_text, provider.clone(), &resolved, &conn)
    };
    let cleaner_config = resolved.cleaner;
    let context_pack_config = resolved.context_pack;
    let local_checks_config = resolved.local_checks;

    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        create_rerun_records(
            &conn,
            &RerunRecordInput {
                baseline_run_id: &baseline_run_id,
                baseline_pr: &pr,
                new_pr_id: &new_pr_id,
                new_run_id: &new_run_id,
                diff_text: &diff_text,
                diff_hash: &diff_hash,
                changed_files_json: &changed_files_json,
                now: &now,
            },
        )?;
    }

    // Register cancellation token
    let token = CancellationToken::new();
    {
        let active = app.state::<ActiveReviews>();
        let mut map = active
            .0
            .lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        map.insert(new_run_id.clone(), token.clone());
    }

    // Spawn pipeline
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

        let owners_by_path = {
            let raw = crate::context_pack::read_local_codeowners(&cwd_path);
            match raw {
                Some(content) => crate::context_pack::resolve_codeowners(&content, &changed_files)
                    .into_iter()
                    .collect::<std::collections::HashMap<_, _>>(),
                None => std::collections::HashMap::new(),
            }
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
            },
        )
        .await;

        // Cleanup active token
        let active = app_clone.state::<ActiveReviews>();
        if let Ok(mut map) = active.0.lock() {
            map.remove(&run_id_clone);
        }

        if let Err(e) = result {
            tracing::error!("Rerun pipeline failed for {}: {:?}", run_id_clone, e);
        }
    });

    Ok(new_run_id)
}

async fn fetch_latest_diff(
    workspace_path: &str,
    pr_number: i32,
    app: &AppHandle,
) -> Result<String, AppError> {
    use tauri_plugin_shell::ShellExt;

    let output = app
        .shell()
        .command("gh")
        .current_dir(workspace_path)
        .args(["pr", "diff", &pr_number.to_string(), "--color=never"])
        .output()
        .await
        .map_err(|e| AppError::Transient(format!("Failed to run gh: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Transient(format!(
            "gh pr diff failed: {}",
            stderr.trim()
        )));
    }

    let diff = String::from_utf8_lossy(&output.stdout).to_string();
    if diff.trim().is_empty() {
        return Err(AppError::InvalidInput("Empty diff returned from gh".into()));
    }

    Ok(diff)
}

fn extract_changed_files_from_diff(diff: &str) -> Vec<String> {
    let mut files = BTreeSet::new();
    for line in diff.lines() {
        if line.starts_with("diff --git ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                let b_path = parts[3];
                let file = b_path.strip_prefix("b/").unwrap_or(b_path);
                files.insert(file.to_string());
            }
        }
    }
    files.into_iter().collect()
}

struct RerunRecordInput<'a> {
    baseline_run_id: &'a str,
    baseline_pr: &'a PullRequest,
    new_pr_id: &'a str,
    new_run_id: &'a str,
    diff_text: &'a str,
    diff_hash: &'a str,
    changed_files_json: &'a str,
    now: &'a str,
}

fn create_rerun_records(
    conn: &rusqlite::Connection,
    input: &RerunRecordInput<'_>,
) -> Result<(), AppError> {
    queries::insert_pull_request(
        conn,
        &PullRequest {
            id: input.new_pr_id.to_string(),
            workspace_id: input.baseline_pr.workspace_id.clone(),
            pr_number: input.baseline_pr.pr_number,
            title: input.baseline_pr.title.clone(),
            author: input.baseline_pr.author.clone(),
            base_branch: input.baseline_pr.base_branch.clone(),
            head_branch: input.baseline_pr.head_branch.clone(),
            url: input.baseline_pr.url.clone(),
            diff_text: Some(input.diff_text.to_string()),
            changed_files: Some(input.changed_files_json.to_string()),
            fetched_at: input.now.to_string(),
            diff_hash: Some(input.diff_hash.to_string()),
            platform_metadata_json: input.baseline_pr.platform_metadata_json.clone(),
            platform_metadata_fetched_at: input.baseline_pr.platform_metadata_fetched_at.clone(),
        },
    )?;

    queries::insert_review_run(
        conn,
        &ReviewRun {
            id: input.new_run_id.to_string(),
            pr_id: input.new_pr_id.to_string(),
            status: "created".into(),
            started_at: Some(input.now.to_string()),
            completed_at: None,
            error_message: None,
            baseline_run_id: Some(input.baseline_run_id.to_string()),
            metrics_json: None,
            analysis_diff_hash: Some(input.diff_hash.to_string()),
            analysis_diff_text: Some(input.diff_text.to_string()),
            context_pack_json: None,
            local_checks_json: None,
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::{ReviewRun, Workspace};

    fn setup_baseline(conn: &rusqlite::Connection) -> (PullRequest, ReviewRun) {
        let workspace = Workspace {
            id: "ws-1".into(),
            local_path: "/tmp/repo".into(),
            remote_owner: "o".into(),
            remote_repo: "r".into(),
            created_at: "2026-01-01T00:00:00Z".into(),
            remote_host: "github.com".into(),
        };
        queries::insert_workspace(conn, &workspace).unwrap();

        let baseline_pr = PullRequest {
            id: "pr-base".into(),
            workspace_id: workspace.id,
            pr_number: 42,
            title: "Baseline".into(),
            author: Some("octocat".into()),
            base_branch: Some("main".into()),
            head_branch: Some("feature".into()),
            url: "https://github.com/o/r/pull/42".into(),
            diff_text: Some("diff --git a/src/a.rs b/src/a.rs".into()),
            changed_files: Some("[\"src/a.rs\"]".into()),
            fetched_at: "2026-01-01T00:00:00Z".into(),
            diff_hash: Some("basehash".into()),
            platform_metadata_json: None,
            platform_metadata_fetched_at: None,
        };
        queries::insert_pull_request(conn, &baseline_pr).unwrap();

        let baseline_run = ReviewRun {
            id: "run-base".into(),
            pr_id: baseline_pr.id.clone(),
            status: "ready".into(),
            started_at: Some("2026-01-01T00:00:00Z".into()),
            completed_at: Some("2026-01-01T00:01:00Z".into()),
            error_message: None,
            baseline_run_id: None,
            metrics_json: None,
            analysis_diff_hash: Some("basehash".into()),
            analysis_diff_text: Some("diff --git a/src/a.rs b/src/a.rs".into()),
            context_pack_json: None,
            local_checks_json: None,
        };
        queries::insert_review_run(conn, &baseline_run).unwrap();

        (baseline_pr, baseline_run)
    }

    #[test]
    fn extract_changed_files_from_diff_dedups_and_sorts() {
        let diff = "\
diff --git a/src/b.rs b/src/b.rs
index 111..222 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -1 +1 @@
-old
+new
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1 +1 @@
-old
+new
diff --git a/src/b.rs b/src/b.rs
index 222..333 100644
--- a/src/b.rs
+++ b/src/b.rs
@@ -2 +2 @@
-x
+y
";
        assert_eq!(
            extract_changed_files_from_diff(diff),
            vec!["src/a.rs".to_string(), "src/b.rs".to_string()]
        );
    }

    #[test]
    fn create_rerun_records_links_baseline_and_diff_fields() {
        let db = init_db_in_memory().expect("in-memory DB");
        let conn = db.0.into_inner().expect("owned connection");
        let (baseline_pr, baseline_run) = setup_baseline(&conn);

        create_rerun_records(
            &conn,
            &RerunRecordInput {
                baseline_run_id: &baseline_run.id,
                baseline_pr: &baseline_pr,
                new_pr_id: "pr-rerun",
                new_run_id: "run-rerun",
                diff_text: "diff --git a/src/c.rs b/src/c.rs",
                diff_hash: "newhash",
                changed_files_json: "[\"src/c.rs\"]",
                now: "2026-01-02T00:00:00Z",
            },
        )
        .expect("record creation should succeed");

        let rerun = queries::get_review_run(&conn, "run-rerun")
            .unwrap()
            .expect("rerun row should exist");
        assert_eq!(rerun.baseline_run_id.as_deref(), Some("run-base"));
        assert_eq!(rerun.analysis_diff_hash.as_deref(), Some("newhash"));
        assert_eq!(
            rerun.analysis_diff_text.as_deref(),
            Some("diff --git a/src/c.rs b/src/c.rs")
        );

        let rerun_pr = queries::get_pull_request(&conn, "pr-rerun")
            .unwrap()
            .expect("rerun PR row should exist");
        assert_eq!(rerun_pr.diff_hash.as_deref(), Some("newhash"));
        assert_eq!(rerun_pr.changed_files.as_deref(), Some("[\"src/c.rs\"]"));
    }
}
