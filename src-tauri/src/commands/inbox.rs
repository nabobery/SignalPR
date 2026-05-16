use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::commands::environment::EnvironmentSummary;
use crate::storage::db::AppDb;
use crate::storage::models::{
    AgentRun, InboxAttentionSummary, InboxLaneHealth, InboxMetadataFreshness, InboxReviewFreshness,
    InboxReviewRow, InboxReviewerSignal, InboxSection, InboxSubmissionHealth, InboxWorkspaceRow,
    ReviewRun, SubmissionRecord,
};
use crate::storage::queries::{self, InboxPrCandidate};

const MAX_INBOX_ROWS: i32 = 40;
const MAX_RECENT_WORKSPACES: i32 = 8;
const METADATA_STALE_AFTER_HOURS: i64 = 24;
const SUBMITTED_RECENT_WINDOW_HOURS: i64 = 48;

#[derive(Debug, Serialize)]
pub struct InboxOverview {
    pub environment_summary: EnvironmentSummary,
    pub attention_summary: InboxAttentionSummary,
    pub sections: Vec<InboxSection>,
    pub recent_workspaces: Vec<InboxWorkspaceRow>,
}

#[derive(Debug, Default)]
struct ParsedPlatformMetadata {
    platform: String,
    draft: bool,
    head_sha: Option<String>,
    requested_reviewers: Vec<String>,
    requested_teams: Vec<String>,
}

#[derive(Debug)]
struct DerivedQueueRow {
    row: InboxReviewRow,
    attention_failed_run: bool,
    attention_failed_submission: bool,
    attention_stale_metadata: bool,
    attention_degraded_run: bool,
}

#[derive(Debug)]
struct InboxProjectionData {
    latest_runs_by_pr: HashMap<String, ReviewRun>,
    active_finding_counts_by_run: HashMap<String, i32>,
    agent_runs_by_review: HashMap<String, Vec<AgentRun>>,
    submissions_by_pr: HashMap<String, Vec<SubmissionRecord>>,
    review_draft_run_ids: HashSet<String>,
}

#[derive(Debug, Deserialize)]
struct GhAuthStatus {
    hosts: HashMap<String, Vec<GhAuthHost>>,
}

#[derive(Debug, Deserialize)]
struct GhAuthHost {
    #[serde(default)]
    active: bool,
    host: Option<String>,
    login: Option<String>,
}

#[tauri::command]
pub async fn get_inbox_overview(
    app: AppHandle,
    db: tauri::State<'_, AppDb>,
) -> Result<InboxOverview, crate::errors::AppError> {
    use crate::errors::AppError;

    let environment_summary = crate::commands::environment::build_environment_summary(&app).await;
    let github_login = resolve_github_login(&app).await;

    let (rows, recent_workspaces) = {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        let candidates = queries::list_inbox_pr_candidates(&conn, MAX_INBOX_ROWS)?;
        let recent_workspaces = queries::list_recent_workspaces(&conn, MAX_RECENT_WORKSPACES)?;
        let projection = load_projection_data(&conn, &candidates)?;
        let mut rows = Vec::new();
        for candidate in candidates {
            if let Some(row) = build_queue_row(
                &candidate,
                github_login.as_deref(),
                &projection,
                &environment_summary,
            ) {
                rows.push(row);
            }
        }
        (rows, recent_workspaces)
    };

    let attention_summary = build_attention_summary(&rows);
    let sections = build_sections(rows);

    Ok(InboxOverview {
        environment_summary,
        attention_summary,
        sections,
        recent_workspaces,
    })
}

fn load_projection_data(
    conn: &rusqlite::Connection,
    candidates: &[InboxPrCandidate],
) -> Result<InboxProjectionData, rusqlite::Error> {
    let pr_ids: Vec<String> = candidates
        .iter()
        .map(|candidate| candidate.pr_id.clone())
        .collect();
    let latest_runs_by_pr = queries::list_latest_review_runs_for_prs(conn, &pr_ids)?;
    let run_ids: Vec<String> = latest_runs_by_pr
        .values()
        .map(|run| run.id.clone())
        .collect();

    Ok(InboxProjectionData {
        active_finding_counts_by_run: queries::list_active_finding_counts_for_runs(conn, &run_ids)?,
        agent_runs_by_review: queries::list_agent_runs_for_reviews(conn, &run_ids)?,
        submissions_by_pr: queries::list_submission_history_for_prs(conn, &pr_ids)?,
        review_draft_run_ids: queries::list_review_draft_run_ids(conn, &run_ids)?,
        latest_runs_by_pr,
    })
}

