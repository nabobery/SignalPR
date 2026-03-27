use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use crate::storage::models::{PreferenceSummary, ReviewerDecision};

const DECAY_FACTOR: f64 = 0.95;

/// Returns true if the decision counts as "accepted" (accept or edit).
fn is_accept(decision: &str) -> bool {
    matches!(decision, "accept" | "edit")
}

/// Compute preference summaries for each (agent_type, category_tag) pair.
///
/// For each pair:
///   accept_rate = sum(weight_i * is_accept_i) / sum(weight_i)
///   where weight_i = 0.95^(days_since_decision)
///
/// "accept" and "edit" count as accept; "reject" and "skip" count as reject.
pub fn compute_preference_summaries(decisions: &[ReviewerDecision]) -> Vec<PreferenceSummary> {
    if decisions.is_empty() {
        return vec![];
    }

    let now = Utc::now();

    // Group by (agent_type, category_tag)
    let mut groups: HashMap<(String, Option<String>), Vec<&ReviewerDecision>> = HashMap::new();
    for d in decisions {
        let key = (d.original_agent_type.clone(), d.category_tag.clone());
        groups.entry(key).or_default().push(d);
    }

    let mut summaries = Vec::new();
    for ((agent_type, category_tag), group) in &groups {
        let mut weighted_accept = 0.0_f64;
        let mut weight_sum = 0.0_f64;

        for d in group {
            let decided_at = chrono::DateTime::parse_from_rfc3339(&d.decided_at)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or(now);
            let days_since = (now - decided_at).num_seconds().max(0) as f64 / 86400.0;
            let weight = DECAY_FACTOR.powf(days_since);

            weight_sum += weight;
            if is_accept(&d.decision) {
                weighted_accept += weight;
            }
        }

        let accept_rate = if weight_sum > 0.0 {
            weighted_accept / weight_sum
        } else {
            0.0
        };

        summaries.push(PreferenceSummary {
            id: Uuid::new_v4().to_string(),
            agent_type: agent_type.clone(),
            category_tag: category_tag.clone(),
            accept_rate,
            total_decisions: group.len() as i32,
            last_updated: now.to_rfc3339(),
        });
    }

    // Sort for deterministic output
    summaries.sort_by(|a, b| {
        a.agent_type
            .cmp(&b.agent_type)
            .then_with(|| a.category_tag.cmp(&b.category_tag))
    });

    summaries
}

