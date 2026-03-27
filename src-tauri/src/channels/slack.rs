use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::manager::ChannelManager;
use super::ChannelStatus;

/// Start a Slack listener that polls for PR review mentions.
/// Currently a stub -- will be upgraded to Socket Mode WebSocket in future.
#[allow(dead_code)]
pub async fn start_slack_listener(
    _token: String,
    manager: Arc<ChannelManager>,
    cancel: CancellationToken,
) {
    manager.update_status(ChannelStatus {
        source: "slack".into(),
        connected: true,
        message: Some("Polling mode (upgrade to Socket Mode pending)".into()),
    });

    // Placeholder polling loop - in production this would use Socket Mode WebSocket
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                // Future: poll Slack API or connect via Socket Mode
                tracing::debug!("Slack listener heartbeat");
            }
        }
    }

    manager.update_status(ChannelStatus {
        source: "slack".into(),
        connected: false,
        message: Some("Disconnected".into()),
    });
}

/// Extract GitHub PR URLs from text.
#[allow(dead_code)]
pub fn extract_pr_urls(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"https://github\.com/[\w.-]+/[\w.-]+/pull/\d+").unwrap();
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_single_url() {
        let text = "Please review https://github.com/owner/repo/pull/42 thanks!";
        let urls = extract_pr_urls(text);
        assert_eq!(urls, vec!["https://github.com/owner/repo/pull/42"]);
    }

    #[test]
    fn test_extract_multiple_urls() {
        let text = "Check https://github.com/a/b/pull/1 and https://github.com/c/d/pull/99";
        let urls = extract_pr_urls(text);
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://github.com/a/b/pull/1");
        assert_eq!(urls[1], "https://github.com/c/d/pull/99");
    }

    #[test]
    fn test_extract_no_urls() {
        let text = "No links here, just plain text.";
        let urls = extract_pr_urls(text);
        assert!(urls.is_empty());
    }

    #[test]
    fn test_extract_url_with_dots_and_hyphens() {
        let text = "https://github.com/my-org/my.repo-name/pull/123";
        let urls = extract_pr_urls(text);
        assert_eq!(
            urls,
            vec!["https://github.com/my-org/my.repo-name/pull/123"]
        );
    }

    #[test]
    fn test_extract_ignores_non_github_urls() {
        let text = "See https://gitlab.com/owner/repo/pull/1 for details";
        let urls = extract_pr_urls(text);
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_slack_listener_cancellation() {
        let manager = Arc::new(ChannelManager::new());
        let cancel = CancellationToken::new();

        let m = manager.clone();
        let c = cancel.clone();
        let handle = tokio::spawn(async move {
            start_slack_listener("fake-token".into(), m, c).await;
        });

        // Give the listener a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify it reports connected
        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].connected);

        // Cancel and wait for shutdown
        cancel.cancel();
        handle.await.unwrap();

        // Verify it reports disconnected
        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].connected);
    }
}
