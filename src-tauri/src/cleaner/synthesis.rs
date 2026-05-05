use crate::storage::models::Finding;

/// A cluster of related findings with a synthesized representative.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FindingCluster {
    pub id: String,
    pub review_run_id: String,
    pub representative: Finding,
    pub members: Vec<Finding>,
    pub label: Option<String>,
    pub member_count: usize,
}

/// Build clusters from deduplicated findings. Each finding becomes a single-member
/// cluster (Jaccard dedup already merged duplicates into one Finding).
/// For multi-member clusters, use `cluster_findings` with semantic embeddings (WS3).
#[allow(dead_code)]
pub fn wrap_as_clusters(findings: Vec<Finding>, review_run_id: &str) -> Vec<FindingCluster> {
    findings
        .into_iter()
        .map(|f| {
            let id = format!("cluster_{}", f.id);
            FindingCluster {
                id,
                review_run_id: review_run_id.to_string(),
                representative: f.clone(),
                members: vec![f],
                label: None,
                member_count: 1,
            }
        })
        .collect()
}

/// Cluster findings by grouping those with the same file_path and similar content.
/// Uses Jaccard similarity as a pre-filter for clustering (semantic embeddings in WS3).
pub fn cluster_findings(
    findings: Vec<Finding>,
    review_run_id: &str,
    similarity_threshold: f64,
) -> Vec<FindingCluster> {
    if findings.is_empty() {
        return vec![];
    }

    let mut assigned: Vec<bool> = vec![false; findings.len()];
    let mut clusters: Vec<FindingCluster> = Vec::new();

    for i in 0..findings.len() {
        if assigned[i] {
            continue;
        }
        assigned[i] = true;

        let mut members = vec![findings[i].clone()];

        // Only compare within same file_path or same agent_type bucket
        for j in (i + 1)..findings.len() {
            if assigned[j] {
                continue;
            }

            let same_file =
                findings[i].file_path == findings[j].file_path && findings[i].file_path.is_some();
            let same_agent = findings[i].agent_type == findings[j].agent_type;

            if !same_file && !same_agent {
                continue;
            }

            let sim = jaccard_similarity(
                &format!("{} {}", findings[i].title, findings[i].body),
                &format!("{} {}", findings[j].title, findings[j].body),
            );

            if sim >= similarity_threshold {
                assigned[j] = true;
                members.push(findings[j].clone());
            }
        }

        let representative = synthesize_representative(&members);
        let member_count = members.len();

        // Deterministic cluster ID: hash of sorted member IDs
        let mut member_ids: Vec<&str> = members.iter().map(|m| m.id.as_str()).collect();
        member_ids.sort();
        let id = format!(
            "cluster_{}",
            &deterministic_hash(&member_ids.join(","))[..16]
        );

        let label = if member_count > 1 {
            Some(representative.title.clone())
        } else {
            None
        };

        clusters.push(FindingCluster {
            id,
            review_run_id: review_run_id.to_string(),
            representative,
            members,
            label,
            member_count,
        });
    }

    clusters
}

