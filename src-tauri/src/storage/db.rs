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
    }

    #[test]
    fn test_migration_is_idempotent() {
        let db = init_db_in_memory().expect("Failed to init DB");
        let conn = db.0.lock().unwrap();
        // Running migrations again should not fail
        run_migrations(&conn).expect("Second migration run should succeed");
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