fn build_queue_row(
    candidate: &InboxPrCandidate,
    github_login: Option<&str>,
    projection: &InboxProjectionData,
    environment_summary: &EnvironmentSummary,
) -> Option<DerivedQueueRow> {
    let latest_run = projection.latest_runs_by_pr.get(&candidate.pr_id)?;
    let agent_runs = projection
        .agent_runs_by_review
        .get(&latest_run.id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let submissions = projection
        .submissions_by_pr
        .get(&candidate.pr_id)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let has_saved_review_draft = projection.review_draft_run_ids.contains(&latest_run.id);

    let metadata = parse_platform_metadata(candidate.platform_metadata_json.as_deref());
    let platform_capabilities =
        parse_platform_capabilities(candidate.platform_capabilities_json.as_deref());
    let metadata_freshness =
        derive_metadata_freshness(candidate.platform_metadata_fetched_at.as_deref());
    let reviewer_signal = derive_reviewer_signal(&metadata, github_login);
    let lane_health = derive_lane_health(latest_run, agent_runs);
    let submission_health = derive_submission_health(submissions);
    let review_freshness = derive_review_freshness(latest_run, &metadata, &submission_health);
    let allowed_actions = derive_allowed_actions(
        latest_run,
        &metadata,
        platform_capabilities.as_ref(),
        &metadata_freshness,
        &review_freshness,
        environment_summary,
    );
    let queue_state = classify_queue_state(
        latest_run,
        &metadata_freshness,
        &review_freshness,
        &reviewer_signal,
        &lane_health,
        &submission_health,
    );
    let attention_reasons = derive_attention_reasons(
        latest_run,
        &metadata_freshness,
        &lane_health,
        &submission_health,
    );
    let providers_used = {
        let mut providers: Vec<String> =
            agent_runs.iter().map(|r| r.provider_name.clone()).collect();
        providers.sort();
        providers.dedup();
        providers
    };

    let row = InboxReviewRow {
        run_id: latest_run.id.clone(),
        pr_id: candidate.pr_id.clone(),
        pr_number: candidate.pr_number,
        title: candidate.title.clone(),
        author: candidate.author.clone(),
        pr_url: candidate.pr_url.clone(),
        status: latest_run.status.clone(),
        last_updated: latest_run
            .completed_at
            .clone()
            .or_else(|| latest_run.started_at.clone())
            .unwrap_or_else(|| candidate.last_activity_at.clone()),
        active_finding_count: *projection
            .active_finding_counts_by_run
            .get(&latest_run.id)
            .unwrap_or(&0),
        providers_used,
        queue_state,
        platform: metadata.platform.clone(),
        repo_owner: candidate.repo_owner.clone(),
        repo_name: candidate.repo_name.clone(),
        remote_host: candidate.remote_host.clone(),
        workspace_id: candidate.workspace_id.clone(),
        workspace_path: candidate.workspace_path.clone(),
        draft: metadata.draft,
        has_saved_review_draft,
        metadata_freshness,
        platform_capabilities,
        platform_capabilities_fetched_at: candidate.platform_capabilities_fetched_at.clone(),
        review_freshness,
        reviewer_signal,
        lane_health,
        submission_health,
        attention_reasons: attention_reasons.clone(),
        allowed_actions,
    };

    Some(DerivedQueueRow {
        attention_failed_run: latest_run.status == "failed",
        attention_failed_submission: row.submission_health.state == "failed",
        attention_stale_metadata: row.metadata_freshness.is_stale,
        attention_degraded_run: row.lane_health.state == "degraded"
            || row.lane_health.state == "failed",
        row,
    })
}

fn parse_platform_metadata(json: Option<&str>) -> ParsedPlatformMetadata {
    let Some(json) = json else {
        return ParsedPlatformMetadata {
            platform: "unknown".into(),
            ..ParsedPlatformMetadata::default()
        };
    };

    if let Ok(meta) = serde_json::from_str::<crate::platform::adapter::PlatformMetadata>(json) {
        return match meta {
            crate::platform::adapter::PlatformMetadata::GitHub(g) => ParsedPlatformMetadata {
                platform: "github".into(),
                draft: g.draft,
                head_sha: Some(g.head_sha),
                requested_reviewers: g.requested_reviewers,
                requested_teams: g.requested_teams,
            },
            crate::platform::adapter::PlatformMetadata::GitLab(g) => ParsedPlatformMetadata {
                platform: "gitlab".into(),
                draft: g.draft,
                head_sha: Some(g.head_sha),
                requested_reviewers: g.reviewers,
                requested_teams: Vec::new(),
            },
            crate::platform::adapter::PlatformMetadata::Bitbucket(b) => ParsedPlatformMetadata {
                platform: "bitbucket".into(),
                draft: b.draft,
                head_sha: Some(b.head_sha),
                requested_reviewers: if b.reviewers.is_empty() {
                    b.default_reviewers
                } else {
                    b.reviewers
                },
                requested_teams: Vec::new(),
            },
        };
    }

    if let Ok(legacy) =
        serde_json::from_str::<crate::providers::github::PlatformMetadataSnapshot>(json)
    {
        return ParsedPlatformMetadata {
            platform: "github".into(),
            draft: legacy.draft,
            head_sha: Some(legacy.head_sha),
            requested_reviewers: legacy.requested_reviewers,
            requested_teams: legacy.requested_teams,
        };
    }

    ParsedPlatformMetadata {
        platform: "unknown".into(),
        ..ParsedPlatformMetadata::default()
    }
}

fn parse_platform_capabilities(
    json: Option<&str>,
) -> Option<crate::platform::adapter::PlatformCapabilities> {
    json.and_then(|raw| serde_json::from_str(raw).ok())
}

fn derive_metadata_freshness(fetched_at: Option<&str>) -> InboxMetadataFreshness {
    let is_stale = fetched_at
        .and_then(parse_rfc3339)
        .map(|ts| Utc::now() - ts > Duration::hours(METADATA_STALE_AFTER_HOURS))
        .unwrap_or(false);
    InboxMetadataFreshness {
        fetched_at: fetched_at.map(ToString::to_string),
        is_stale,
    }
}

fn derive_reviewer_signal(
    metadata: &ParsedPlatformMetadata,
    github_login: Option<&str>,
) -> InboxReviewerSignal {
    let requested_reviewers = metadata.requested_reviewers.clone();
    let requested_teams = metadata.requested_teams.clone();
    let exact_match = if metadata.platform == "github" {
        github_login.is_some_and(|login| {
            requested_reviewers
                .iter()
                .any(|r| r.eq_ignore_ascii_case(login))
        })
    } else {
        false
    };
    let has_signal = !requested_reviewers.is_empty() || !requested_teams.is_empty();
    let (label, precision) = if exact_match {
        ("Needs your review".to_string(), "exact".to_string())
    } else if has_signal {
        ("Review requested".to_string(), "repo".to_string())
    } else {
        ("No reviewer signal".to_string(), "none".to_string())
    };

    InboxReviewerSignal {
        has_signal,
        label,
        precision,
        requested_reviewers,
        requested_teams,
    }
}

fn derive_lane_health(
    latest_run: &ReviewRun,
    agent_runs: &[crate::storage::models::AgentRun],
) -> InboxLaneHealth {
    let failed_count = agent_runs
        .iter()
        .filter(|run| run.status == "failed" || run.status == "cancelled")
        .count() as i32;
    let timed_out_count = agent_runs
        .iter()
        .filter(|run| run.status == "timed_out")
        .count() as i32;
    let running_count = agent_runs
        .iter()
        .filter(|run| run.status == "running")
        .count() as i32;
    let completed_count = agent_runs
        .iter()
        .filter(|run| run.status == "completed")
        .count() as i32;
    let state = if latest_run.status == "failed" || failed_count > 0 {
        "failed"
    } else if timed_out_count > 0 {
        "degraded"
    } else if running_count > 0
        || matches!(
            latest_run.status.as_str(),
            "created" | "running_agents" | "cleaning"
        )
    {
        "running"
    } else if completed_count > 0 {
        "healthy"
    } else {
        "unknown"
    };

    InboxLaneHealth {
        state: state.to_string(),
        failed_count,
        timed_out_count,
        running_count,
        completed_count,
    }
}

fn derive_submission_health(submissions: &[SubmissionRecord]) -> InboxSubmissionHealth {
    let latest = submissions.first();
    InboxSubmissionHealth {
        state: latest
            .map(|submission| submission.status.clone())
            .unwrap_or_else(|| "none".into()),
        submitted_at: latest.and_then(|submission| submission.submitted_at.clone()),
        review_action: latest.map(|submission| submission.review_action.clone()),
        commit_id: latest.and_then(|submission| submission.commit_id_at_submission.clone()),
        error_message: latest.and_then(|submission| submission.error_message.clone()),
    }
}

fn derive_review_freshness(
    latest_run: &ReviewRun,
    metadata: &ParsedPlatformMetadata,
    submission_health: &InboxSubmissionHealth,
) -> InboxReviewFreshness {
    let current_head_sha = metadata.head_sha.clone();
    let reviewed_head_sha = if latest_run.status == "submitted" {
        submission_health
            .commit_id
            .clone()
            .or_else(|| latest_run.head_sha_at_run.clone())
    } else {
        latest_run
            .head_sha_at_run
            .clone()
            .or_else(|| submission_health.commit_id.clone())
    };
    let has_unreviewed_updates = reviewed_head_sha
        .as_deref()
        .zip(current_head_sha.as_deref())
        .is_some_and(|(reviewed, current)| reviewed != current);

    InboxReviewFreshness {
        state: if has_unreviewed_updates {
            "stale".into()
        } else {
            "current".into()
        },
        reviewed_at: latest_run
            .completed_at
            .clone()
            .or_else(|| latest_run.started_at.clone()),
        reviewed_head_sha,
        current_head_sha,
        has_unreviewed_updates,
    }
}

fn classify_queue_state(
    latest_run: &ReviewRun,
    metadata_freshness: &InboxMetadataFreshness,
    review_freshness: &InboxReviewFreshness,
    reviewer_signal: &InboxReviewerSignal,
    lane_health: &InboxLaneHealth,
    submission_health: &InboxSubmissionHealth,
) -> String {
    let has_attention = latest_run.status == "failed"
        || submission_health.state == "failed"
        || metadata_freshness.is_stale
        || lane_health.state == "failed"
        || lane_health.state == "degraded";
    if has_attention {
        return "attention_needed".into();
    }

    if matches!(
        latest_run.status.as_str(),
        "created" | "running_agents" | "cleaning"
    ) {
        return "in_progress".into();
    }

    let current_head = review_freshness.current_head_sha.as_deref();
    let reviewed_head = review_freshness.reviewed_head_sha.as_deref();
    let current_head_matches_submission = current_head
        .zip(submission_health.commit_id.as_deref())
        .is_some_and(|(current, submitted)| current == submitted);
    let current_head_matches_run = current_head
        .zip(reviewed_head)
        .is_some_and(|(current, reviewed)| current == reviewed);
    let has_recent_submission = submission_health.state == "submitted"
        && submission_health
            .submitted_at
            .as_deref()
            .and_then(parse_rfc3339)
            .map(|submitted_at| {
                Utc::now() - submitted_at <= Duration::hours(SUBMITTED_RECENT_WINDOW_HOURS)
            })
            .unwrap_or(false);

    if submission_health.state == "submitted"
        && (current_head_matches_submission || current_head.is_none())
    {
        return if has_recent_submission {
            "submitted_recently".into()
        } else {
            "waiting_on_author".into()
        };
    }

    if matches!(latest_run.status.as_str(), "ready" | "submitted")
        && review_freshness.has_unreviewed_updates
    {
        return "updated_since_review".into();
    }

    if reviewer_signal.precision == "exact" {
        return "needs_your_review".into();
    }

    if reviewer_signal.has_signal || review_freshness.has_unreviewed_updates {
        return "review_requested".into();
    }

    if latest_run.status == "ready" && (current_head_matches_run || current_head.is_none()) {
        return "ready_to_submit".into();
    }

    if latest_run.status == "ready" {
        return "ready_to_submit".into();
    }

    "submitted_recently".into()
}

fn derive_attention_reasons(
    latest_run: &ReviewRun,
    metadata_freshness: &InboxMetadataFreshness,
    lane_health: &InboxLaneHealth,
    submission_health: &InboxSubmissionHealth,
) -> Vec<String> {
    let mut reasons = Vec::new();
    if latest_run.status == "failed" {
        reasons.push("Review run failed".into());
    }
    if lane_health.failed_count > 0 {
        reasons.push(format!("{} lane failed", lane_health.failed_count));
    }
    if lane_health.timed_out_count > 0 {
        reasons.push(format!("{} lane timed out", lane_health.timed_out_count));
    }
    if submission_health.state == "failed" {
        reasons.push("Submission failed".into());
    }
    if metadata_freshness.is_stale {
        reasons.push("Platform metadata is stale".into());
    }
    reasons
}

fn derive_allowed_actions(
    latest_run: &ReviewRun,
    metadata: &ParsedPlatformMetadata,
    platform_capabilities: Option<&crate::platform::adapter::PlatformCapabilities>,
    metadata_freshness: &InboxMetadataFreshness,
    review_freshness: &InboxReviewFreshness,
    environment_summary: &EnvironmentSummary,
) -> Vec<String> {
    let mut actions = vec!["open".to_string()];
    if matches!(
        latest_run.status.as_str(),
        "created" | "running_agents" | "cleaning"
    ) {
        actions.push("resume".into());
    }
    if metadata.platform != "unknown"
        && platform_auth_ready(&metadata.platform, environment_summary)
        && (platform_capabilities.is_none()
            || capability_available(
                platform_capabilities,
                crate::platform::adapter::PlatformCapabilityKey::PrMetadata,
            ))
    {
        actions.push("refresh_metadata".into());
    }
    if !metadata_freshness.is_stale
        && matches!(latest_run.status.as_str(), "ready" | "submitted")
        && review_freshness.has_unreviewed_updates
        && platform_auth_ready(&metadata.platform, environment_summary)
        && capability_available(
            platform_capabilities,
            crate::platform::adapter::PlatformCapabilityKey::DiffFetch,
        )
    {
        actions.push("rerun".into());
    }
    actions
}

fn capability_available(
    capabilities: Option<&crate::platform::adapter::PlatformCapabilities>,
    key: crate::platform::adapter::PlatformCapabilityKey,
) -> bool {
    match capabilities.and_then(|caps| caps.get(key)) {
        Some(capability) => capability.support != crate::platform::adapter::CapabilitySupport::None,
        None => false,
    }
}

fn platform_auth_ready(platform: &str, environment_summary: &EnvironmentSummary) -> bool {
    match platform {
        "github" => {
            std::env::var("GITHUB_TOKEN")
                .ok()
                .is_some_and(|v| !v.is_empty())
                || std::env::var("GH_TOKEN")
                    .ok()
                    .is_some_and(|v| !v.is_empty())
                || tool_ready(environment_summary, "github_token")
                || tool_ready(environment_summary, "gh")
        }
        "gitlab" => tool_ready(environment_summary, "gitlab_token"),
        "bitbucket" => tool_ready(environment_summary, "bitbucket_token"),
        _ => false,
    }
}

fn tool_ready(environment_summary: &EnvironmentSummary, tool_name: &str) -> bool {
    environment_summary
        .tools
        .iter()
        .find(|tool| tool.tool_name == tool_name)
        .is_some_and(|tool| tool.status == "ready")
}

fn build_attention_summary(rows: &[DerivedQueueRow]) -> InboxAttentionSummary {
    InboxAttentionSummary {
        total_items: rows
            .iter()
            .filter(|row| row.row.queue_state == "attention_needed")
            .count() as i32,
        failed_runs: rows.iter().filter(|row| row.attention_failed_run).count() as i32,
        failed_submissions: rows
            .iter()
            .filter(|row| row.attention_failed_submission)
            .count() as i32,
        stale_metadata: rows
            .iter()
            .filter(|row| row.attention_stale_metadata)
            .count() as i32,
        degraded_runs: rows.iter().filter(|row| row.attention_degraded_run).count() as i32,
    }
}

fn build_sections(rows: Vec<DerivedQueueRow>) -> Vec<InboxSection> {
    let mut needs_your_review = Vec::new();
    let mut updated_since_review = Vec::new();
    let mut review_requested = Vec::new();
    let mut attention_needed = Vec::new();
    let mut in_progress = Vec::new();
    let mut ready_to_submit = Vec::new();
    let mut waiting_on_author = Vec::new();
    let mut submitted_recently = Vec::new();

    for row in rows {
        match row.row.queue_state.as_str() {
            "needs_your_review" => needs_your_review.push(row.row),
            "updated_since_review" => updated_since_review.push(row.row),
            "review_requested" => review_requested.push(row.row),
            "attention_needed" => attention_needed.push(row.row),
            "in_progress" => in_progress.push(row.row),
            "ready_to_submit" => ready_to_submit.push(row.row),
            "waiting_on_author" => waiting_on_author.push(row.row),
            "submitted_recently" => submitted_recently.push(row.row),
            _ => {}
        }
    }

    let mut sections = Vec::new();
    push_section(
        &mut sections,
        "needs_your_review",
        "Needs your review",
        needs_your_review,
    );
    push_section(
        &mut sections,
        "updated_since_review",
        "Updated since review",
        updated_since_review,
    );
    push_section(
        &mut sections,
        "review_requested",
        "Review requested",
        review_requested,
    );
    push_section(
        &mut sections,
        "attention_needed",
        "Attention needed",
        attention_needed,
    );
    push_section(&mut sections, "in_progress", "In progress", in_progress);
    push_section(
        &mut sections,
        "ready_to_submit",
        "Ready to submit",
        ready_to_submit,
    );
    push_section(
        &mut sections,
        "waiting_on_author",
        "Waiting on author",
        waiting_on_author,
    );
    push_section(
        &mut sections,
        "submitted_recently",
        "Submitted recently",
        submitted_recently,
    );
    sections
}

fn push_section(
    sections: &mut Vec<InboxSection>,
    id: &str,
    title: &str,
    items: Vec<InboxReviewRow>,
) {
    if items.is_empty() {
        return;
    }

    sections.push(InboxSection {
        id: id.to_string(),
        title: title.to_string(),
        items,
    });
}

fn parse_rfc3339(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|timestamp| timestamp.with_timezone(&Utc))
}

