use crate::storage::hashing::sha256_hex;
use crate::storage::models::{Finding, ReviewerDecision};

/// Extract a category tag from a finding's title for preference grouping.
/// Uses the first significant word(s) as a rough category signal.
pub fn extract_category_tag(finding: &Finding) -> Option<String> {
    let title = finding.title.to_lowercase();
    // Common category keywords in security/architecture/performance findings
    let keywords = [
        "auth",
        "injection",
        "xss",
        "csrf",
        "token",
        "secret",
        "permission",
        "coupling",
        "boundary",
        "dependency",
        "abstraction",
        "interface",
        "n+1",
        "loop",
        "memory",
        "cache",
        "latency",
        "allocation",
        "null",
        "error",
        "panic",
        "unwrap",
        "validation",
    ];
    for kw in &keywords {
        if title.contains(kw) {
            return Some(kw.to_string());
        }
    }
    None
}

/// Build a ReviewerDecision from a finding and a user action.
pub fn build_decision(
    finding: &Finding,
    decision: &str,
    time_to_decision_ms: Option<i64>,
) -> ReviewerDecision {
    // Deterministic ID makes decision recording idempotent (e.g. across resubmits).
    let id = sha256_hex(&format!(
        "{}:{}:{}",
        finding.review_run_id, finding.id, decision
    ));
    ReviewerDecision {
        id,
        finding_id: finding.id.clone(),
        review_run_id: finding.review_run_id.clone(),
        decision: decision.to_string(),
        original_severity: finding.severity.clone(),
        original_agent_type: finding.agent_type.clone(),
        category_tag: extract_category_tag(finding),
        time_to_decision_ms,
        decided_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::Finding;

    fn make_finding(title: &str, agent_type: &str, severity: &str) -> Finding {
        Finding {
            id: "f1".into(),
            review_run_id: "run1".into(),
            agent_type: agent_type.into(),
            file_path: None,
            line_start: None,
            line_end: None,
            severity: severity.into(),
            confidence: 0.8,
            title: title.into(),
            body: "body".into(),
            evidence: None,
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: false,
            created_at: "2026-01-01".into(),
            cluster_id: None,
            lane_id: None,
            provider_name: None,
            diff_side: None,
            diff_new_line: None,
            fix_search: None,
            fix_replace: None,
            fix_explanation: None,
            fix_status: None,
            fingerprint: None,
            source_kind: None,
            source_id: None,
            explain_json: None,
        }
    }

    #[test]
    fn test_extract_category_tag_auth() {
        let f = make_finding("Token validation bypass risk", "security", "blocker");
        assert_eq!(extract_category_tag(&f), Some("token".into()));
    }

    #[test]
    fn test_extract_category_tag_performance() {
        let f = make_finding("N+1 query in user loader", "performance", "warning");
        assert_eq!(extract_category_tag(&f), Some("n+1".into()));
    }

    #[test]
    fn test_extract_category_tag_none() {
        let f = make_finding("Refactor suggestion", "architecture", "info");
        assert_eq!(extract_category_tag(&f), None);
    }

    #[test]
    fn test_build_decision() {
        let f = make_finding("Auth bypass issue", "security", "blocker");
        let d = build_decision(&f, "accept", Some(2000));
        assert_eq!(d.decision, "accept");
        assert_eq!(d.original_severity, "blocker");
        assert_eq!(d.original_agent_type, "security");
        assert_eq!(d.category_tag, Some("auth".into()));
        assert_eq!(d.time_to_decision_ms, Some(2000));
        assert_eq!(d.finding_id, "f1");
        assert_eq!(d.review_run_id, "run1");
    }

    #[test]
    fn test_build_decision_id_is_deterministic_for_finding_and_decision() {
        let f = make_finding("Auth bypass issue", "security", "blocker");
        let d1 = build_decision(&f, "accept", None);
        let d2 = build_decision(&f, "accept", None);
        assert_eq!(d1.id, d2.id);

        let d3 = build_decision(&f, "reject", None);
        assert_ne!(d1.id, d3.id);
    }

    #[test]
    fn test_build_decision_no_category() {
        let f = make_finding("General issue", "architecture", "info");
        let d = build_decision(&f, "reject", None);
        assert_eq!(d.decision, "reject");
        assert_eq!(d.category_tag, None);
        assert_eq!(d.time_to_decision_ms, None);
    }
}