/// Synthesize a representative finding from a cluster of members.
/// Uses heuristic approach: highest-confidence member as base, merge evidence.
fn synthesize_representative(members: &[Finding]) -> Finding {
    assert!(!members.is_empty());

    if members.len() == 1 {
        return members[0].clone();
    }

    // Pick highest-confidence member as representative (NaN-safe: prefer finite values)
    let best = members
        .iter()
        .max_by(
            |a, b| match (a.confidence.is_nan(), b.confidence.is_nan()) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.confidence.total_cmp(&b.confidence),
            },
        )
        .unwrap();

    let mut representative = best.clone();

    // Merge evidence from all members (HashSet-based dedup for true uniqueness)
    use std::collections::HashSet;
    let evidence_set: HashSet<String> = members
        .iter()
        .filter_map(|m| m.evidence.as_ref())
        .flat_map(|ev| serde_json::from_str::<Vec<String>>(ev).unwrap_or_default())
        .collect();
    let all_evidence: Vec<String> = evidence_set.into_iter().collect();
    if !all_evidence.is_empty() {
        representative.evidence = Some(serde_json::to_string(&all_evidence).unwrap_or_default());
    }

    // Add synthesis note to body
    let other_count = members.len() - 1;
    representative.body = format!(
        "{}\n\nAdditionally found in {} other location{}.",
        representative.body,
        other_count,
        if other_count == 1 { "" } else { "s" }
    );

    // Use max confidence across all members (NaN-safe: skip NaN values)
    representative.confidence = members
        .iter()
        .map(|m| m.confidence)
        .filter(|c| !c.is_nan())
        .fold(0.0_f64, |a, b| {
            if b.total_cmp(&a) == std::cmp::Ordering::Greater {
                b
            } else {
                a
            }
        });

    // Preserve best anchor (prefer anchored representative)
    if !representative.is_anchored {
        if let Some(anchored) = members.iter().find(|m| m.is_anchored) {
            representative.file_path = anchored.file_path.clone();
            representative.line_start = anchored.line_start;
            representative.line_end = anchored.line_end;
            representative.is_anchored = true;
        }
    }

    representative
}

