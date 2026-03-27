use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use super::manager::ChannelManager;
use super::ChannelStatus;

/// Start a Discord listener that polls for PR review mentions.
/// Currently a stub -- will be upgraded to Gateway WebSocket in future.
#[allow(dead_code)]
pub async fn start_discord_listener(
    _token: String,
    manager: Arc<ChannelManager>,
    cancel: CancellationToken,
) {
    manager.update_status(ChannelStatus {
        source: "discord".into(),
        connected: true,
        message: Some("Polling mode (upgrade to Gateway pending)".into()),
    });

    // Placeholder polling loop - in production this would use Discord Gateway WebSocket
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {
                // Future: poll Discord API or connect via Gateway
                tracing::debug!("Discord listener heartbeat");
            }
        }
    }

    manager.update_status(ChannelStatus {
        source: "discord".into(),
        connected: false,
        message: Some("Disconnected".into()),
    });
}

/// Extract GitHub PR URLs from text (same logic as Slack).
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
    }

    #[test]
    fn test_extract_no_urls() {
        let text = "No links here.";
        let urls = extract_pr_urls(text);
        assert!(urls.is_empty());
    }

    #[tokio::test]
    async fn test_discord_listener_cancellation() {
        let manager = Arc::new(ChannelManager::new());
        let cancel = CancellationToken::new();

        let m = manager.clone();
        let c = cancel.clone();
        let handle = tokio::spawn(async move {
            start_discord_listener("fake-token".into(), m, c).await;
        });

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].connected);
        assert_eq!(statuses[0].source, "discord");

        cancel.cancel();
        handle.await.unwrap();

        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].connected);
    }
}
