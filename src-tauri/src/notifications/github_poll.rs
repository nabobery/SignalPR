use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::storage::db::AppDb;
use crate::storage::queries;

/// Background poller for GitHub review request notifications.
/// Opt-in: only starts if `github_polling_enabled` setting is "true".
///
/// Note: `seen_ids` is in-memory only — app restart may re-notify for
/// still-unread notifications. This is acceptable for Phase 2 since
/// notifications are idempotent.
///
/// ETag support deferred to Phase 3 when we switch to reqwest for GitHub API.
pub struct GitHubPoller {
    app: AppHandle,
    interval: Duration,
    cancel: CancellationToken,
    seen_ids: Arc<Mutex<HashSet<String>>>,
}

#[derive(Deserialize)]
struct GhNotification {
    id: String,
    reason: String,
    subject: GhSubject,
}

#[derive(Deserialize)]
struct GhSubject {
    title: String,
    url: Option<String>,
    #[serde(rename = "type")]
    subject_type: String,
}

impl GitHubPoller {
    pub fn new(app: AppHandle, cancel: CancellationToken) -> Self {
        Self {
            app,
            interval: Duration::from_secs(300), // 5 minutes default
            cancel,
            seen_ids: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    /// Start the polling loop. Polls immediately on start, then every interval.
    pub async fn run(&self) {
        tracing::info!(
            "GitHub notification poller started (interval: {:?})",
            self.interval
        );

        // Poll immediately on start (don't wait for first interval)
        if let Err(e) = self.poll_once().await {
            tracing::warn!("Initial GitHub poll failed: {}", e);
        }

        loop {
            tokio::select! {
                _ = self.cancel.cancelled() => {
                    tracing::info!("GitHub notification poller stopped");
                    return;
                }
                _ = tokio::time::sleep(self.interval) => {
                    if let Err(e) = self.poll_once().await {
                        tracing::warn!("GitHub poll failed: {}", e);
                    }
                }
            }
        }
    }

    async fn poll_once(&self) -> Result<(), String> {
        let shell = self.app.shell();

        let args = vec![
            "api".to_string(),
            "notifications".to_string(),
            "--jq".to_string(),
            r#"[.[] | select(.reason == "review_requested" and .subject.type == "PullRequest")]"#
                .to_string(),
        ];

        let output = shell
            .command("gh")
            .args(&args)
            .output()
            .await
            .map_err(|e| format!("gh command failed: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("gh api failed: {}", stderr));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.trim().is_empty() || stdout.trim() == "[]" {
            return Ok(());
        }

        let notifications: Vec<GhNotification> =
            serde_json::from_str(&stdout).map_err(|e| format!("Parse notifications: {}", e))?;

        let mut seen = self.seen_ids.lock().await;

        for notif in &notifications {
            if notif.reason != "review_requested" {
                continue;
            }
            if seen.contains(&notif.id) {
                continue;
            }
            seen.insert(notif.id.clone());

            tracing::info!(
                "New review request: {} ({})",
                notif.subject.title,
                notif.subject.subject_type
            );

            let _ = self.app.emit(
                "github_review_requested",
                serde_json::json!({
                    "title": notif.subject.title,
                    "url": notif.subject.url,
                }),
            );
        }

        Ok(())
    }
}

/// Check if polling is enabled and start the poller if so.
pub fn maybe_start_poller(app: &AppHandle, cancel: CancellationToken) {
    let db = app.state::<AppDb>();
    let enabled = {
        let conn = match db.0.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        queries::get_setting(&conn, "github_polling_enabled")
            .unwrap_or(None)
            .as_deref()
            == Some("true")
    };

    if !enabled {
        tracing::info!("GitHub polling disabled (set github_polling_enabled=true to enable)");
        return;
    }

    let poller = GitHubPoller::new(app.clone(), cancel);
    tauri::async_runtime::spawn(async move {
        poller.run().await;
    });
}
