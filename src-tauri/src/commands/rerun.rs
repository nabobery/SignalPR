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

fn extract_head_sha_from_metadata_json(json: &str) -> Option<String> {
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

struct LatestPrSnapshot {
    diff_text: String,
    diff_hash: String,
    changed_files: Vec<String>,
    changed_files_json: String,
    platform_metadata_json: Option<String>,
    platform_metadata_fetched_at: Option<String>,
    platform_capabilities_json: Option<String>,
    platform_capabilities_fetched_at: Option<String>,
    head_sha: Option<String>,
}

/// Start an incremental rerun linked to a baseline review run.
/// Fetches the latest diff from GitHub, creates a new PR snapshot and review run,
/// then spawns the review pipeline.
#[tauri::command]
pub async fn rerun_review(
    baseline_run_id: String,
    trigger_source: Option<String>,
    reason: Option<String>,
    app: AppHandle,
    db: tauri::State<'_, AppDb>,
) -> Result<String, AppError> {
    let trigger_source = trigger_source.unwrap_or_else(|| "workspace".to_string());
    let reason = reason.unwrap_or_else(|| "manual".to_string());
    let (pr, workspace_path, new_run_id) = {
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

        drop(conn);
        (pr, workspace_path, new_run_id)
    };

    let latest_snapshot = fetch_latest_pr_snapshot(&app, &pr).await?;
    let changed_files = latest_snapshot.changed_files.clone();

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
        super::review::build_agent_lanes(
            &latest_snapshot.diff_text,
            provider.clone(),
            &resolved,
            &conn,
        )
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
                new_run_id: &new_run_id,
                diff_text: &latest_snapshot.diff_text,
                diff_hash: &latest_snapshot.diff_hash,
                changed_files_json: &latest_snapshot.changed_files_json,
                now: &now,
                latest_platform_metadata_json: latest_snapshot.platform_metadata_json.as_deref(),
                latest_platform_metadata_fetched_at: latest_snapshot
                    .platform_metadata_fetched_at
                    .as_deref(),
                latest_platform_capabilities_json: latest_snapshot
                    .platform_capabilities_json
                    .as_deref(),
                latest_platform_capabilities_fetched_at: latest_snapshot
                    .platform_capabilities_fetched_at
                    .as_deref(),
                latest_head_sha: latest_snapshot.head_sha.as_deref(),
                trigger_source: &trigger_source,
                reason: &reason,
                scope: "full_pr",
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
    let pr_id_clone = pr.id.clone();
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
            super::review::resolve_issue_refs_and_codeowners(&app_clone, &db, &pr_id_clone).await;

        let context_pack = ContextPackBuilder::new(&context_pack_config, &cwd_path, &changed_files)
            .with_preferences(preference_text)
            .with_codeowners_content(base_branch_codeowners.clone(), codeowners_source.clone())
            .with_issues(issue_refs.clone())
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

async fn fetch_latest_pr_snapshot(
    app: &AppHandle,
    pr: &PullRequest,
) -> Result<LatestPrSnapshot, AppError> {
    let review_url = crate::platform::parse_review_url(&pr.url)?;
    let adapter = crate::platform::factory::build_adapter(app, &review_url).await?;

    let snapshot = adapter.fetch_review_snapshot().await?;
    let diff_text = snapshot.diff_text;
    if diff_text.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Empty diff returned from platform".into(),
        ));
    }

    let metadata = snapshot.metadata;
    let capabilities = snapshot.capabilities;
    let platform_metadata_json =
        serde_json::to_string(&metadata).map_err(|e| AppError::InvalidInput(e.to_string()))?;
    let platform_metadata_fetched_at = chrono::Utc::now().to_rfc3339();
    let platform_capabilities_json =
        serde_json::to_string(&capabilities).map_err(|e| AppError::InvalidInput(e.to_string()))?;
    let platform_capabilities_fetched_at = platform_metadata_fetched_at.clone();
    let diff_hash = sha256_hex(&diff_text);
    let changed_files = extract_changed_files_from_diff(&diff_text);
    let changed_files_json = serde_json::to_string(&changed_files).unwrap_or_default();
    let head_sha = extract_head_sha_from_metadata_json(&platform_metadata_json);

    Ok(LatestPrSnapshot {
        diff_text,
        diff_hash,
        changed_files,
        changed_files_json,
        platform_metadata_json: Some(platform_metadata_json),
        platform_metadata_fetched_at: Some(platform_metadata_fetched_at),
        platform_capabilities_json: Some(platform_capabilities_json),
        platform_capabilities_fetched_at: Some(platform_capabilities_fetched_at),
        head_sha,
    })
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
    new_run_id: &'a str,
    diff_text: &'a str,
    diff_hash: &'a str,
    changed_files_json: &'a str,
    now: &'a str,
    latest_platform_metadata_json: Option<&'a str>,
    latest_platform_metadata_fetched_at: Option<&'a str>,
    latest_platform_capabilities_json: Option<&'a str>,
    latest_platform_capabilities_fetched_at: Option<&'a str>,
    latest_head_sha: Option<&'a str>,
    trigger_source: &'a str,
    reason: &'a str,
    scope: &'a str,
}

