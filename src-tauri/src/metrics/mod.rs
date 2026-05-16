use std::collections::HashMap;

use rusqlite::Connection;
use serde::{Deserialize, Deserializer, Serialize};

use crate::storage::models::{AgentRun, Finding, ReviewerDecision};
use crate::storage::queries;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaneScorecard {
    pub lane_id: String,
    pub provider_name: String,
    pub lane_latency_ms: Option<i64>,
    pub raw_findings_count: i32,
    pub surfaced_findings_count: i32,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub reviewer_accept_rate: f64,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub reviewer_edit_rate: f64,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub suppress_rate: f64,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub anchor_validity: f64,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub submission_inclusion_rate: f64,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunScorecard {
    pub lanes: Vec<LaneScorecard>,
    #[serde(default, alias = "total_surfaced_findings")]
    pub overall_surfaced: i32,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub overall_accept_rate: f64,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub overall_edit_rate: f64,
    #[serde(default, deserialize_with = "de_f64_or_default")]
    pub overall_suppress_rate: f64,
    pub total_cost_usd: Option<f64>,
}

fn de_f64_or_default<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<f64>::deserialize(deserializer)?.unwrap_or(0.0))
}

/// Compute the run scorecard from DB data for the given run.
pub fn compute_run_scorecard(
    conn: &Connection,
    run_id: &str,
) -> Result<RunScorecard, rusqlite::Error> {
    let agent_runs = queries::get_agent_runs_for_review(conn, run_id)?;
    let findings = queries::get_findings_for_run(conn, run_id)?;
    let decisions = queries::get_decisions_for_run(conn, run_id)?;

    Ok(build_scorecard(&agent_runs, &findings, &decisions))
}

/// Store the scorecard JSON cache on the review_runs row.
pub fn store_run_scorecard_cache(
    conn: &Connection,
    run_id: &str,
    scorecard: &RunScorecard,
) -> Result<(), rusqlite::Error> {
    let json = serde_json::to_string(scorecard)
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
    queries::update_review_run_metrics(conn, run_id, &json)
}

