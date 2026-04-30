use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

#[allow(dead_code)]
pub struct AppDb(pub Mutex<Connection>);

const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS workspaces (
  id TEXT PRIMARY KEY,
  local_path TEXT NOT NULL,
  remote_owner TEXT NOT NULL,
  remote_repo TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS pull_requests (
  id TEXT PRIMARY KEY,
  workspace_id TEXT NOT NULL REFERENCES workspaces(id),
  pr_number INTEGER NOT NULL,
  title TEXT NOT NULL,
  author TEXT,
  base_branch TEXT,
  head_branch TEXT,
  url TEXT NOT NULL,
  diff_text TEXT,
  changed_files TEXT,
  fetched_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS review_runs (
  id TEXT PRIMARY KEY,
  pr_id TEXT NOT NULL REFERENCES pull_requests(id),
  status TEXT NOT NULL DEFAULT 'created',
  started_at TEXT,
  completed_at TEXT,
  error_message TEXT
);

CREATE TABLE IF NOT EXISTS findings (
  id TEXT PRIMARY KEY,
  review_run_id TEXT NOT NULL REFERENCES review_runs(id),
  agent_type TEXT NOT NULL,
  file_path TEXT,
  line_start INTEGER,
  line_end INTEGER,
  severity TEXT NOT NULL,
  confidence REAL NOT NULL,
  title TEXT NOT NULL,
  body TEXT NOT NULL,
  evidence TEXT,
  status TEXT NOT NULL DEFAULT 'active',
  user_edited_body TEXT,
  user_severity_override TEXT,
  is_anchored INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS submission_records (
  id TEXT PRIMARY KEY,
  review_run_id TEXT NOT NULL REFERENCES review_runs(id),
  review_action TEXT NOT NULL,
  submitted_at TEXT,
  status TEXT NOT NULL DEFAULT 'pending',
  gh_review_id TEXT,
  error_message TEXT
);

CREATE TABLE IF NOT EXISTS tool_status (
  tool_name TEXT PRIMARY KEY,
  status TEXT NOT NULL,
  version TEXT,
  message TEXT,
  checked_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS schema_version (
  version INTEGER PRIMARY KEY
);
"#;

const MIGRATION_V2: &str = r#"
CREATE TABLE IF NOT EXISTS agent_runs (
  id TEXT PRIMARY KEY,
  review_run_id TEXT NOT NULL REFERENCES review_runs(id),
  lane_id TEXT NOT NULL,
  provider_name TEXT NOT NULL,
  status TEXT NOT NULL DEFAULT 'pending',
  started_at TEXT,
  completed_at TEXT,
  finding_count INTEGER DEFAULT 0,
  error_message TEXT
);

CREATE TABLE IF NOT EXISTS finding_clusters (
  id TEXT PRIMARY KEY,
  review_run_id TEXT NOT NULL REFERENCES review_runs(id),
  label TEXT,
  representative_finding_id TEXT,
  member_count INTEGER NOT NULL DEFAULT 1,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL,
  updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS embedding_cache (
  text_hash TEXT PRIMARY KEY,
  model_id TEXT NOT NULL,
  embedding BLOB NOT NULL,
  created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Add Phase 2 columns to findings (nullable for backward compat with V1 data)
ALTER TABLE findings ADD COLUMN cluster_id TEXT REFERENCES finding_clusters(id);
ALTER TABLE findings ADD COLUMN lane_id TEXT;
ALTER TABLE findings ADD COLUMN provider_name TEXT;
ALTER TABLE findings ADD COLUMN diff_side TEXT;
ALTER TABLE findings ADD COLUMN diff_new_line INTEGER;
"#;

pub fn init_db(app_data_dir: &Path) -> Result<AppDb, rusqlite::Error> {
    let db_path = app_data_dir.join("signalpr.db");
    let conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    run_migrations(&conn)?;
    Ok(AppDb(Mutex::new(conn)))
}

#[allow(dead_code)]
pub fn init_db_in_memory() -> Result<AppDb, rusqlite::Error> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON;")?;
    run_migrations(&conn)?;
    Ok(AppDb(Mutex::new(conn)))
}

const MIGRATION_V3: &str = r#"
-- Submission tracking for idempotent retry
ALTER TABLE submission_records ADD COLUMN idempotency_key TEXT;
ALTER TABLE submission_records ADD COLUMN attempt_count INTEGER DEFAULT 1;
ALTER TABLE submission_records ADD COLUMN last_attempt_at TEXT;

-- Diff change detection
ALTER TABLE pull_requests ADD COLUMN diff_hash TEXT;

-- Performance indexes
CREATE INDEX IF NOT EXISTS idx_review_runs_status ON review_runs(status);
CREATE INDEX IF NOT EXISTS idx_findings_review_run ON findings(review_run_id);
"#;

const MIGRATION_V4: &str = r#"
-- Phase 4: Reviewer preference learning
CREATE TABLE IF NOT EXISTS reviewer_decisions (
  id TEXT PRIMARY KEY,
  finding_id TEXT NOT NULL REFERENCES findings(id),
  review_run_id TEXT NOT NULL,
  decision TEXT NOT NULL,
  original_severity TEXT NOT NULL,
  original_agent_type TEXT NOT NULL,
  category_tag TEXT,
  time_to_decision_ms INTEGER,
  decided_at TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_decisions_agent_type ON reviewer_decisions(original_agent_type);
CREATE INDEX IF NOT EXISTS idx_decisions_category ON reviewer_decisions(category_tag);

CREATE TABLE IF NOT EXISTS preference_summaries (
  id TEXT PRIMARY KEY,
  agent_type TEXT NOT NULL,
  category_tag TEXT,
  accept_rate REAL NOT NULL DEFAULT 0.0,
  total_decisions INTEGER NOT NULL DEFAULT 0,
  last_updated TEXT NOT NULL DEFAULT (datetime('now')),
  UNIQUE(agent_type, category_tag)
);

-- Phase 4: Auto-fix columns on findings
ALTER TABLE findings ADD COLUMN fix_search TEXT;
ALTER TABLE findings ADD COLUMN fix_replace TEXT;
ALTER TABLE findings ADD COLUMN fix_explanation TEXT;
ALTER TABLE findings ADD COLUMN fix_status TEXT DEFAULT 'none';
"#;

const MIGRATION_V5: &str = r#"
-- Phase 5: Provider session metadata and governance tracking
ALTER TABLE agent_runs ADD COLUMN governance_tier_at_run TEXT;
ALTER TABLE agent_runs ADD COLUMN provider_session_id TEXT;
ALTER TABLE agent_runs ADD COLUMN resume_cursor TEXT;
ALTER TABLE agent_runs ADD COLUMN checkpoint_metadata_json TEXT;
ALTER TABLE agent_runs ADD COLUMN cost_usd REAL;
"#;

fn run_migrations(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current_version: i32 = conn
        .query_row(
            "SELECT version FROM schema_version ORDER BY version DESC LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        conn.execute_batch(MIGRATION_V1)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (1)",
            [],
        )?;
    }

    if current_version < 2 {
        conn.execute_batch(MIGRATION_V2)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (2)",
            [],
        )?;
    }

    if current_version < 3 {
        conn.execute_batch(MIGRATION_V3)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (3)",
            [],
        )?;
    }

    if current_version < 4 {
        conn.execute_batch(MIGRATION_V4)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (4)",
            [],
        )?;
    }

    if current_version < 5 {
        conn.execute_batch(MIGRATION_V5)?;
        conn.execute(
            "INSERT OR REPLACE INTO schema_version (version) VALUES (5)",
            [],
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_db_in_memory() {
        let db = init_db_in_memory().expect("Failed to init in-memory DB");
        let conn = db.0.lock().unwrap();

        // Verify all tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"workspaces".to_string()));
        assert!(tables.contains(&"pull_requests".to_string()));
        assert!(tables.contains(&"review_runs".to_string()));
        assert!(tables.contains(&"findings".to_string()));
        assert!(tables.contains(&"submission_records".to_string()));
        assert!(tables.contains(&"tool_status".to_string()));
        // V2 tables
        assert!(tables.contains(&"agent_runs".to_string()));
        assert!(tables.contains(&"finding_clusters".to_string()));
        assert!(tables.contains(&"settings".to_string()));
        assert!(tables.contains(&"embedding_cache".to_string()));
        // V4 tables
        assert!(tables.contains(&"reviewer_decisions".to_string()));
        assert!(tables.contains(&"preference_summaries".to_string()));
    }

    #[test]
    fn test_migration_is_idempotent() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();
        // Running migrations again should not fail
        run_migrations(&conn).expect("Second migration run should succeed");
    }

    #[test]
    fn test_v3_submission_columns_exist() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();
        // Insert test data to verify V3 columns exist
        conn.execute(
            "INSERT INTO workspaces (id, local_path, remote_owner, remote_repo) VALUES ('ws', '/', 'o', 'r')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO pull_requests (id, workspace_id, pr_number, title, url, diff_hash) VALUES ('pr', 'ws', 1, 't', 'u', 'abc123')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO review_runs (id, pr_id, status) VALUES ('run', 'pr', 'ready')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO submission_records (id, review_run_id, review_action, idempotency_key, attempt_count, last_attempt_at) VALUES ('s', 'run', 'comment', 'key123', 2, '2026-03-27T00:00:00Z')",
            [],
        )
        .expect("V3 columns should exist");
    }

    #[test]
    fn test_v3_indexes_exist() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();
        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(indexes.contains(&"idx_review_runs_status".to_string()));
        assert!(indexes.contains(&"idx_findings_review_run".to_string()));
    }

    #[test]
    fn test_v4_preference_tables_exist() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();

        // Set up required parent rows
        conn.execute(
            "INSERT INTO workspaces (id, local_path, remote_owner, remote_repo) VALUES ('ws', '/', 'o', 'r')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO pull_requests (id, workspace_id, pr_number, title, url) VALUES ('pr', 'ws', 1, 't', 'u')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO review_runs (id, pr_id, status) VALUES ('run', 'pr', 'ready')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO findings (id, review_run_id, agent_type, severity, confidence, title, body) VALUES ('f1', 'run', 'security', 'warning', 0.8, 'Test', 'Body')",
            [],
        ).unwrap();

        // Test reviewer_decisions insert
        conn.execute(
            "INSERT INTO reviewer_decisions (id, finding_id, review_run_id, decision, original_severity, original_agent_type, category_tag, time_to_decision_ms) VALUES ('d1', 'f1', 'run', 'accept', 'warning', 'security', 'auth', 1500)",
            [],
        ).expect("V4 reviewer_decisions table should exist");

        // Test preference_summaries insert
        conn.execute(
            "INSERT INTO preference_summaries (id, agent_type, category_tag, accept_rate, total_decisions, last_updated) VALUES ('ps1', 'security', 'auth', 0.75, 10, '2026-03-27T00:00:00Z')",
            [],
        ).expect("V4 preference_summaries table should exist");

        // Test fix columns on findings
        conn.execute(
            "UPDATE findings SET fix_search = 'old', fix_replace = 'new', fix_explanation = 'reason', fix_status = 'pending' WHERE id = 'f1'",
            [],
        ).expect("V4 fix columns should exist on findings");
    }

    #[test]
    fn test_v4_indexes_exist() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();
        let indexes: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_decisions_%'",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(indexes.contains(&"idx_decisions_agent_type".to_string()));
        assert!(indexes.contains(&"idx_decisions_category".to_string()));
    }

    #[test]
    fn test_foreign_keys_enabled() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();
        let fk_enabled: i32 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(fk_enabled, 1);
    }
}
