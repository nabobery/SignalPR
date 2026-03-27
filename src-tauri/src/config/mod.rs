use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;
use serde::Deserialize;
use tauri::AppHandle;

use crate::cleaner::CleanerConfig;
use crate::providers::claude::ClaudeProvider;
use crate::providers::codex::{CodexProvider, MockProvider};
use crate::providers::traits::ReviewProvider;
use crate::storage::queries;

/// Fully resolved configuration merging defaults, user settings, and repo config.
pub struct ResolvedConfig {
    pub cleaner: CleanerConfig,
    pub preferred_provider: String,
    pub lane_timeout: Duration,
    pub lanes: Vec<String>,
}

/// Resolve config by merging: built-in defaults < user settings (DB) < repo config.
/// Invalid or missing values fall back to defaults silently.
pub fn resolve_config(conn: &Connection, repo: Option<&RepoConfig>) -> ResolvedConfig {
    let defaults = CleanerConfig::default();
    let default_timeout: u64 = 120;
    let default_lanes: Vec<String> = vec![
        "security".to_string(),
        "architecture".to_string(),
        "performance".to_string(),
    ];

    // Layer 1: user settings override defaults
    let mut max_surface_findings = read_setting_as::<usize>(conn, "max_surface_findings")
        .unwrap_or(defaults.max_surface_findings);
    let mut similarity_threshold = read_setting_as::<f64>(conn, "similarity_threshold")
        .unwrap_or(defaults.similarity_threshold);
    let mut drop_nitpicks =
        read_setting_as::<bool>(conn, "drop_nitpicks").unwrap_or(defaults.drop_nitpicks);
    let mut min_confidence =
        read_setting_as::<f64>(conn, "min_confidence").unwrap_or(defaults.min_confidence);
    let mut preferred_provider = queries::get_setting(conn, "preferred_provider")
        .ok()
        .flatten()
        .unwrap_or_else(|| "auto".to_string());
    let mut lane_timeout_secs =
        read_setting_as::<u64>(conn, "lane_timeout_secs").unwrap_or(default_timeout);
    let mut lanes = default_lanes.clone();

    // Layer 2: repo config overrides user settings
    if let Some(repo) = repo {
        if let Some(v) = repo.max_findings {
            max_surface_findings = v;
        }
        if let Some(v) = repo.similarity_threshold {
            similarity_threshold = v;
        }
        if let Some(v) = repo.drop_nitpicks {
            drop_nitpicks = v;
        }
        if let Some(v) = repo.min_confidence {
            min_confidence = v;
        }
        if let Some(ref v) = repo.preferred_provider {
            preferred_provider = v.clone();
        }
        if let Some(v) = repo.lane_timeout_secs {
            lane_timeout_secs = v;
        }
        if let Some(ref v) = repo.lanes {
            let filtered: Vec<String> = v
                .iter()
                .filter_map(|lane| match lane.as_str() {
                    "security" | "architecture" | "performance" => Some(lane.clone()),
                    _ => None,
                })
                .collect();
            if filtered.is_empty() {
                tracing::warn!("Repo config lanes were empty/invalid, falling back to defaults");
            } else {
                lanes = filtered;
            }
        }
    }

    ResolvedConfig {
        cleaner: CleanerConfig {
            similarity_threshold,
            drop_nitpicks,
            max_surface_findings,
            min_confidence,
        },
        preferred_provider,
        lane_timeout: Duration::from_secs(lane_timeout_secs),
        lanes,
    }
}

/// Select a review provider based on preference and availability.
/// Falls back through: preferred → codex → claude → mock.
pub async fn select_provider(app: &AppHandle, preference: &str) -> Arc<dyn ReviewProvider> {
    match preference {
        "codex" => {
            let codex = CodexProvider::new(app.clone());
            if codex.health_check().await.available {
                return Arc::new(codex);
            }
            tracing::warn!("Codex preferred but unavailable, trying Claude");
        }
        "claude" => {
            let claude = ClaudeProvider::new();
            if claude.health_check().await.available {
                return Arc::new(claude);
            }
            tracing::warn!("Claude preferred but unavailable, trying Codex");
        }
        _ => {} // "auto" — try both in order
    }

    // Auto fallback chain: codex → claude → mock
    let codex = CodexProvider::new(app.clone());
    if codex.health_check().await.available {
        return Arc::new(codex);
    }

    let claude = ClaudeProvider::new();
    if claude.health_check().await.available {
        tracing::info!("Codex not available, using Claude provider");
        return Arc::new(claude);
    }

    tracing::info!("No providers available, using mock provider");
    Arc::new(MockProvider::with_default_fixture())
}

/// Repo-level config loaded from `.signalpr.yml` at workspace root.
/// All fields are optional — missing fields fall back to user settings or defaults.
/// Unknown fields are silently ignored for forward compatibility.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    pub lanes: Option<Vec<String>>,
    pub max_findings: Option<usize>,
    pub similarity_threshold: Option<f64>,
    pub drop_nitpicks: Option<bool>,
    pub min_confidence: Option<f64>,
    pub lane_timeout_secs: Option<u64>,
    pub preferred_provider: Option<String>,
}

