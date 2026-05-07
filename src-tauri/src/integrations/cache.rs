use rusqlite::Connection;
use serde::{Deserialize, Serialize};

const DEFAULT_TTL_SECONDS: i64 = 3600; // 1 hour
const NEGATIVE_CACHE_TTL_SECONDS: i64 = 300; // 5 minutes for failures

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedIssue {
    pub tracker: String,
    pub issue_key: String,
    pub title: String,
    pub body_excerpt: Option<String>,
    pub labels: Vec<String>,
    pub state: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheStatus {
    Ok,
    NotFound,
    Unauthorized,
    TransientError,
}

impl CacheStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            CacheStatus::Ok => "ok",
            CacheStatus::NotFound => "not_found",
            CacheStatus::Unauthorized => "unauthorized",
            CacheStatus::TransientError => "transient_error",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "ok" => CacheStatus::Ok,
            "not_found" => CacheStatus::NotFound,
            "unauthorized" => CacheStatus::Unauthorized,
            _ => CacheStatus::TransientError,
        }
    }
}

/// Build a cache key from tracker, scope, and issue key.
pub fn build_cache_key(tracker: &str, scope: Option<&str>, issue_key: &str) -> String {
    match scope {
        Some(s) => format!("{}:{}:{}", tracker, s, issue_key),
        None => format!("{}:{}", tracker, issue_key),
    }
}

/// Look up a cached issue by cache key. Returns None if not found or expired.
pub fn get_issue_cache(
    conn: &Connection,
    cache_key: &str,
) -> Option<(CacheStatus, Option<CachedIssue>)> {
    let result = conn.query_row(
        "SELECT status, value_json, expires_at FROM issue_context_cache WHERE cache_key = ?1",
        rusqlite::params![cache_key],
        |row| {
            let status: String = row.get(0)?;
            let value_json: String = row.get(1)?;
            let expires_at: String = row.get(2)?;
            Ok((status, value_json, expires_at))
        },
    );

    match result {
        Ok((status_str, value_json, expires_at)) => {
            let now = chrono::Utc::now().to_rfc3339();
            if expires_at < now {
                return None;
            }
            let status = CacheStatus::from_str(&status_str);
            let issue = if status == CacheStatus::Ok {
                serde_json::from_str(&value_json).ok()
            } else {
                None
            };
            Some((status, issue))
        }
        Err(_) => None,
    }
}

/// Store an issue in the cache.
pub fn put_issue_cache(
    conn: &Connection,
    cache_key: &str,
    tracker: &str,
    scope: Option<&str>,
    issue_key: &str,
    status: &CacheStatus,
    issue: Option<&CachedIssue>,
) {
    let now = chrono::Utc::now();
    let ttl = if *status == CacheStatus::Ok {
        DEFAULT_TTL_SECONDS
    } else {
        NEGATIVE_CACHE_TTL_SECONDS
    };
    let expires_at = now + chrono::Duration::seconds(ttl);
    let value_json = issue
        .and_then(|i| serde_json::to_string(i).ok())
        .unwrap_or_else(|| "{}".to_string());

    let _ = conn.execute(
        "INSERT OR REPLACE INTO issue_context_cache (cache_key, tracker, scope, issue_key, value_json, fetched_at, expires_at, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            cache_key,
            tracker,
            scope.unwrap_or(""),
            issue_key,
            value_json,
            now.to_rfc3339(),
            expires_at.to_rfc3339(),
            status.as_str(),
        ],
    );
}

/// Remove all expired entries from the cache.
pub fn prune_issue_cache(conn: &Connection) {
    let now = chrono::Utc::now().to_rfc3339();
    let _ = conn.execute(
        "DELETE FROM issue_context_cache WHERE expires_at < ?1",
        rusqlite::params![now],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;

    #[test]
    fn test_build_cache_key_with_scope() {
        let key = build_cache_key("jira", Some("myorg.atlassian.net"), "AUTH-123");
        assert_eq!(key, "jira:myorg.atlassian.net:AUTH-123");
    }

    #[test]
    fn test_build_cache_key_without_scope() {
        let key = build_cache_key("github", None, "42");
        assert_eq!(key, "github:42");
    }

    #[test]
    fn test_cache_roundtrip() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();

        let issue = CachedIssue {
            tracker: "jira".into(),
            issue_key: "AUTH-123".into(),
            title: "Fix login".into(),
            body_excerpt: Some("Users cannot log in".into()),
            labels: vec!["bug".into()],
            state: "In Progress".into(),
            url: Some("https://myorg.atlassian.net/browse/AUTH-123".into()),
        };

        let cache_key = build_cache_key("jira", Some("myorg.atlassian.net"), "AUTH-123");
        put_issue_cache(
            &conn,
            &cache_key,
            "jira",
            Some("myorg.atlassian.net"),
            "AUTH-123",
            &CacheStatus::Ok,
            Some(&issue),
        );

        let result = get_issue_cache(&conn, &cache_key);
        assert!(result.is_some());
        let (status, cached) = result.unwrap();
        assert_eq!(status, CacheStatus::Ok);
        let cached = cached.unwrap();
        assert_eq!(cached.title, "Fix login");
        assert_eq!(cached.state, "In Progress");
    }

    #[test]
    fn test_cache_negative_entry() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();

        let cache_key = build_cache_key("jira", None, "GONE-999");
        put_issue_cache(
            &conn,
            &cache_key,
            "jira",
            None,
            "GONE-999",
            &CacheStatus::NotFound,
            None,
        );

        let result = get_issue_cache(&conn, &cache_key);
        assert!(result.is_some());
        let (status, cached) = result.unwrap();
        assert_eq!(status, CacheStatus::NotFound);
        assert!(cached.is_none());
    }

    #[test]
    fn test_cache_miss() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let result = get_issue_cache(&conn, "nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn test_prune_does_not_fail() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        prune_issue_cache(&conn);
    }
}