async fn resolve_github_login(app: &AppHandle) -> Option<String> {
    let shell = app.shell();

    if let Ok(output) = shell
        .command("gh")
        .args(["auth", "status", "--json", "hosts"])
        .output()
        .await
    {
        if let Some(login) = resolve_github_login_from_status_output(&output.stdout, &output.stderr)
        {
            return Some(login);
        }
    }

    let output = shell
        .command("gh")
        .args(["auth", "status", "--active", "--hostname", "github.com"])
        .output()
        .await
        .ok()?;

    resolve_github_login_from_status_output(&output.stdout, &output.stderr)
}

fn resolve_github_login_from_status_output(stdout: &[u8], stderr: &[u8]) -> Option<String> {
    let stdout_text = String::from_utf8_lossy(stdout);
    if let Some(login) = parse_github_login_from_status_json(&stdout_text) {
        return Some(login);
    }

    let text = format!("{}\n{}", stdout_text, String::from_utf8_lossy(stderr));
    text.lines().find_map(parse_github_login_from_status_line)
}

fn parse_github_login_from_status_json(text: &str) -> Option<String> {
    let status: GhAuthStatus = serde_json::from_str(text).ok()?;
    let github_hosts: Vec<GhAuthHost> = status
        .hosts
        .into_iter()
        .filter(|(host, _)| host.eq_ignore_ascii_case("github.com"))
        .flat_map(|(_, entries)| entries)
        .collect();
    if github_hosts.is_empty() {
        return None;
    }

    github_hosts
        .iter()
        .find(|host| host.active)
        .or_else(|| {
            github_hosts
                .iter()
                .find(|host| host.host.as_deref() == Some("github.com"))
        })
        .or_else(|| github_hosts.first())
        .and_then(|host| host.login.as_deref())
        .map(str::trim)
        .filter(|login| !login.is_empty())
        .map(ToString::to_string)
}