fn jaccard_similarity(a: &str, b: &str) -> f64 {
    use std::collections::HashSet;
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

/// Simple rolling hash for deterministic cluster IDs (not cryptographic).
fn deterministic_hash(input: &str) -> String {
    use std::fmt::Write;
    let mut hash: u64 = 0;
    for byte in input.bytes() {
        hash = hash.wrapping_mul(31).wrapping_add(byte as u64);
    }
    let mut s = String::with_capacity(16);
    write!(s, "{:016x}", hash).unwrap();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_finding(
        id: &str,
        file: Option<&str>,
        title: &str,
        body: &str,
        confidence: f64,
        agent: &str,
    ) -> Finding {
        Finding {
            id: id.to_string(),
            review_run_id: "run".to_string(),
            agent_type: agent.to_string(),
            file_path: file.map(|s| s.to_string()),
            line_start: Some(10),
            line_end: Some(20),
            severity: "warning".to_string(),
            confidence,
            title: title.to_string(),
            body: body.to_string(),
            evidence: None,
            status: "active".to_string(),
            user_edited_body: None,
            user_severity_override: None,
            is_anchored: file.is_some(),
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
    fn test_wrap_as_clusters_single_members() {
        let findings = vec![
            make_finding("f1", Some("a.rs"), "Bug A", "body", 0.9, "security"),
            make_finding("f2", Some("b.rs"), "Bug B", "body", 0.8, "performance"),
        ];
        let clusters = wrap_as_clusters(findings, "run-1");
        assert_eq!(clusters.len(), 2);
        assert_eq!(clusters[0].member_count, 1);
        assert_eq!(clusters[1].member_count, 1);
        assert!(clusters[0].label.is_none());
    }

    #[test]
    fn test_cluster_similar_findings() {
        let findings = vec![
            make_finding(
                "f1",
                Some("auth.rs"),
                "Token bypass risk",
                "Auth middleware bypassed",
                0.9,
                "security",
            ),
            make_finding(
                "f2",
                Some("auth.rs"),
                "Token bypass vulnerability",
                "Auth middleware is bypassed completely",
                0.85,
                "security",
            ),
            make_finding(
                "f3",
                Some("db.rs"),
                "N+1 query pattern",
                "Database loop detected",
                0.7,
                "performance",
            ),
        ];
        let clusters = cluster_findings(findings, "run-1", 0.50);
        // f1 and f2 should cluster, f3 separate
        assert_eq!(clusters.len(), 2);

        let big_cluster = clusters.iter().find(|c| c.member_count > 1).unwrap();
        assert_eq!(big_cluster.member_count, 2);
        assert!(big_cluster
            .representative
            .body
            .contains("Additionally found"));
        assert_eq!(big_cluster.representative.confidence, 0.9); // max
        assert!(big_cluster.label.is_some());
    }

    #[test]
    fn test_cluster_different_files_same_agent() {
        let findings = vec![
            make_finding(
                "f1",
                Some("a.rs"),
                "Token bypass risk",
                "Auth bypassed",
                0.9,
                "security",
            ),
            make_finding(
                "f2",
                Some("b.rs"),
                "Token bypass risk",
                "Auth bypassed",
                0.8,
                "security",
            ),
        ];
        // Same agent_type, high similarity → should cluster
        let clusters = cluster_findings(findings, "run-1", 0.70);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].member_count, 2);
    }

    #[test]
    fn test_cluster_no_merge_different_topics() {
        let findings = vec![
            make_finding(
                "f1",
                Some("a.rs"),
                "SQL injection risk",
                "User input not sanitized",
                0.9,
                "security",
            ),
            make_finding(
                "f2",
                Some("b.rs"),
                "N+1 query loop",
                "Database queries in loop",
                0.8,
                "performance",
            ),
        ];
        let clusters = cluster_findings(findings, "run-1", 0.50);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn test_cluster_deterministic_ids() {
        let findings = vec![
            make_finding("f1", Some("a.rs"), "Bug", "body", 0.9, "security"),
            make_finding("f2", Some("a.rs"), "Bug", "body", 0.8, "security"),
        ];
        let c1 = cluster_findings(findings.clone(), "run-1", 0.70);
        let c2 = cluster_findings(findings, "run-1", 0.70);
        assert_eq!(c1[0].id, c2[0].id);
    }

    #[test]
    fn test_synthesize_preserves_best_anchor() {
        let mut f1 = make_finding("f1", None, "Bug", "body1", 0.9, "security");
        f1.is_anchored = false;
        f1.file_path = None;
        f1.line_start = None;
        f1.line_end = None;

        let f2 = make_finding("f2", Some("auth.rs"), "Bug", "body2", 0.8, "security");

        let rep = synthesize_representative(&[f1, f2]);
        assert!(rep.is_anchored);
        assert_eq!(rep.file_path.as_deref(), Some("auth.rs"));
    }

    #[test]
    fn test_empty_input() {
        let clusters = cluster_findings(vec![], "run-1", 0.70);
        assert!(clusters.is_empty());
    }

    #[test]
    fn test_duplicate_evidence_deduped() {
        let mut f1 = make_finding("f1", Some("a.rs"), "Bug", "body", 0.9, "security");
        f1.evidence = Some(r#"["line 10 is bad","shared evidence","line 10 is bad"]"#.to_string());

        let mut f2 = make_finding("f2", Some("a.rs"), "Bug", "body", 0.8, "security");
        f2.evidence = Some(r#"["shared evidence","unique to f2"]"#.to_string());

        let rep = synthesize_representative(&[f1, f2]);
        let evidence: Vec<String> = serde_json::from_str(rep.evidence.as_ref().unwrap()).unwrap();
        // All evidence strings should be unique
        let mut seen = std::collections::HashSet::new();
        for e in &evidence {
            assert!(seen.insert(e.clone()), "Duplicate evidence found: {}", e);
        }
        // Should contain exactly 3 unique strings
        assert_eq!(evidence.len(), 3);
    }

    #[test]
    fn test_nan_confidence_no_panic() {
        let f1 = make_finding("f1", Some("a.rs"), "Bug A", "body", f64::NAN, "security");
        let f2 = make_finding("f2", Some("a.rs"), "Bug A", "body", 0.8, "security");
        let f3 = make_finding("f3", Some("a.rs"), "Bug A", "body", f64::NAN, "security");

        // Should not panic despite NaN confidence values
        let rep = synthesize_representative(&[f1, f2, f3]);
        assert!(
            rep.confidence.is_finite(),
            "Result confidence should be finite"
        );
        assert_eq!(rep.confidence, 0.8);
    }
}
