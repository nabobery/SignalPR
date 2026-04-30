use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;

use crate::providers::prompts::AgentFocus;
use crate::providers::traits::{ReviewInput, ReviewProvider};

/// Configuration for a single agent lane in a multi-lane review.
#[allow(dead_code)]
pub struct AgentLaneConfig {
    pub id: String,
    pub focus: AgentFocus,
    pub provider: Arc<dyn ReviewProvider>,
    pub input: ReviewInput,
    pub timeout: Duration,
}

/// Status of an individual agent lane during execution.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status")]
#[allow(dead_code)]
pub enum LaneStatus {
    Pending,
    Running,
    Completed { finding_count: usize },
    Failed { error: String },
    TimedOut,
    Cancelled,
}

impl LaneStatus {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed { .. } => "completed",
            Self::Failed { .. } => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Result from a completed (or failed) agent lane.
#[allow(dead_code)]
pub struct AgentLaneResult {
    pub agent_run_id: String,
    pub lane_id: String,
    pub provider_name: String,
    pub findings: Vec<crate::providers::traits::RawFinding>,
    pub provider_session_id: Option<String>,
    pub resume_cursor: Option<String>,
    pub checkpoint_metadata_json: Option<String>,
    pub cost_usd: Option<f64>,
    pub status: LaneStatus,
    pub started_at: String,
    pub completed_at: String,
}

/// Snapshot of lane status for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct LaneSnapshot {
    pub lane_id: String,
    pub status: String,
    pub finding_count: usize,
    pub provider_name: String,
    pub error_message: Option<String>,
}

impl From<&AgentLaneResult> for LaneSnapshot {
    fn from(result: &AgentLaneResult) -> Self {
        let (finding_count, error_message) = match &result.status {
            LaneStatus::Completed { finding_count } => (*finding_count, None),
            LaneStatus::Failed { error } => (0, Some(error.clone())),
            LaneStatus::TimedOut => (0, Some("Timed out".to_string())),
            LaneStatus::Cancelled => (0, Some("Cancelled".to_string())),
            _ => (0, None),
        };
        LaneSnapshot {
            lane_id: result.lane_id.clone(),
            status: result.status.as_str().to_string(),
            finding_count,
            provider_name: result.provider_name.clone(),
            error_message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lane_status_as_str() {
        assert_eq!(LaneStatus::Pending.as_str(), "pending");
        assert_eq!(LaneStatus::Running.as_str(), "running");
        assert_eq!(
            LaneStatus::Completed { finding_count: 5 }.as_str(),
            "completed"
        );
        assert_eq!(
            LaneStatus::Failed {
                error: "err".into()
            }
            .as_str(),
            "failed"
        );
        assert_eq!(LaneStatus::TimedOut.as_str(), "timed_out");
        assert_eq!(LaneStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn test_lane_snapshot_from_completed_result() {
        let result = AgentLaneResult {
            agent_run_id: "ar-1".into(),
            lane_id: "security".into(),
            provider_name: "codex".into(),
            findings: vec![],
            provider_session_id: None,
            resume_cursor: None,
            checkpoint_metadata_json: None,
            cost_usd: None,
            status: LaneStatus::Completed { finding_count: 3 },
            started_at: "2026-01-01T00:00:00Z".into(),
            completed_at: "2026-01-01T00:01:00Z".into(),
        };
        let snapshot = LaneSnapshot::from(&result);
        assert_eq!(snapshot.lane_id, "security");
        assert_eq!(snapshot.status, "completed");
        assert_eq!(snapshot.finding_count, 3);
        assert!(snapshot.error_message.is_none());
    }

    #[test]
    fn test_lane_snapshot_from_failed_result() {
        let result = AgentLaneResult {
            agent_run_id: "ar-2".into(),
            lane_id: "arch".into(),
            provider_name: "claude".into(),
            findings: vec![],
            provider_session_id: None,
            resume_cursor: None,
            checkpoint_metadata_json: None,
            cost_usd: None,
            status: LaneStatus::Failed {
                error: "rate limited".into(),
            },
            started_at: "2026-01-01T00:00:00Z".into(),
            completed_at: "2026-01-01T00:00:05Z".into(),
        };
        let snapshot = LaneSnapshot::from(&result);
        assert_eq!(snapshot.status, "failed");
        assert_eq!(snapshot.error_message.as_deref(), Some("rate limited"));
    }
}
