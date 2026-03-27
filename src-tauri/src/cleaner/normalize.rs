use crate::providers::traits::RawFinding;
use crate::storage::models::Finding;
use uuid::Uuid;

pub fn normalize(raw: Vec<RawFinding>, review_run_id: &str) -> Vec<Finding> {
    raw.into_iter()
        .map(|r| {
            let evidence_json = r
                .evidence
                .map(|e| serde_json::to_string(&e).unwrap_or_default());
            let has_anchor = r.file_path.is_some() && r.line_start.is_some();

            Finding {
                id: Uuid::new_v4().to_string(),
                review_run_id: review_run_id.to_string(),
                agent_type: if r.agent_type.is_empty() {
                    "general".to_string()
                } else {
                    r.agent_type
                },
                file_path: r.file_path,
                line_start: r.line_start,
                line_end: r.line_end,
                severity: normalize_severity(&r.severity),
                confidence: r.confidence.clamp(0.0, 1.0),
                title: r.title,
                body: r.body,
                evidence: evidence_json,
                status: "active".to_string(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: has_anchor,
                created_at: chrono::Utc::now().to_rfc3339(),
                cluster_id: None,
                lane_id: r.lane_id,
                provider_name: r.provider_name,
                diff_side: None,
                diff_new_line: None,
            }
        })
        .collect()
}

fn normalize_severity(s: &str) -> String {
    match s.to_lowercase().as_str() {
        "blocker" | "critical" | "warning" | "info" | "nitpick" => s.to_lowercase(),
        "high" | "error" => "critical".to_string(),
        "medium" | "warn" => "warning".to_string(),
        "low" | "minor" => "info".to_string(),
        "style" | "nit" => "nitpick".to_string(),
        _ => "info".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::RawFinding;

    fn raw(title: &str, severity: &str, agent: &str) -> RawFinding {
        RawFinding {
            title: title.to_string(),
            body: "Test body".to_string(),
            file_path: Some("src/main.rs".to_string()),
            line_start: Some(10),
            line_end: Some(20),
            severity: severity.to_string(),
            confidence: 0.8,
            evidence: Some(vec!["evidence".to_string()]),
            agent_type: agent.to_string(),
            lane_id: None,
            provider_name: None,
        }
    }

    #[test]
    fn test_normalize_basic() {
        let findings = normalize(vec![raw("Bug", "warning", "security")], "run-1");
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].title, "Bug");
        assert_eq!(findings[0].severity, "warning");
        assert_eq!(findings[0].agent_type, "security");
        assert_eq!(findings[0].review_run_id, "run-1");
        assert!(findings[0].is_anchored);
    }

    #[test]
    fn test_normalize_empty_agent_type() {
        let mut r = raw("Bug", "warning", "");
        r.agent_type = "".to_string();
        let findings = normalize(vec![r], "run-1");
        assert_eq!(findings[0].agent_type, "general");
    }

    #[test]
    fn test_normalize_severity_aliases() {
        assert_eq!(normalize_severity("high"), "critical");
        assert_eq!(normalize_severity("error"), "critical");
        assert_eq!(normalize_severity("medium"), "warning");
        assert_eq!(normalize_severity("low"), "info");
        assert_eq!(normalize_severity("nit"), "nitpick");
        assert_eq!(normalize_severity("style"), "nitpick");
        assert_eq!(normalize_severity("unknown"), "info");
        assert_eq!(normalize_severity("BLOCKER"), "blocker");
    }

    #[test]
    fn test_normalize_no_line_range_not_anchored() {
        let r = RawFinding {
            title: "Bug".into(),
            body: "body".into(),
            file_path: Some("file.rs".into()),
            line_start: None,
            line_end: None,
            severity: "warning".into(),
            confidence: 0.5,
            evidence: None,
            agent_type: "security".into(),
            lane_id: None,
            provider_name: None,
        };
        let findings = normalize(vec![r], "run-1");
        assert!(!findings[0].is_anchored);
    }

    #[test]
    fn test_normalize_empty_input() {
        let findings = normalize(vec![], "run-1");
        assert!(findings.is_empty());
    }

    #[test]
    fn test_normalize_clamps_confidence() {
        let mut r = raw("Bug", "warning", "security");
        r.confidence = 1.5;
        let findings = normalize(vec![r], "run-1");
        assert_eq!(findings[0].confidence, 1.0);

        let mut r2 = raw("Bug", "warning", "security");
        r2.confidence = -0.5;
        let findings2 = normalize(vec![r2], "run-1");
        assert_eq!(findings2[0].confidence, 0.0);
    }
}
