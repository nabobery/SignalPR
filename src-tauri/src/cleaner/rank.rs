use crate::cleaner::CleanerConfig;
use crate::storage::models::Finding;

pub fn rank_and_suppress(
    findings: Vec<Finding>,
    config: &CleanerConfig,
) -> (Vec<Finding>, Vec<Finding>) {
    let mut surfaced: Vec<Finding> = Vec::new();
    let mut dropped: Vec<Finding> = Vec::new();

    for f in findings {
        let effective_severity = f.user_severity_override.as_deref().unwrap_or(&f.severity);

        // Suppress nitpicks if configured
        if config.drop_nitpicks && effective_severity == "nitpick" {
            let mut suppressed = f;
            suppressed.status = "suppressed".to_string();
            dropped.push(suppressed);
            continue;
        }

        // Suppress low-confidence findings
        if f.confidence < config.min_confidence {
            let mut suppressed = f;
            suppressed.status = "suppressed".to_string();
            dropped.push(suppressed);
            continue;
        }

        surfaced.push(f);
    }

    // Sort surfaced by severity then confidence
    surfaced.sort_by(|a, b| {
        let sa = severity_weight(&a.severity);
        let sb = severity_weight(&b.severity);
        sb.cmp(&sa).then(
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });

    // Apply max findings cap
    if surfaced.len() > config.max_surface_findings {
        let excess = surfaced.split_off(config.max_surface_findings);
        for mut f in excess {
            f.status = "suppressed".to_string();
            dropped.push(f);
        }
    }

    (surfaced, dropped)
}

fn severity_weight(s: &str) -> u8 {
    match s {
        "blocker" => 5,
        "critical" => 4,
        "warning" => 3,
        "info" => 2,
        "nitpick" => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(id: &str, severity: &str, confidence: f64) -> Finding {
        Finding {
            id: id.to_string(),
            review_run_id: "run".to_string(),
            agent_type: "security".to_string(),
            file_path: Some("file.rs".to_string()),
            line_start: Some(1),
            line_end: Some(5),
            severity: severity.to_string(),
            confidence,
            title: format!("{} finding", severity),
            body: "body".to_string(),
            evidence: None,
            status: "active".to_string(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: true,
            created_at: "2026-01-01".to_string(),
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
        }
    }

    #[test]
    fn test_severity_ordering() {
        let findings = vec![
            make_finding("f1", "info", 0.8),
            make_finding("f2", "blocker", 0.9),
            make_finding("f3", "warning", 0.7),
            make_finding("f4", "critical", 0.85),
        ];
        let config = CleanerConfig {
            drop_nitpicks: false,
            min_confidence: 0.0,
            max_surface_findings: 10,
            ..Default::default()
        };
        let (surfaced, _) = rank_and_suppress(findings, &config);
        assert_eq!(surfaced[0].severity, "blocker");
        assert_eq!(surfaced[1].severity, "critical");
        assert_eq!(surfaced[2].severity, "warning");
        assert_eq!(surfaced[3].severity, "info");
    }

    #[test]
    fn test_confidence_tiebreak() {
        let findings = vec![
            make_finding("f1", "warning", 0.7),
            make_finding("f2", "warning", 0.95),
            make_finding("f3", "warning", 0.8),
        ];
        let config = CleanerConfig {
            drop_nitpicks: false,
            min_confidence: 0.0,
            max_surface_findings: 10,
            ..Default::default()
        };
        let (surfaced, _) = rank_and_suppress(findings, &config);
        assert_eq!(surfaced[0].id, "f2"); // highest confidence
        assert_eq!(surfaced[1].id, "f3");
        assert_eq!(surfaced[2].id, "f1");
    }

    #[test]
    fn test_nitpick_suppression() {
        let findings = vec![
            make_finding("f1", "blocker", 0.9),
            make_finding("f2", "nitpick", 0.8),
        ];
        let config = CleanerConfig::default(); // drop_nitpicks: true
        let (surfaced, dropped) = rank_and_suppress(findings, &config);
        assert_eq!(surfaced.len(), 1);
        assert_eq!(surfaced[0].severity, "blocker");
        assert_eq!(dropped.len(), 1);
        assert_eq!(dropped[0].severity, "nitpick");
        assert_eq!(dropped[0].status, "suppressed");
    }

    #[test]
    fn test_low_confidence_suppression() {
        let findings = vec![
            make_finding("f1", "warning", 0.9),
            make_finding("f2", "warning", 0.1),
            make_finding("f3", "warning", 0.2),
        ];
        let config = CleanerConfig::default(); // min_confidence: 0.3
        let (surfaced, dropped) = rank_and_suppress(findings, &config);
        assert_eq!(surfaced.len(), 1);
        assert_eq!(dropped.len(), 2);
    }

    #[test]
    fn test_max_findings_cap() {
        let findings: Vec<Finding> = (0..15)
            .map(|i| make_finding(&format!("f{}", i), "warning", 0.5 + (i as f64) * 0.03))
            .collect();
        let config = CleanerConfig {
            max_surface_findings: 8,
            drop_nitpicks: false,
            min_confidence: 0.0,
            ..Default::default()
        };
        let (surfaced, dropped) = rank_and_suppress(findings, &config);
        assert_eq!(surfaced.len(), 8);
        assert_eq!(dropped.len(), 7);
    }

    #[test]
    fn test_empty_input() {
        let config = CleanerConfig::default();
        let (surfaced, dropped) = rank_and_suppress(vec![], &config);
        assert!(surfaced.is_empty());
        assert!(dropped.is_empty());
    }

    #[test]
    fn test_nitpick_kept_when_not_dropped() {
        let findings = vec![make_finding("f1", "nitpick", 0.8)];
        let config = CleanerConfig {
            drop_nitpicks: false,
            min_confidence: 0.0,
            max_surface_findings: 10,
            ..Default::default()
        };
        let (surfaced, _) = rank_and_suppress(findings, &config);
        assert_eq!(surfaced.len(), 1);
    }
}