fn build_scorecard(
    agent_runs: &[AgentRun],
    findings: &[Finding],
    decisions: &[ReviewerDecision],
) -> RunScorecard {
    // Build latest decision per finding
    let mut latest_decision: HashMap<&str, &str> = HashMap::new();
    for d in decisions {
        latest_decision
            .entry(d.finding_id.as_str())
            .or_insert(d.decision.as_str());
    }
    // decisions are ordered DESC by decided_at, so first insert wins (latest)

    let mut lanes: Vec<LaneScorecard> = Vec::new();

    // Group agent_runs by (lane_id, provider_name)
    let mut lane_groups: HashMap<(&str, &str), Vec<&AgentRun>> = HashMap::new();
    for ar in agent_runs {
        lane_groups
            .entry((ar.lane_id.as_str(), ar.provider_name.as_str()))
            .or_default()
            .push(ar);
    }

    for ((lane_id, provider_name), runs) in &lane_groups {
        let raw_count: i32 = runs.iter().map(|r| r.finding_count).sum();

        let lane_findings: Vec<&Finding> = findings
            .iter()
            .filter(|f| f.lane_id.as_deref() == Some(lane_id))
            .collect();
        let surfaced = lane_findings.len() as i32;

        let latency_ms = runs.iter().find_map(|r| {
            let started = r.started_at.as_ref()?;
            let completed = r.completed_at.as_ref()?;
            let start = chrono::DateTime::parse_from_rfc3339(started).ok()?;
            let end = chrono::DateTime::parse_from_rfc3339(completed).ok()?;
            Some((end - start).num_milliseconds())
        });

        let cost: Option<f64> = {
            let total: f64 = runs.iter().filter_map(|r| r.cost_usd).sum();
            if total > 0.0 {
                Some(total)
            } else {
                None
            }
        };

        let (accept_count, edit_count, suppress_count, anchored_count, included_count) =
            compute_lane_rates(&lane_findings, &latest_decision);

        let accept_rate = if surfaced > 0 {
            accept_count as f64 / surfaced as f64
        } else {
            0.0
        };
        let edit_rate = if surfaced > 0 {
            edit_count as f64 / surfaced as f64
        } else {
            0.0
        };
        let suppress_rate = if surfaced > 0 {
            suppress_count as f64 / surfaced as f64
        } else {
            0.0
        };
        let anchor_rate = if surfaced > 0 {
            anchored_count as f64 / surfaced as f64
        } else {
            0.0
        };
        let inclusion_rate = if surfaced > 0 {
            included_count as f64 / surfaced as f64
        } else {
            0.0
        };

        lanes.push(LaneScorecard {
            lane_id: lane_id.to_string(),
            provider_name: provider_name.to_string(),
            lane_latency_ms: latency_ms,
            raw_findings_count: raw_count,
            surfaced_findings_count: surfaced,
            reviewer_accept_rate: accept_rate,
            reviewer_edit_rate: edit_rate,
            suppress_rate,
            anchor_validity: anchor_rate,
            submission_inclusion_rate: inclusion_rate,
            cost_usd: cost,
        });
    }

    lanes.sort_by(|a, b| a.lane_id.cmp(&b.lane_id));

    let total_surfaced: i32 = lanes.iter().map(|l| l.surfaced_findings_count).sum();
    let total_cost: Option<f64> = {
        let sum: f64 = lanes.iter().filter_map(|l| l.cost_usd).sum();
        if sum > 0.0 {
            Some(sum)
        } else {
            None
        }
    };

    let (overall_accept, overall_edit, overall_suppress, _, _) =
        compute_lane_rates(&findings.iter().collect::<Vec<_>>(), &latest_decision);
    let overall_accept_rate = if total_surfaced > 0 {
        overall_accept as f64 / total_surfaced as f64
    } else {
        0.0
    };
    let overall_edit_rate = if total_surfaced > 0 {
        overall_edit as f64 / total_surfaced as f64
    } else {
        0.0
    };
    let overall_suppress_rate = if total_surfaced > 0 {
        overall_suppress as f64 / total_surfaced as f64
    } else {
        0.0
    };

    RunScorecard {
        lanes,
        overall_surfaced: total_surfaced,
        overall_accept_rate,
        overall_edit_rate,
        overall_suppress_rate,
        total_cost_usd: total_cost,
    }
}