/// Generate a text block to inject into LLM system prompts based on preference summaries.
///
/// Summaries with low accept rates get "deprioritize" guidance;
/// summaries with high accept rates get "prioritize" guidance.
/// Returns None if there are no summaries with enough data.
pub fn build_preference_prompt_section(summaries: &[PreferenceSummary]) -> Option<String> {
    if summaries.is_empty() {
        return None;
    }

    // Only include summaries with at least 3 decisions for statistical relevance
    let relevant: Vec<&PreferenceSummary> = summaries
        .iter()
        .filter(|s| s.total_decisions >= 3)
        .collect();

    if relevant.is_empty() {
        return None;
    }

    let mut lines = vec!["## Reviewer Preference History".to_string(), String::new()];

    for s in &relevant {
        let category_label = s.category_tag.as_deref().unwrap_or("general");
        let pct = (s.accept_rate * 100.0).round() as i32;

        if s.accept_rate < 0.3 {
            lines.push(format!(
                "The reviewer tends to reject findings about '{}' from {} agent (accept rate: {}%). \
                 Deprioritize these unless severity is critical or above.",
                category_label, s.agent_type, pct
            ));
        } else if s.accept_rate > 0.8 {
            lines.push(format!(
                "The reviewer frequently accepts findings about '{}' from {} agent (accept rate: {}%). \
                 These are high-value — prioritize similar findings.",
                category_label, s.agent_type, pct
            ));
        } else {
            lines.push(format!(
                "The reviewer has mixed responses to findings about '{}' from {} agent (accept rate: {}%).",
                category_label, s.agent_type, pct
            ));
        }
    }

    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::models::ReviewerDecision;

    fn make_decision(
        agent_type: &str,
        category: Option<&str>,
        decision: &str,
        days_ago: i64,
    ) -> ReviewerDecision {
        let decided_at = (Utc::now() - chrono::Duration::days(days_ago)).to_rfc3339();
        ReviewerDecision {
            id: Uuid::new_v4().to_string(),
            finding_id: "f1".into(),
            review_run_id: "run1".into(),
            decision: decision.into(),
            original_severity: "warning".into(),
            original_agent_type: agent_type.into(),
            category_tag: category.map(|s| s.into()),
            time_to_decision_ms: Some(1000),
            decided_at,
        }
    }

    #[test]
    fn test_empty_decisions_returns_empty() {
        let result = compute_preference_summaries(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn test_recent_decisions_weighted_higher_than_old() {
        // Recent accept + old reject => accept rate should be > 0.5
        let decisions = vec![
            make_decision("security", Some("auth"), "accept", 0), // today, weight ~1.0
            make_decision("security", Some("auth"), "reject", 60), // 60 days ago, weight ~0.046
        ];
        let summaries = compute_preference_summaries(&decisions);
        assert_eq!(summaries.len(), 1);
        let s = &summaries[0];
        assert_eq!(s.agent_type, "security");
        assert_eq!(s.category_tag, Some("auth".into()));
        assert!(
            s.accept_rate > 0.9,
            "Recent accept should dominate: got {}",
            s.accept_rate
        );
        assert_eq!(s.total_decisions, 2);
    }

    #[test]
    fn test_all_accepts_gives_rate_1() {
        let decisions = vec![
            make_decision("security", Some("auth"), "accept", 0),
            make_decision("security", Some("auth"), "edit", 1),
        ];
        let summaries = compute_preference_summaries(&decisions);
        assert_eq!(summaries.len(), 1);
        assert!((summaries[0].accept_rate - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_all_rejects_gives_rate_0() {
        let decisions = vec![
            make_decision("security", Some("auth"), "reject", 0),
            make_decision("security", Some("auth"), "skip", 1),
        ];
        let summaries = compute_preference_summaries(&decisions);
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].accept_rate < 0.001);
    }

    #[test]
    fn test_multiple_groups() {
        let decisions = vec![
            make_decision("security", Some("auth"), "accept", 0),
            make_decision("performance", Some("n+1"), "reject", 0),
        ];
        let summaries = compute_preference_summaries(&decisions);
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_none_category_grouped_separately() {
        let decisions = vec![
            make_decision("security", Some("auth"), "accept", 0),
            make_decision("security", None, "reject", 0),
        ];
        let summaries = compute_preference_summaries(&decisions);
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn test_build_preference_prompt_section_empty() {
        assert_eq!(build_preference_prompt_section(&[]), None);
    }

    #[test]
    fn test_build_preference_prompt_section_too_few_decisions() {
        let summaries = vec![PreferenceSummary {
            id: "s1".into(),
            agent_type: "security".into(),
            category_tag: Some("auth".into()),
            accept_rate: 0.1,
            total_decisions: 2, // below threshold of 3
            last_updated: Utc::now().to_rfc3339(),
        }];
        assert_eq!(build_preference_prompt_section(&summaries), None);
    }

    #[test]
    fn test_build_preference_prompt_section_low_accept_rate() {
        let summaries = vec![PreferenceSummary {
            id: "s1".into(),
            agent_type: "security".into(),
            category_tag: Some("auth".into()),
            accept_rate: 0.15,
            total_decisions: 10,
            last_updated: Utc::now().to_rfc3339(),
        }];
        let result = build_preference_prompt_section(&summaries).unwrap();
        assert!(
            result.contains("Deprioritize"),
            "Should contain deprioritize text: {}",
            result
        );
        assert!(
            result.contains("15%"),
            "Should contain accept rate: {}",
            result
        );
        assert!(
            result.contains("auth"),
            "Should contain category: {}",
            result
        );
        assert!(
            result.contains("security"),
            "Should contain agent type: {}",
            result
        );
    }

    #[test]
    fn test_build_preference_prompt_section_high_accept_rate() {
        let summaries = vec![PreferenceSummary {
            id: "s1".into(),
            agent_type: "performance".into(),
            category_tag: Some("n+1".into()),
            accept_rate: 0.95,
            total_decisions: 20,
            last_updated: Utc::now().to_rfc3339(),
        }];
        let result = build_preference_prompt_section(&summaries).unwrap();
        assert!(
            result.contains("prioritize"),
            "Should contain prioritize text: {}",
            result
        );
        assert!(result.contains("95%"));
    }

    #[test]
    fn test_build_preference_prompt_section_mixed() {
        let summaries = vec![PreferenceSummary {
            id: "s1".into(),
            agent_type: "architecture".into(),
            category_tag: None,
            accept_rate: 0.55,
            total_decisions: 8,
            last_updated: Utc::now().to_rfc3339(),
        }];
        let result = build_preference_prompt_section(&summaries).unwrap();
        assert!(
            result.contains("mixed"),
            "Should contain mixed text: {}",
            result
        );
    }
}
