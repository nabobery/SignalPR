use tokio::sync::broadcast;

use super::{ChannelEvent, ChannelStatus};

pub struct ChannelManager {
    sender: broadcast::Sender<ChannelEvent>,
    statuses: std::sync::Mutex<Vec<ChannelStatus>>,
}

impl ChannelManager {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(32);
        Self {
            sender,
            statuses: std::sync::Mutex::new(vec![]),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChannelEvent> {
        self.sender.subscribe()
    }

    #[allow(dead_code)]
    pub fn emit(&self, event: ChannelEvent) {
        let _ = self.sender.send(event);
    }

    pub fn update_status(&self, status: ChannelStatus) {
        let mut statuses = self.statuses.lock().unwrap();
        statuses.retain(|s| s.source != status.source);
        statuses.push(status);
    }

    pub fn get_statuses(&self) -> Vec<ChannelStatus> {
        self.statuses.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_subscribe_round_trip() {
        let manager = ChannelManager::new();
        let mut rx = manager.subscribe();

        let event = ChannelEvent {
            source: "slack".into(),
            pr_url: "https://github.com/owner/repo/pull/42".into(),
            requester: Some("alice".into()),
            channel: Some("#reviews".into()),
            received_at: "2026-03-27T00:00:00Z".into(),
        };

        manager.emit(event.clone());

        let received = rx.try_recv().expect("should receive event");
        assert_eq!(received.source, "slack");
        assert_eq!(received.pr_url, "https://github.com/owner/repo/pull/42");
        assert_eq!(received.requester, Some("alice".into()));
        assert_eq!(received.channel, Some("#reviews".into()));
    }

    #[test]
    fn test_emit_no_subscribers_does_not_panic() {
        let manager = ChannelManager::new();
        let event = ChannelEvent {
            source: "discord".into(),
            pr_url: "https://github.com/owner/repo/pull/1".into(),
            requester: None,
            channel: None,
            received_at: "2026-03-27T00:00:00Z".into(),
        };
        // Should not panic even with no active subscribers
        manager.emit(event);
    }

    #[test]
    fn test_status_tracking() {
        let manager = ChannelManager::new();

        assert!(manager.get_statuses().is_empty());

        manager.update_status(ChannelStatus {
            source: "slack".into(),
            connected: true,
            message: Some("Connected".into()),
        });

        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].source, "slack");
        assert!(statuses[0].connected);
    }

    #[test]
    fn test_status_replaces_existing_source() {
        let manager = ChannelManager::new();

        manager.update_status(ChannelStatus {
            source: "slack".into(),
            connected: true,
            message: Some("Connected".into()),
        });

        manager.update_status(ChannelStatus {
            source: "slack".into(),
            connected: false,
            message: Some("Disconnected".into()),
        });

        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 1);
        assert!(!statuses[0].connected);
        assert_eq!(statuses[0].message, Some("Disconnected".into()));
    }

    #[test]
    fn test_status_multiple_sources() {
        let manager = ChannelManager::new();

        manager.update_status(ChannelStatus {
            source: "slack".into(),
            connected: true,
            message: None,
        });

        manager.update_status(ChannelStatus {
            source: "discord".into(),
            connected: true,
            message: None,
        });

        let statuses = manager.get_statuses();
        assert_eq!(statuses.len(), 2);
    }
}
