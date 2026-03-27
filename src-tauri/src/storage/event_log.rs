use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// JSONL event log for review pipeline events.
/// Each review run gets its own file: `<events_dir>/<run_id>.jsonl`.
/// Logs references only (run_id, lane_id, counts, statuses) — no raw diffs or provider transcripts.
pub struct EventLog {
    events_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimestampedEvent {
    pub timestamp: String,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[allow(dead_code)]
impl EventLog {
    pub fn new(app_data_dir: &Path) -> Self {
        let events_dir = app_data_dir.join("events");
        Self { events_dir }
    }

    /// Append a single event to the run's JSONL file.
    pub fn append(
        &self,
        run_id: &str,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.events_dir)?;

        let file_path = self.run_file(run_id);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(file_path)?;

        let event = TimestampedEvent {
            timestamp: chrono::Utc::now().to_rfc3339(),
            event_type: event_type.to_string(),
            payload,
        };

        let line = serde_json::to_string(&event)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    /// Read all events for a given run.
    pub fn read(&self, run_id: &str) -> Result<Vec<TimestampedEvent>, std::io::Error> {
        let file_path = self.run_file(run_id);
        if !file_path.exists() {
            return Ok(vec![]);
        }

        let file = fs::File::open(file_path)?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<TimestampedEvent>(&line) {
                Ok(event) => events.push(event),
                Err(e) => {
                    tracing::warn!("Skipping malformed event log line: {}", e);
                }
            }
        }

        Ok(events)
    }

    /// Check if a specific lane completed successfully (for resume logic).
    pub fn lane_completed(&self, run_id: &str, lane_id: &str) -> bool {
        let events = self.read(run_id).unwrap_or_default();
        events.iter().any(|e| {
            e.event_type == "lane_completed"
                && e.payload.get("lane_id").and_then(|v| v.as_str()) == Some(lane_id)
        })
    }

    fn run_file(&self, run_id: &str) -> PathBuf {
        self.events_dir.join(format!("{}.jsonl", run_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_append_and_read() {
        let tmp = tempdir().unwrap();
        let log = EventLog::new(tmp.path());

        log.append(
            "run-1",
            "status_changed",
            serde_json::json!({"status": "running_agents"}),
        )
        .unwrap();

        log.append(
            "run-1",
            "lane_completed",
            serde_json::json!({"lane_id": "security", "finding_count": 3}),
        )
        .unwrap();

        let events = log.read("run-1").unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "status_changed");
        assert_eq!(events[1].event_type, "lane_completed");
        assert_eq!(events[1].payload["lane_id"], "security");
    }

    #[test]
    fn test_read_nonexistent_run() {
        let tmp = tempdir().unwrap();
        let log = EventLog::new(tmp.path());

        let events = log.read("nonexistent").unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn test_lane_completed_check() {
        let tmp = tempdir().unwrap();
        let log = EventLog::new(tmp.path());

        log.append(
            "run-1",
            "lane_completed",
            serde_json::json!({"lane_id": "security", "finding_count": 2}),
        )
        .unwrap();

        assert!(log.lane_completed("run-1", "security"));
        assert!(!log.lane_completed("run-1", "architecture"));
        assert!(!log.lane_completed("run-2", "security"));
    }

    #[test]
    fn test_multiple_runs_isolated() {
        let tmp = tempdir().unwrap();
        let log = EventLog::new(tmp.path());

        log.append("run-1", "event_a", serde_json::json!({}))
            .unwrap();
        log.append("run-2", "event_b", serde_json::json!({}))
            .unwrap();

        assert_eq!(log.read("run-1").unwrap().len(), 1);
        assert_eq!(log.read("run-2").unwrap().len(), 1);
    }

    #[test]
    fn test_malformed_lines_skipped() {
        let tmp = tempdir().unwrap();
        let log = EventLog::new(tmp.path());

        // Write a valid event first
        log.append("run-1", "valid", serde_json::json!({})).unwrap();

        // Manually append a malformed line
        let file_path = tmp.path().join("events").join("run-1.jsonl");
        let mut file = OpenOptions::new().append(true).open(file_path).unwrap();
        writeln!(file, "not valid json {{").unwrap();

        // Another valid event
        log.append("run-1", "also_valid", serde_json::json!({}))
            .unwrap();

        let events = log.read("run-1").unwrap();
        assert_eq!(events.len(), 2); // malformed line skipped
    }
}
