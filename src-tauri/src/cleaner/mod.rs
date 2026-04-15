pub mod dedup;
pub mod normalize;
pub mod rank;
pub mod remap;
pub mod synthesis;
pub mod verify;

use serde::{Deserialize, Serialize};

use crate::providers::traits::RawFinding;
use crate::storage::models::Finding;
pub use synthesis::FindingCluster;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanerConfig {
    pub similarity_threshold: f64,
    pub drop_nitpicks: bool,
    pub max_surface_findings: usize,
    pub min_confidence: f64,
}

impl Default for CleanerConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 0.70,
            drop_nitpicks: true,
            max_surface_findings: 8,
            min_confidence: 0.3,
        }
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct CleanerResult {
    pub surfaced: Vec<Finding>,
    pub dropped: Vec<Finding>,
    pub clusters: Vec<FindingCluster>,
}

pub fn clean(
    raw: Vec<RawFinding>,
    diff: &str,
    review_run_id: &str,
    config: &CleanerConfig,
) -> CleanerResult {
    let normalized = normalize::normalize(raw, review_run_id);
    let deduped = dedup::dedup_jaccard(normalized, config.similarity_threshold);
    let verified = verify::verify(deduped, diff);
    let (surfaced, dropped) = rank::rank_and_suppress(verified, config);

    // Build clusters from surfaced findings
    let clusters =
        synthesis::cluster_findings(surfaced.clone(), review_run_id, config.similarity_threshold);

    CleanerResult {
        surfaced,
        dropped,
        clusters,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIFF: &str = r#"diff --git a/src/auth.rs b/src/auth.rs
--- a/src/auth.rs
+++ b/src/auth.rs
@@ -10,6 +10,8 @@ fn authenticate() {
     let token = get_token();
+    validate_token(&token);
     process(token);
diff --git a/src/db.rs b/src/db.rs
--- a/src/db.rs
+++ b/src/db.rs
@@ -1,4 +1,6 @@ fn query_users() {
     let users = db.query("SELECT *");
+    for user in &users {
+        db.query("SELECT * FROM posts WHERE user_id = ?");
     }
"#;

    #[allow(clippy::too_many_arguments)]
    fn raw(
        title: &str,
        body: &str,
        file: &str,
        severity: &str,
        confidence: f64,
        agent: &str,
        line_start: Option<i32>,
        line_end: Option<i32>,
    ) -> RawFinding {
        RawFinding {
            title: title.to_string(),
            body: body.to_string(),
            file_path: Some(file.to_string()),
            line_start,
            line_end,
            severity: severity.to_string(),
            confidence,
            evidence: None,
            agent_type: agent.to_string(),
            lane_id: None,
            provider_name: None,
            fix_suggestion: None,
        }
    }

    #[test]
    fn test_full_pipeline() {
        let raw_findings = vec![
            raw("Token bypass risk in auth middleware", "Auth middleware is bypassed on the login route allowing unauthenticated access", "src/auth.rs", "blocker", 0.95, "security", Some(10), Some(12)),
            raw("Token bypass risk in auth middleware handler", "Auth middleware is bypassed on the login route which allows unauthenticated access", "src/auth.rs", "blocker", 0.85, "security", Some(10), Some(12)),
            raw("N+1 query pattern", "Loop queries detected", "src/db.rs", "warning", 0.8, "performance", Some(1), Some(4)),
            raw("Dead code", "Unused function", "src/utils.rs", "info", 0.6, "architecture", Some(1), Some(5)),
            raw("Possible race condition", "Maybe thread unsafe", "src/auth.rs", "warning", 0.1, "security", Some(10), Some(11)),
            raw("Use snake_case", "Variable naming", "src/auth.rs", "nitpick", 0.9, "architecture", Some(10), Some(10)),
        ];

        let config = CleanerConfig::default();
        let result = clean(raw_findings, DIFF, "run-1", &config);

        assert_eq!(result.surfaced.len(), 2);
        assert_eq!(result.surfaced[0].severity, "blocker");
        assert_eq!(result.surfaced[1].severity, "warning");
        assert!(result.dropped.len() >= 2);

        // Should have clusters
        assert!(!result.clusters.is_empty());
    }

    #[test]
    fn test_pipeline_empty_input() {
        let config = CleanerConfig::default();
        let result = clean(vec![], DIFF, "run-1", &config);
        assert!(result.surfaced.is_empty());
        assert!(result.dropped.is_empty());
        assert!(result.clusters.is_empty());
    }
}
