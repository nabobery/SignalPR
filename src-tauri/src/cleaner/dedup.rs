use crate::storage::models::Finding;
use std::collections::HashSet;

/// Jaccard word-set similarity deduplication (Phase 1 fallback).
/// For semantic embedding-based dedup, see `dedup_semantic` (WS3/WS4).
pub fn dedup_jaccard(findings: Vec<Finding>, threshold: f64) -> Vec<Finding> {
    if findings.len() <= 1 {
        return findings;
    }

    let mut merged_indices: HashSet<usize> = HashSet::new();
    let mut result: Vec<Finding> = Vec::new();

    for i in 0..findings.len() {
        if merged_indices.contains(&i) {
            continue;
        }

        let mut current = findings[i].clone();
        let mut merged_bodies: Vec<String> = vec![];

        #[allow(clippy::needless_range_loop)]
        for j in (i + 1)..findings.len() {
            if merged_indices.contains(&j) {
                continue;
            }

            // Only merge findings with same file_path and severity
            if current.file_path != findings[j].file_path
                || current.severity != findings[j].severity
            {
                continue;
            }

            // Check line range overlap if both have anchors
            if !ranges_overlap_or_absent(&current, &findings[j]) {
                continue;
            }

            let sim = jaccard_similarity(
                &format!("{} {}", current.title, current.body),
                &format!("{} {}", findings[j].title, findings[j].body),
            );

            if sim >= threshold {
                // Merge: keep highest confidence, combine evidence
                if findings[j].confidence > current.confidence {
                    current.confidence = findings[j].confidence;
                }
                // Expand line range to cover both
                if let (Some(cs), Some(js)) = (current.line_start, findings[j].line_start) {
                    current.line_start = Some(cs.min(js));
                }
                if let (Some(ce), Some(je)) = (current.line_end, findings[j].line_end) {
                    current.line_end = Some(ce.max(je));
                }
                merged_bodies.push(findings[j].body.clone());
                merged_indices.insert(j);
            }
        }

        if !merged_bodies.is_empty() {
            // Append merged context to body
            for extra in &merged_bodies {
                if !current.body.contains(extra.as_str()) {
                    current.body = format!("{}\n\nAlso: {}", current.body, extra);
                }
            }
        }

        result.push(current);
    }

    result
}

fn ranges_overlap_or_absent(a: &Finding, b: &Finding) -> bool {
    match (a.line_start, a.line_end, b.line_start, b.line_end) {
        (Some(as_), Some(ae), Some(bs), Some(be)) => as_ <= be && bs <= ae,
        _ => true, // If either lacks line range, allow merge based on text similarity
    }
}

fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let words_a: HashSet<String> = a.split_whitespace().map(|w| w.to_lowercase()).collect();
    let words_b: HashSet<String> = b.split_whitespace().map(|w| w.to_lowercase()).collect();

    if words_a.is_empty() && words_b.is_empty() {
        return 1.0;
    }

    let intersection = words_a.intersection(&words_b).count();
    let union = words_a.union(&words_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(
        id: &str,
        file: Option<&str>,
        severity: &str,
        title: &str,
        body: &str,
        line_start: Option<i32>,
        line_end: Option<i32>,
        confidence: f64,
    ) -> Finding {
        Finding {
            id: id.to_string(),
            review_run_id: "run".to_string(),
            agent_type: "security".to_string(),
            file_path: file.map(|s| s.to_string()),
            line_start,
            line_end,
            severity: severity.to_string(),
            confidence,
            title: title.to_string(),
            body: body.to_string(),
            evidence: None,
            status: "active".to_string(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: file.is_some() && line_start.is_some(),
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
        }
    }

    #[test]
    fn test_identical_findings_merged() {
        let findings = vec![
            make_finding(
                "f1",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed on route",
                Some(10),
                Some(20),
                0.9,
            ),
            make_finding(
                "f2",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed on route",
                Some(10),
                Some(20),
                0.8,
            ),
        ];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].confidence, 0.9); // keeps highest
    }

    #[test]
    fn test_near_duplicates_merged() {
        let findings = vec![
            make_finding(
                "f1",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk in auth",
                "Authentication middleware is bypassed",
                Some(10),
                Some(20),
                0.9,
            ),
            make_finding(
                "f2",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass vulnerability in auth",
                "Authentication middleware is completely bypassed",
                Some(15),
                Some(25),
                0.85,
            ),
        ];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 1);
        // Line range should expand
        assert_eq!(result[0].line_start, Some(10));
        assert_eq!(result[0].line_end, Some(25));
    }

    #[test]
    fn test_different_files_not_merged() {
        let findings = vec![
            make_finding(
                "f1",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed",
                Some(10),
                Some(20),
                0.9,
            ),
            make_finding(
                "f2",
                Some("src/routes.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed",
                Some(10),
                Some(20),
                0.8,
            ),
        ];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_different_severity_not_merged() {
        let findings = vec![
            make_finding(
                "f1",
                Some("src/auth.rs"),
                "blocker",
                "Token issue",
                "Auth problem",
                Some(10),
                Some(20),
                0.9,
            ),
            make_finding(
                "f2",
                Some("src/auth.rs"),
                "warning",
                "Token issue",
                "Auth problem",
                Some(10),
                Some(20),
                0.8,
            ),
        ];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_low_similarity_not_merged() {
        let findings = vec![
            make_finding(
                "f1",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed",
                Some(10),
                Some(20),
                0.9,
            ),
            make_finding(
                "f2",
                Some("src/auth.rs"),
                "blocker",
                "N+1 query pattern detected in user loader",
                "Database queries are inefficient",
                Some(10),
                Some(20),
                0.8,
            ),
        ];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_single_finding_unchanged() {
        let findings = vec![make_finding(
            "f1",
            Some("src/auth.rs"),
            "blocker",
            "Token bypass",
            "Auth bypassed",
            Some(10),
            Some(20),
            0.9,
        )];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_empty_input() {
        let result = dedup_jaccard(vec![], 0.70);
        assert!(result.is_empty());
    }

    #[test]
    fn test_non_overlapping_lines_not_merged() {
        let findings = vec![
            make_finding(
                "f1",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed on route",
                Some(10),
                Some(15),
                0.9,
            ),
            make_finding(
                "f2",
                Some("src/auth.rs"),
                "blocker",
                "Token bypass risk",
                "Auth is bypassed on route",
                Some(100),
                Some(110),
                0.8,
            ),
        ];
        let result = dedup_jaccard(findings, 0.70);
        assert_eq!(result.len(), 2);
    }
}