fn compute_lane_rates(
    findings: &[&Finding],
    latest_decision: &HashMap<&str, &str>,
) -> (i32, i32, i32, i32, i32) {
    let mut accept = 0;
    let mut edit = 0;
    let mut suppress = 0;
    let mut anchored = 0;
    let mut included = 0;

    for f in findings {
        let decision = latest_decision.get(f.id.as_str()).copied();

        if decision == Some("accept") || decision == Some("edit") {
            accept += 1;
        }
        if decision == Some("edit")
            || f.user_edited_body.is_some()
            || f.user_severity_override.is_some()
        {
            edit += 1;
        }
        if f.status == "suppressed" {
            suppress += 1;
        }
        if f.is_anchored {
            anchored += 1;
        }
        if f.status == "active" && decision != Some("skip") {
            included += 1;
        }
    }

    (accept, edit, suppress, anchored, included)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::{
        AgentRun, Finding, PullRequest, ReviewRun, ReviewerDecision, Workspace,
    };
    use crate::storage::queries;

    fn setup_test_db() -> Connection {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.into_inner().unwrap();

        queries::insert_workspace(
            &conn,
            &Workspace {
                id: "ws".into(),
                local_path: "/tmp".into(),
                remote_owner: "o".into(),
                remote_repo: "r".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                remote_host: "github.com".into(),
            },
        )
        .unwrap();
        queries::insert_pull_request(
            &conn,
            &PullRequest {
                id: "pr".into(),
                workspace_id: "ws".into(),
                pr_number: 1,
                title: "t".into(),
                author: None,
                base_branch: None,
                head_branch: None,
                url: "u".into(),
                diff_text: None,
                changed_files: None,
                fetched_at: "2026-01-01T00:00:00Z".into(),
                diff_hash: None,
                platform_metadata_json: None,
                platform_metadata_fetched_at: None,
            },
        )
        .unwrap();
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run".into(),
                pr_id: "pr".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:01:00Z".into()),
                error_message: None,
                head_sha_at_run: None,
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();

        conn
    }

    #[test]
    fn test_compute_scorecard_empty_run() {
        let conn = setup_test_db();
        let scorecard = compute_run_scorecard(&conn, "run").unwrap();
        assert_eq!(scorecard.overall_surfaced, 0);
        assert_eq!(scorecard.overall_accept_rate, 0.0);
        assert_eq!(scorecard.overall_edit_rate, 0.0);
        assert_eq!(scorecard.overall_suppress_rate, 0.0);
        assert!(scorecard.lanes.is_empty());
    }

    #[test]
    fn test_compute_scorecard_with_findings_and_decisions() {
        let conn = setup_test_db();

        queries::insert_agent_run(
            &conn,
            &AgentRun {
                id: "ar1".into(),
                review_run_id: "run".into(),
                lane_id: "security".into(),
                provider_name: "codex".into(),
                status: "completed".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:00:30Z".into()),
                finding_count: 2,
                error_message: None,
                governance_tier_at_run: None,
                provider_session_id: None,
                resume_cursor: None,
                checkpoint_metadata_json: None,
                cost_usd: Some(0.05),
            },
        )
        .unwrap();

        queries::insert_finding(
            &conn,
            &Finding {
                id: "f1".into(),
                review_run_id: "run".into(),
                agent_type: "security".into(),
                file_path: Some("src/a.rs".into()),
                line_start: Some(10),
                line_end: Some(20),
                severity: "warning".into(),
                confidence: 0.8,
                title: "Issue 1".into(),
                body: "Body 1".into(),
                evidence: None,
                status: "active".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: true,
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
            },
        )
        .unwrap();

        queries::insert_finding(
            &conn,
            &Finding {
                id: "f2".into(),
                review_run_id: "run".into(),
                agent_type: "security".into(),
                file_path: Some("src/b.rs".into()),
                line_start: Some(5),
                line_end: Some(10),
                severity: "info".into(),
                confidence: 0.5,
                title: "Issue 2".into(),
                body: "Body 2".into(),
                evidence: None,
                status: "suppressed".into(),
                user_edited_body: None,
                user_severity_override: None,
                is_anchored: true,
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
            },
        )
        .unwrap();

        queries::insert_decision(
            &conn,
            &ReviewerDecision {
                id: "d1".into(),
                finding_id: "f1".into(),
                review_run_id: "run".into(),
                decision: "accept".into(),
                original_severity: "warning".into(),
                original_agent_type: "security".into(),
                category_tag: None,
                time_to_decision_ms: Some(1000),
                decided_at: "2026-01-01T00:00:30Z".into(),
            },
        )
        .unwrap();

        let scorecard = compute_run_scorecard(&conn, "run").unwrap();
        assert_eq!(scorecard.overall_surfaced, 2);
        assert_eq!(scorecard.overall_accept_rate, 0.5);
        assert_eq!(scorecard.overall_edit_rate, 0.0);
        assert_eq!(scorecard.overall_suppress_rate, 0.5);
        assert_eq!(scorecard.lanes.len(), 1);
        assert_eq!(scorecard.lanes[0].lane_id, "security");
        assert_eq!(scorecard.lanes[0].raw_findings_count, 2);
        assert_eq!(scorecard.lanes[0].surfaced_findings_count, 2);
        assert_eq!(scorecard.lanes[0].reviewer_accept_rate, 0.5);
        assert_eq!(scorecard.lanes[0].reviewer_edit_rate, 0.0);
        assert_eq!(scorecard.lanes[0].suppress_rate, 0.5);
        assert!(scorecard.lanes[0].cost_usd.unwrap() > 0.0);
        assert!(scorecard.lanes[0].lane_latency_ms.unwrap() > 0);
    }
}