fn parse_github_login_from_status_line(line: &str) -> Option<String> {
    if !line.contains("github.com") || !line.contains("account ") {
        return None;
    }
    let after = line.split("account ").nth(1)?.trim();
    let login = after
        .split([' ', '('])
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(login.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::environment::EnvironmentSummary;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::{
        InboxLaneHealth, InboxMetadataFreshness, InboxReviewFreshness, InboxReviewRow,
        InboxReviewerSignal, InboxSubmissionHealth, PullRequest, ReviewRun, SubmissionRecord,
        Workspace,
    };

    fn test_environment_summary() -> EnvironmentSummary {
        EnvironmentSummary {
            can_review: true,
            can_submit: true,
            available_providers: vec!["codex".into()],
            warnings: vec![],
            tools: vec![
                crate::storage::models::ToolStatus {
                    tool_name: "gh".into(),
                    status: "ready".into(),
                    version: None,
                    message: None,
                    checked_at: "2026-01-01T00:00:00Z".into(),
                },
                crate::storage::models::ToolStatus {
                    tool_name: "gitlab_token".into(),
                    status: "ready".into(),
                    version: None,
                    message: None,
                    checked_at: "2026-01-01T00:00:00Z".into(),
                },
                crate::storage::models::ToolStatus {
                    tool_name: "bitbucket_token".into(),
                    status: "ready".into(),
                    version: None,
                    message: None,
                    checked_at: "2026-01-01T00:00:00Z".into(),
                },
            ],
        }
    }

    fn insert_workspace_and_pr(conn: &rusqlite::Connection, metadata_json: Option<&str>) {
        queries::insert_workspace(
            conn,
            &Workspace {
                id: "ws-1".into(),
                local_path: "/tmp/repo".into(),
                remote_owner: "octo".into(),
                remote_repo: "signal".into(),
                created_at: "2026-01-01T00:00:00Z".into(),
                remote_host: "github.com".into(),
            },
        )
        .unwrap();
        let default_capabilities = r#"{"platform":"github","capabilities":[{"key":"pr_metadata","support":"full","constraints":[],"fallback":null},{"key":"diff_fetch","support":"full","constraints":[],"fallback":null}]}"#;
        queries::insert_pull_request(
            conn,
            &PullRequest {
                id: "pr-1".into(),
                workspace_id: "ws-1".into(),
                pr_number: 42,
                title: "Tighten auth".into(),
                author: Some("alice".into()),
                base_branch: Some("main".into()),
                head_branch: Some("feature/auth".into()),
                url: "https://github.com/octo/signal/pull/42".into(),
                diff_text: Some("diff --git a/src/lib.rs b/src/lib.rs".into()),
                changed_files: Some("[\"src/lib.rs\"]".into()),
                fetched_at: "2026-01-01T00:00:00Z".into(),
                diff_hash: Some("abc".into()),
                platform_metadata_json: metadata_json.map(ToString::to_string),
                platform_metadata_fetched_at: Some(Utc::now().to_rfc3339()),
                platform_capabilities_json: Some(default_capabilities.into()),
                platform_capabilities_fetched_at: Some(Utc::now().to_rfc3339()),
            },
        )
        .unwrap();
    }

    fn load_candidate_and_projection(
        conn: &rusqlite::Connection,
    ) -> (InboxPrCandidate, InboxProjectionData) {
        let mut candidates = queries::list_inbox_pr_candidates(conn, 10).unwrap();
        let projection = load_projection_data(conn, &candidates).unwrap();
        (candidates.remove(0), projection)
    }

    fn make_section_row(queue_state: &str, precision: &str) -> DerivedQueueRow {
        DerivedQueueRow {
            attention_failed_run: false,
            attention_failed_submission: false,
            attention_stale_metadata: false,
            attention_degraded_run: false,
            row: InboxReviewRow {
                run_id: format!("run-{queue_state}"),
                pr_id: format!("pr-{queue_state}"),
                pr_number: 1,
                title: queue_state.into(),
                author: Some("alice".into()),
                pr_url: "https://example.com".into(),
                status: "ready".into(),
                last_updated: "2026-01-01T00:00:00Z".into(),
                active_finding_count: 0,
                providers_used: vec!["codex".into()],
                queue_state: queue_state.into(),
                platform: "github".into(),
                repo_owner: "octo".into(),
                repo_name: "signal".into(),
                remote_host: "github.com".into(),
                workspace_id: "ws-1".into(),
                workspace_path: "/tmp/repo".into(),
                draft: false,
                has_saved_review_draft: false,
                metadata_freshness: InboxMetadataFreshness {
                    fetched_at: None,
                    is_stale: false,
                },
                platform_capabilities: None,
                platform_capabilities_fetched_at: None,
                review_freshness: InboxReviewFreshness {
                    state: "current".into(),
                    reviewed_at: Some("2026-01-01T00:00:00Z".into()),
                    reviewed_head_sha: Some("sha-reviewed".into()),
                    current_head_sha: Some("sha-reviewed".into()),
                    has_unreviewed_updates: false,
                },
                reviewer_signal: InboxReviewerSignal {
                    has_signal: precision != "none",
                    label: if precision == "exact" {
                        "Needs your review".into()
                    } else {
                        "Review requested".into()
                    },
                    precision: precision.into(),
                    requested_reviewers: Vec::new(),
                    requested_teams: Vec::new(),
                },
                lane_health: InboxLaneHealth {
                    state: "healthy".into(),
                    failed_count: 0,
                    timed_out_count: 0,
                    running_count: 0,
                    completed_count: 3,
                },
                submission_health: InboxSubmissionHealth {
                    state: "none".into(),
                    submitted_at: None,
                    review_action: None,
                    commit_id: None,
                    error_message: None,
                },
                attention_reasons: Vec::new(),
                allowed_actions: vec!["open".into()],
            },
        }
    }

    #[test]
    fn github_requested_reviewer_becomes_needs_your_review() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        insert_workspace_and_pr(
            &conn,
            Some(
                r#"{"platform":"github","pr_body":null,"head_sha":"sha-1","base_sha":"sha-base","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":["mona"],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            ),
        );
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:05:00Z".into()),
                error_message: None,
                head_sha_at_run: Some("sha-1".into()),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();

        let (candidate, projection) = load_candidate_and_projection(&conn);
        let row = build_queue_row(
            &candidate,
            Some("mona"),
            &projection,
            &test_environment_summary(),
        )
        .unwrap();
        assert_eq!(row.row.queue_state, "needs_your_review");
        assert_eq!(row.row.reviewer_signal.precision, "exact");
    }

    #[test]
    fn submitted_current_head_becomes_waiting_on_author_after_window() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        insert_workspace_and_pr(
            &conn,
            Some(
                r#"{"platform":"github","pr_body":null,"head_sha":"sha-2","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            ),
        );
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "submitted".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:05:00Z".into()),
                error_message: None,
                head_sha_at_run: Some("sha-2".into()),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();
        queries::insert_submission(
            &conn,
            &SubmissionRecord {
                id: "sub-1".into(),
                review_run_id: "run-1".into(),
                review_action: "comment".into(),
                submitted_at: Some((Utc::now() - Duration::hours(72)).to_rfc3339()),
                status: "submitted".into(),
                commit_id_at_submission: Some("sha-2".into()),
                platform_review_id: None,
                error_message: None,
                idempotency_key: None,
                attempt_count: Some(1),
                last_attempt_at: Some((Utc::now() - Duration::hours(72)).to_rfc3339()),
            },
        )
        .unwrap();

        let (candidate, projection) = load_candidate_and_projection(&conn);
        let row =
            build_queue_row(&candidate, None, &projection, &test_environment_summary()).unwrap();
        assert_eq!(row.row.queue_state, "waiting_on_author");
    }

    #[test]
    fn stale_metadata_surfaces_attention_needed() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        insert_workspace_and_pr(
            &conn,
            Some(
                r#"{"platform":"github","pr_body":null,"head_sha":"sha-2","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            ),
        );
        queries::update_pull_request_metadata(
            &conn,
            "pr-1",
            r#"{"platform":"github","pr_body":null,"head_sha":"sha-2","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            &(Utc::now() - Duration::hours(30)).to_rfc3339(),
        )
        .unwrap();
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:05:00Z".into()),
                error_message: None,
                head_sha_at_run: Some("sha-2".into()),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();

        let (candidate, projection) = load_candidate_and_projection(&conn);
        let row =
            build_queue_row(&candidate, None, &projection, &test_environment_summary()).unwrap();
        assert_eq!(row.row.queue_state, "attention_needed");
        assert!(row.row.metadata_freshness.is_stale);
    }

    #[test]
    fn newer_head_sha_becomes_updated_since_review() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        insert_workspace_and_pr(
            &conn,
            Some(
                r#"{"platform":"github","pr_body":null,"head_sha":"sha-3","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            ),
        );
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:05:00Z".into()),
                error_message: None,
                head_sha_at_run: Some("sha-2".into()),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();

        let (candidate, projection) = load_candidate_and_projection(&conn);
        let row =
            build_queue_row(&candidate, None, &projection, &test_environment_summary()).unwrap();
        assert_eq!(row.row.queue_state, "updated_since_review");
        assert_eq!(row.row.review_freshness.state, "stale");
        assert!(row.row.review_freshness.has_unreviewed_updates);
        assert_eq!(
            row.row.review_freshness.reviewed_head_sha.as_deref(),
            Some("sha-2")
        );
        assert_eq!(
            row.row.review_freshness.current_head_sha.as_deref(),
            Some("sha-3")
        );
        assert!(row
            .row
            .allowed_actions
            .iter()
            .any(|action| action == "rerun"));
    }

    #[test]
    fn stale_metadata_does_not_become_updated_since_review() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        insert_workspace_and_pr(
            &conn,
            Some(
                r#"{"platform":"github","pr_body":null,"head_sha":"sha-3","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            ),
        );
        queries::update_pull_request_metadata(
            &conn,
            "pr-1",
            r#"{"platform":"github","pr_body":null,"head_sha":"sha-3","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            &(Utc::now() - Duration::hours(30)).to_rfc3339(),
        )
        .unwrap();
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:05:00Z".into()),
                error_message: None,
                head_sha_at_run: Some("sha-2".into()),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();

        let (candidate, projection) = load_candidate_and_projection(&conn);
        let row =
            build_queue_row(&candidate, None, &projection, &test_environment_summary()).unwrap();
        assert_eq!(row.row.queue_state, "attention_needed");
        assert_eq!(row.row.review_freshness.state, "stale");
        assert!(row.row.review_freshness.has_unreviewed_updates);
        assert!(!row
            .row
            .allowed_actions
            .iter()
            .any(|action| action == "rerun"));
    }

    #[test]
    fn github_login_resolves_from_json_even_when_status_is_non_zero() {
        assert_eq!(
            resolve_github_login_from_status_output(
                br#"{"hosts":{"github.com":[{"state":"error","error":"network","active":true,"host":"github.com","login":"mona","tokenSource":"default","gitProtocol":"ssh"}]}}"#,
                b"",
            )
            .as_deref(),
            Some("mona")
        );
    }

    #[test]
    fn mixed_precision_review_rows_split_into_two_sections() {
        let sections = build_sections(vec![
            make_section_row("needs_your_review", "exact"),
            make_section_row("updated_since_review", "none"),
            make_section_row("review_requested", "repo"),
        ]);

        assert_eq!(sections.len(), 3);
        assert_eq!(sections[0].id, "needs_your_review");
        assert_eq!(sections[0].title, "Needs your review");
        assert_eq!(sections[1].id, "updated_since_review");
        assert_eq!(sections[1].title, "Updated since review");
        assert_eq!(sections[2].id, "review_requested");
        assert_eq!(sections[2].title, "Review requested");
    }

    #[test]
    fn saved_review_draft_does_not_mark_pr_as_platform_draft() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        insert_workspace_and_pr(
            &conn,
            Some(
                r#"{"platform":"github","pr_body":null,"head_sha":"sha-2","base_sha":"sha-1","base_ref":"main","head_ref":"feature/auth","draft":false,"labels":[],"requested_reviewers":[],"requested_teams":[],"review_state_summary":[],"linked_issue_numbers":[],"text_issue_refs":[]}"#,
            ),
        );
        queries::insert_review_run(
            &conn,
            &ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "ready".into(),
                started_at: Some("2026-01-01T00:00:00Z".into()),
                completed_at: Some("2026-01-01T00:05:00Z".into()),
                error_message: None,
                head_sha_at_run: Some("sha-2".into()),
                baseline_run_id: None,
                metrics_json: None,
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        )
        .unwrap();
        queries::save_review_draft(&conn, "run-1", "Summary", "comment").unwrap();

        let (candidate, projection) = load_candidate_and_projection(&conn);
        let row =
            build_queue_row(&candidate, None, &projection, &test_environment_summary()).unwrap();

        assert!(!row.row.draft);
        assert!(row.row.has_saved_review_draft);
    }

    #[test]
    fn parses_github_login_from_status_line() {
        let line = "  ✓ Logged in to github.com account mona (keyring)";
        assert_eq!(
            parse_github_login_from_status_line(line).as_deref(),
            Some("mona")
        );
    }
}