fn create_rerun_records(
    conn: &rusqlite::Connection,
    input: &RerunRecordInput<'_>,
) -> Result<(), AppError> {
    let baseline_run = queries::get_review_run(conn, input.baseline_run_id)?
        .ok_or_else(|| AppError::InvalidInput("Baseline run not found".into()))?;

    if baseline_run.analysis_diff_text.is_none() {
        if let Some(diff_text) = input.baseline_pr.diff_text.as_deref() {
            let diff_hash = input
                .baseline_pr
                .diff_hash
                .clone()
                .unwrap_or_else(|| sha256_hex(diff_text));
            queries::update_review_run_analysis_diff(
                conn,
                input.baseline_run_id,
                diff_text,
                &diff_hash,
            )?;
        }
    }

    if baseline_run.head_sha_at_run.is_none() {
        if let Some(head_sha) = input
            .baseline_pr
            .platform_metadata_json
            .as_deref()
            .and_then(extract_head_sha_from_metadata_json)
        {
            queries::update_review_run_head_sha_at_run(conn, input.baseline_run_id, &head_sha)?;
        }
    }

    queries::update_pull_request_snapshot(
        conn,
        &input.baseline_pr.id,
        &queries::PullRequestSnapshotUpdate {
            diff_text: input.diff_text,
            changed_files_json: input.changed_files_json,
            diff_hash: input.diff_hash,
            fetched_at_rfc3339: input.now,
            metadata_json: input.latest_platform_metadata_json,
            metadata_fetched_at_rfc3339: input.latest_platform_metadata_fetched_at,
            capabilities_json: input.latest_platform_capabilities_json,
            capabilities_fetched_at_rfc3339: input.latest_platform_capabilities_fetched_at,
        },
    )?;

    queries::insert_review_run(
        conn,
        &ReviewRun {
            id: input.new_run_id.to_string(),
            pr_id: input.baseline_pr.id.clone(),
            status: "created".into(),
            started_at: Some(input.now.to_string()),
            completed_at: None,
            error_message: None,
            head_sha_at_run: input.latest_head_sha.map(ToString::to_string).or_else(|| {
                input
                    .latest_platform_metadata_json
                    .and_then(extract_head_sha_from_metadata_json)
            }),
            baseline_run_id: Some(input.baseline_run_id.to_string()),
            metrics_json: None,
            analysis_diff_hash: Some(input.diff_hash.to_string()),
            analysis_diff_text: Some(input.diff_text.to_string()),
            context_pack_json: None,
            local_checks_json: None,
            rerun_trigger_source: Some(input.trigger_source.to_string()),
            rerun_reason: Some(input.reason.to_string()),
            rerun_scope: Some(input.scope.to_string()),
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
            platform_capabilities_json: None,
            platform_capabilities_fetched_at: None,
        };
        queries::insert_pull_request(conn, &baseline_pr).unwrap();

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
            analysis_diff_hash: Some("basehash".into()),
            analysis_diff_text: Some("diff --git a/src/a.rs b/src/a.rs".into()),
            context_pack_json: None,
            local_checks_json: None,
            rerun_trigger_source: None,
            rerun_reason: None,
            rerun_scope: None,
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
                new_run_id: "run-rerun",
                diff_text: "diff --git a/src/c.rs b/src/c.rs",
                diff_hash: "newhash",
                changed_files_json: "[\"src/c.rs\"]",
                now: "2026-01-02T00:00:00Z",
                latest_platform_metadata_json: None,
                latest_platform_metadata_fetched_at: None,
                latest_platform_capabilities_json: None,
                latest_platform_capabilities_fetched_at: None,
                latest_head_sha: None,
                trigger_source: "workspace",
                reason: "manual",
                scope: "full_pr",
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
        assert_eq!(rerun.pr_id, baseline_pr.id);

        let rerun_pr = queries::get_pull_request(&conn, &baseline_pr.id)
            .unwrap()
            .expect("rerun PR row should exist");
        assert_eq!(rerun_pr.diff_hash.as_deref(), Some("newhash"));
        assert_eq!(rerun_pr.changed_files.as_deref(), Some("[\"src/c.rs\"]"));

        let pr_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM pull_requests", [], |row| row.get(0))
            .expect("count pull requests");
        assert_eq!(pr_count, 1);
    }

    #[test]
    fn create_rerun_records_persists_rerun_metadata() {
        let db = init_db_in_memory().expect("in-memory DB");
        let conn = db.0.into_inner().expect("owned connection");
        let (baseline_pr, baseline_run) = setup_baseline(&conn);

        create_rerun_records(
            &conn,
            &RerunRecordInput {
                baseline_run_id: &baseline_run.id,
                baseline_pr: &baseline_pr,
                new_run_id: "run-rerun-meta",
                diff_text: "diff --git a/src/c.rs b/src/c.rs",
                diff_hash: "newhash",
                changed_files_json: "[\"src/c.rs\"]",
                now: "2026-01-02T00:00:00Z",
                latest_platform_metadata_json: None,
                latest_platform_metadata_fetched_at: None,
                latest_platform_capabilities_json: None,
                latest_platform_capabilities_fetched_at: None,
                latest_head_sha: None,
                trigger_source: "workspace",
                reason: "manual",
                scope: "full_pr",
            },
        )
        .expect("record creation should succeed");

        let rerun = queries::get_review_run(&conn, "run-rerun-meta")
            .unwrap()
            .expect("rerun row should exist");
        assert_eq!(rerun.rerun_trigger_source.as_deref(), Some("workspace"));
        assert_eq!(rerun.rerun_reason.as_deref(), Some("manual"));
        assert_eq!(rerun.rerun_scope.as_deref(), Some("full_pr"));
    }

    #[test]
    fn create_rerun_records_updates_platform_snapshot_and_head_sha() {
        let db = init_db_in_memory().expect("in-memory DB");
        let conn = db.0.into_inner().expect("owned connection");
        let (mut baseline_pr, baseline_run) = setup_baseline(&conn);
        baseline_pr.platform_metadata_json = Some(
            r#"{"platform":"github","pr_body":null,"head_sha":"sha-old","base_sha":"sha-base","base_ref":"main","head_ref":"feature","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#
                .into(),
        );
        queries::update_pull_request_metadata(
            &conn,
            &baseline_pr.id,
            baseline_pr.platform_metadata_json.as_deref().unwrap(),
            "2026-01-01T00:00:00Z",
        )
        .unwrap();

        let latest_metadata = r#"{"platform":"github","pr_body":null,"head_sha":"sha-new","base_sha":"sha-base","base_ref":"main","head_ref":"feature","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#;

        create_rerun_records(
            &conn,
            &RerunRecordInput {
                baseline_run_id: &baseline_run.id,
                baseline_pr: &baseline_pr,
                new_run_id: "run-rerun-meta-head",
                diff_text: "diff --git a/src/c.rs b/src/c.rs",
                diff_hash: "newhash",
                changed_files_json: "[\"src/c.rs\"]",
                now: "2026-01-02T00:00:00Z",
                latest_platform_metadata_json: Some(latest_metadata),
                latest_platform_metadata_fetched_at: Some("2026-01-02T00:00:00Z"),
                latest_platform_capabilities_json: None,
                latest_platform_capabilities_fetched_at: None,
                latest_head_sha: Some("sha-new"),
                trigger_source: "workspace",
                reason: "manual",
                scope: "full_pr",
            },
        )
        .expect("record creation should succeed");

        let rerun = queries::get_review_run(&conn, "run-rerun-meta-head")
            .unwrap()
            .expect("rerun row should exist");
        assert_eq!(rerun.pr_id, baseline_pr.id);
        assert_eq!(rerun.head_sha_at_run.as_deref(), Some("sha-new"));

        let updated_pr = queries::get_pull_request(&conn, &baseline_pr.id)
            .unwrap()
            .expect("baseline pr should exist");
        assert_eq!(
            updated_pr.platform_metadata_json.as_deref(),
            Some(latest_metadata)
        );
        assert_eq!(
            updated_pr.platform_metadata_fetched_at.as_deref(),
            Some("2026-01-02T00:00:00Z")
        );
    }
}