/// Load `.signalpr.yml` from the workspace root. Returns None if file
/// is missing or malformed (logs a warning on parse failure).
pub fn load_repo_config(workspace_path: &Path) -> Option<RepoConfig> {
    let path = workspace_path.join(".signalpr.yml");
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_yml::from_str(&content) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("Failed to parse .signalpr.yml: {}", e);
            None
        }
    }
}

fn read_setting_as<T: std::str::FromStr>(conn: &Connection, key: &str) -> Option<T> {
    queries::get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<T>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::queries;

    #[test]
    fn test_resolve_config_defaults() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let config = resolve_config(&conn, None);
        assert_eq!(config.cleaner.max_surface_findings, 8);
        assert!((config.cleaner.similarity_threshold - 0.70).abs() < f64::EPSILON);
        assert_eq!(config.preferred_provider, "auto");
        assert_eq!(config.lane_timeout.as_secs(), 120);
        assert!(config.cleaner.drop_nitpicks);
        assert_eq!(
            config.lanes,
            vec![
                "security".to_string(),
                "architecture".to_string(),
                "performance".to_string(),
            ]
        );
    }

    #[test]
    fn test_resolve_config_from_settings() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "15").unwrap();
        queries::upsert_setting(&conn, "preferred_provider", "claude").unwrap();
        queries::upsert_setting(&conn, "lane_timeout_secs", "60").unwrap();
        let config = resolve_config(&conn, None);
        assert_eq!(config.cleaner.max_surface_findings, 15);
        assert_eq!(config.preferred_provider, "claude");
        assert_eq!(config.lane_timeout.as_secs(), 60);
    }

    #[test]
    fn test_resolve_config_invalid_setting_falls_back() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "not_a_number").unwrap();
        queries::upsert_setting(&conn, "similarity_threshold", "abc").unwrap();
        let config = resolve_config(&conn, None);
        assert_eq!(config.cleaner.max_surface_findings, 8); // default
        assert!((config.cleaner.similarity_threshold - 0.70).abs() < f64::EPSILON);
        // default
    }

    #[test]
    fn test_resolve_config_partial_settings() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "drop_nitpicks", "false").unwrap();
        let config = resolve_config(&conn, None);
        assert!(!config.cleaner.drop_nitpicks);
        assert_eq!(config.cleaner.max_surface_findings, 8);
    }

    // --- Repo config YAML parsing tests ---

    #[test]
    fn test_parse_repo_config() {
        let yaml = "lanes:\n  - security\n  - performance\nmax_findings: 5\ndrop_nitpicks: false\nsimilarity_threshold: 0.80\n";
        let config: RepoConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(
            config.lanes,
            Some(vec!["security".into(), "performance".into()])
        );
        assert_eq!(config.max_findings, Some(5));
        assert_eq!(config.drop_nitpicks, Some(false));
        assert_eq!(config.similarity_threshold, Some(0.80));
    }

    #[test]
    fn test_parse_tolerates_unknown_fields() {
        let yaml = "future_key: true\nlanes:\n  - security\n";
        let config: RepoConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.lanes, Some(vec!["security".into()]));
    }

    #[test]
    fn test_repo_config_overrides_user_settings() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "15").unwrap();
        let repo = RepoConfig {
            max_findings: Some(3),
            ..Default::default()
        };
        let config = resolve_config(&conn, Some(&repo));
        assert_eq!(config.cleaner.max_surface_findings, 3);
    }

    #[test]
    fn test_repo_config_does_not_override_unset_fields() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "15").unwrap();
        let repo = RepoConfig::default();
        let config = resolve_config(&conn, Some(&repo));
        assert_eq!(config.cleaner.max_surface_findings, 15);
    }

    #[test]
    fn test_repo_config_lanes_filters_unknown_and_preserves_order() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let repo = RepoConfig {
            lanes: Some(vec![
                "performance".into(),
                "unknown".into(),
                "security".into(),
            ]),
            ..Default::default()
        };
        let config = resolve_config(&conn, Some(&repo));
        assert_eq!(
            config.lanes,
            vec!["performance".to_string(), "security".to_string()]
        );
    }

    #[test]
    fn test_load_repo_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_repo_config(dir.path()).is_none());
    }

    #[test]
    fn test_load_repo_config_malformed_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".signalpr.yml"), "{{invalid").unwrap();
        assert!(load_repo_config(dir.path()).is_none());
    }

    #[test]
    fn test_load_repo_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".signalpr.yml"),
            "max_findings: 5\ndrop_nitpicks: true\n",
        )
        .unwrap();
        let config = load_repo_config(dir.path()).expect("should parse");
        assert_eq!(config.max_findings, Some(5));
        assert_eq!(config.drop_nitpicks, Some(true));
    }

    #[test]
    fn test_load_repo_config_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".signalpr.yml"), "").unwrap();
        let config = load_repo_config(dir.path()).expect("empty file should parse");
        assert!(config.max_findings.is_none());
    }
}
