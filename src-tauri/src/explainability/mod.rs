use serde::{Deserialize, Serialize};

use crate::storage::models::Finding;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingExplanation {
    pub origin: OriginInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking: Option<RankingInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferences: Option<PreferenceInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ownership: Option<OwnershipInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issue_context: Option<IssueContextInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueContextInfo {
    pub included_count: usize,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginInfo {
    pub source_kind: String,
    pub source_id: Option<String>,
    pub lane_id: String,
    pub provider_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankingInfo {
    pub confidence_raw: f64,
    pub severity_raw: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suppressed_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceInfo {
    pub category_tag: Option<String>,
    pub accept_rate: Option<f64>,
    pub total_decisions: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub override_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OwnershipInfo {
    pub owners: Vec<String>,
}

#[derive(Default)]
pub struct ExplainContext {
    pub category_tag: Option<String>,
    pub accept_rate: Option<f64>,
    pub total_decisions: Option<u32>,
    pub override_action: Option<String>,
    pub owners: Vec<String>,
    pub suppressed_reason: Option<String>,
    pub issue_context_included_count: usize,
    pub issue_context_sources: Vec<String>,
}

/// Build explanation JSON for a finding using available context.
pub fn build_explanation(finding: &Finding, ctx: &ExplainContext) -> FindingExplanation {
    let origin = OriginInfo {
        source_kind: finding
            .source_kind
            .clone()
            .unwrap_or_else(|| "ai_provider".to_string()),
        source_id: finding.source_id.clone(),
        lane_id: finding
            .lane_id
            .clone()
            .unwrap_or_else(|| "unknown".to_string()),
        provider_name: finding.provider_name.clone(),
    };

    let ranking = Some(RankingInfo {
        confidence_raw: finding.confidence,
        severity_raw: finding.severity.clone(),
        suppressed_reason: ctx.suppressed_reason.clone(),
    });

    let preferences = if ctx.category_tag.is_some() || ctx.accept_rate.is_some() {
        Some(PreferenceInfo {
            category_tag: ctx.category_tag.clone(),
            accept_rate: ctx.accept_rate,
            total_decisions: ctx.total_decisions,
            override_action: ctx.override_action.clone(),
        })
    } else {
        None
    };

    let ownership = if !ctx.owners.is_empty() {
        Some(OwnershipInfo {
            owners: ctx.owners.clone(),
        })
    } else {
        None
    };

    let issue_context = if ctx.issue_context_included_count > 0 {
        Some(IssueContextInfo {
            included_count: ctx.issue_context_included_count,
            sources: ctx.issue_context_sources.clone(),
        })
    } else {
        None
    };

    FindingExplanation {
        origin,
        ranking,
        preferences,
        ownership,
        issue_context,
    }
}

/// Serialize a FindingExplanation to a JSON string for storage.
pub fn to_json(explanation: &FindingExplanation) -> Option<String> {
    serde_json::to_string(explanation).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_finding() -> Finding {
        Finding {
            id: "f1".into(),
            review_run_id: "run1".into(),
            agent_type: "security".into(),
            file_path: Some("src/auth.rs".into()),
            line_start: Some(10),
            line_end: Some(15),
            severity: "critical".into(),
            confidence: 0.85,
            title: "SQL injection risk".into(),
            body: "Possible injection".into(),
            evidence: None,
            status: "active".into(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: false,
            created_at: "2026-01-01T00:00:00Z".into(),
            cluster_id: None,
            lane_id: Some("security".into()),
            provider_name: Some("codex".into()),
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
    fn test_build_explanation_minimal() {
        let finding = test_finding();
        let ctx = ExplainContext::default();
        let explain = build_explanation(&finding, &ctx);

        assert_eq!(explain.origin.source_kind, "ai_provider");
        assert_eq!(explain.origin.lane_id, "security");
        assert!(explain.ranking.is_some());
        assert!(explain.preferences.is_none());
        assert!(explain.ownership.is_none());
    }

    #[test]
    fn test_build_explanation_with_preferences() {
        let finding = test_finding();
        let ctx = ExplainContext {
            category_tag: Some("injection".into()),
            accept_rate: Some(0.8),
            total_decisions: Some(10),
            ..Default::default()
        };
        let explain = build_explanation(&finding, &ctx);

        let prefs = explain.preferences.unwrap();
        assert_eq!(prefs.category_tag, Some("injection".into()));
        assert_eq!(prefs.accept_rate, Some(0.8));
        assert_eq!(prefs.total_decisions, Some(10));
    }

    #[test]
    fn test_build_explanation_with_ownership() {
        let finding = test_finding();
        let ctx = ExplainContext {
            owners: vec!["@security-team".into(), "@alice".into()],
            ..Default::default()
        };
        let explain = build_explanation(&finding, &ctx);

        let ownership = explain.ownership.unwrap();
        assert_eq!(ownership.owners.len(), 2);
        assert!(ownership.owners.contains(&"@security-team".to_string()));
    }

    #[test]
    fn test_build_explanation_with_local_check() {
        let mut finding = test_finding();
        finding.source_kind = Some("local_check".into());
        finding.source_id = Some("no-unused-vars".into());
        let ctx = ExplainContext::default();
        let explain = build_explanation(&finding, &ctx);

        assert_eq!(explain.origin.source_kind, "local_check");
        assert_eq!(explain.origin.source_id, Some("no-unused-vars".into()));
    }

    #[test]
    fn test_serialization_roundtrip() {
        let finding = test_finding();
        let ctx = ExplainContext {
            category_tag: Some("injection".into()),
            accept_rate: Some(0.75),
            total_decisions: Some(5),
            owners: vec!["@team".into()],
            suppressed_reason: None,
            override_action: None,
            ..Default::default()
        };
        let explain = build_explanation(&finding, &ctx);

        let json = to_json(&explain).unwrap();
        let parsed: FindingExplanation = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.origin.lane_id, "security");
        assert!(parsed.preferences.is_some());
        assert!(parsed.ownership.is_some());
    }

    #[test]
    fn test_suppressed_reason() {
        let finding = test_finding();
        let ctx = ExplainContext {
            suppressed_reason: Some("low_confidence".into()),
            ..Default::default()
        };
        let explain = build_explanation(&finding, &ctx);

        assert_eq!(
            explain.ranking.unwrap().suppressed_reason,
            Some("low_confidence".into())
        );
    }

    #[test]
    fn test_issue_context_present() {
        let finding = test_finding();
        let ctx = ExplainContext {
            issue_context_included_count: 3,
            issue_context_sources: vec![
                "github:issue:#1".into(),
                "jira:issue:AUTH-42".into(),
                "linear:issue:ENG-99".into(),
            ],
            ..Default::default()
        };
        let explain = build_explanation(&finding, &ctx);
        let ic = explain.issue_context.unwrap();
        assert_eq!(ic.included_count, 3);
        assert_eq!(ic.sources.len(), 3);
    }

    #[test]
    fn test_issue_context_absent_when_empty() {
        let finding = test_finding();
        let ctx = ExplainContext::default();
        let explain = build_explanation(&finding, &ctx);
        assert!(explain.issue_context.is_none());
    }

    #[test]
    fn test_issue_context_in_serialized_json() {
        let finding = test_finding();
        let ctx = ExplainContext {
            issue_context_included_count: 2,
            issue_context_sources: vec!["github:issue:#5".into(), "jira:issue:CORE-10".into()],
            ..Default::default()
        };
        let explain = build_explanation(&finding, &ctx);
        let json = to_json(&explain).unwrap();
        assert!(json.contains("issue_context"));
        assert!(json.contains("included_count"));
        let parsed: FindingExplanation = serde_json::from_str(&json).unwrap();
        let ic = parsed.issue_context.unwrap();
        assert_eq!(ic.included_count, 2);
    }
}
