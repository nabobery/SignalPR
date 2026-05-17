use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::config;
use crate::errors::AppError;
use crate::metrics::RunScorecard;
use crate::providers::capabilities::{
    canonical_provider_id, provider_registry, ProviderCapabilities, ProviderSelectionEligibility,
};
use crate::providers::claude::ClaudeProvider;
use crate::providers::claude_code::manager::ClaudeCodeManager;
use crate::providers::claude_code::provider::ClaudeCodeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::codex_app_server::manager::CodexAppServerManager;
use crate::providers::codex_app_server::provider::CodexAppServerProvider;
use crate::providers::copilot::manager::CopilotManager;
use crate::providers::copilot::provider::CopilotProvider;
use crate::providers::cursor::manager::CursorManager;
use crate::providers::cursor::provider::CursorProvider;
use crate::providers::gemini::manager::GeminiManager;
use crate::providers::gemini::provider::GeminiProvider;
use crate::providers::opencode::manager::OpenCodeManager;
use crate::providers::opencode::provider::OpenCodeProvider;
use crate::providers::pi::manager::PiManager;
use crate::providers::pi::provider::PiProvider;
use crate::providers::setup::{
    currently_runnable, determine_setup_state, execution_supported, release_gate_passed,
    release_gate_status, selection_eligible_for_auto, selection_eligible_for_manual,
    ProviderReleaseGateStatus, ProviderSetupState,
};
use crate::providers::traits::{ProviderHealth, ReviewProvider};
use crate::secrets::credentials::{self, CredentialSource};
use crate::storage::models::ReviewRun;
use crate::storage::queries;

const RECENT_RUN_WINDOW: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSelectionMode {
    Preferred,
    Auto,
    Fallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSelectionCheck {
    pub provider_id: String,
    pub available: bool,
    pub reason: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSelectionTrace {
    pub requested_provider: String,
    pub selected_provider: String,
    pub selection_mode: ProviderSelectionMode,
    pub checks: Vec<ProviderSelectionCheck>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRecentMetrics {
    pub sample_count: i32,
    pub avg_latency_ms: Option<f64>,
    pub avg_accept_rate: Option<f64>,
    pub avg_edit_rate: Option<f64>,
    pub avg_suppress_rate: Option<f64>,
    pub avg_anchor_validity: Option<f64>,
    pub avg_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderControlPlaneProvider {
    pub provider_id: String,
    pub display_name: String,
    pub status: String,
    pub status_reason: String,
    pub setup_state: ProviderSetupState,
    pub execution_supported: bool,
    pub release_gate_status: ProviderReleaseGateStatus,
    pub release_gate_passed: bool,
    pub currently_runnable: bool,
    pub credential_source: Option<CredentialSource>,
    pub capabilities: ProviderCapabilities,
    pub recent_metrics: ProviderRecentMetrics,
    pub fit_narrative: String,
    pub recommended_default: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderControlPlaneSnapshot {
    pub providers: Vec<ProviderControlPlaneProvider>,
    pub recommended_provider_id: Option<String>,
    pub recommendation_reason: Option<String>,
    pub preferred_provider: String,
    pub workspace_id: Option<String>,
    pub recent_window_size: usize,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunMetadataResponse {
    pub runs: Vec<crate::storage::models::AgentRun>,
    pub provider_selection: Option<ProviderSelectionTrace>,
}

#[derive(Debug, Default, Clone)]
struct MetricAccumulator {
    sample_count: i32,
    latency_total_ms: f64,
    latency_count: i32,
    accept_total: f64,
    edit_total: f64,
    suppress_total: f64,
    anchor_total: f64,
    cost_total: f64,
    cost_count: i32,
}

impl MetricAccumulator {
    fn add_lane(&mut self, lane: &crate::metrics::LaneScorecard) {
        self.sample_count += 1;
        self.accept_total += lane.reviewer_accept_rate;
        self.edit_total += lane.reviewer_edit_rate;
        self.suppress_total += lane.suppress_rate;
        self.anchor_total += lane.anchor_validity;
        if let Some(latency) = lane.lane_latency_ms {
            self.latency_total_ms += latency as f64;
            self.latency_count += 1;
        }
        if let Some(cost) = lane.cost_usd {
            self.cost_total += cost;
            self.cost_count += 1;
        }
    }

    fn into_recent_metrics(self) -> ProviderRecentMetrics {
        ProviderRecentMetrics {
            sample_count: self.sample_count,
            avg_latency_ms: average(self.latency_total_ms, self.latency_count),
            avg_accept_rate: average(self.accept_total, self.sample_count),
            avg_edit_rate: average(self.edit_total, self.sample_count),
            avg_suppress_rate: average(self.suppress_total, self.sample_count),
            avg_anchor_validity: average(self.anchor_total, self.sample_count),
            avg_cost_usd: average(self.cost_total, self.cost_count),
        }
    }
}

fn average(total: f64, count: i32) -> Option<f64> {
    if count > 0 {
        Some(total / count as f64)
    } else {
        None
    }
}

fn credential_source_for_provider(
    provider_id: &str,
    statuses: &[credentials::CredentialStatus],
) -> Option<CredentialSource> {
    let canonical = canonical_provider_id(provider_id);
    let mut best: Option<CredentialSource> = None;
    for status in statuses {
        if status
            .provider_ids
            .iter()
            .any(|id| canonical_provider_id(id) == canonical)
        {
            best = Some(match (best, status.source) {
                (Some(CredentialSource::Environment), _) => CredentialSource::Environment,
                (_, CredentialSource::Environment) => CredentialSource::Environment,
                (Some(CredentialSource::Keychain), _) => CredentialSource::Keychain,
                (_, CredentialSource::Keychain) => CredentialSource::Keychain,
                _ => CredentialSource::None,
            });
        }
    }
    best
}

fn aggregate_recent_metrics(recent_runs: &[ReviewRun]) -> HashMap<String, ProviderRecentMetrics> {
    let mut accumulators: HashMap<String, MetricAccumulator> = HashMap::new();

    for run in recent_runs {
        let Some(metrics_json) = run.metrics_json.as_deref() else {
            continue;
        };
        let Ok(scorecard) = serde_json::from_str::<RunScorecard>(metrics_json) else {
            continue;
        };
        for lane in scorecard.lanes {
            let provider_id = canonical_provider_id(&lane.provider_name).to_string();
            accumulators.entry(provider_id).or_default().add_lane(&lane);
        }
    }

    accumulators
        .into_iter()
        .map(|(provider_id, acc)| (provider_id, acc.into_recent_metrics()))
        .collect()
}

fn fit_narrative(
    caps: &ProviderCapabilities,
    status: &str,
    metrics: &ProviderRecentMetrics,
    warnings: &[String],
) -> String {
    let mut parts = Vec::new();
    parts.push(match status {
        "ready" => "Ready now.",
        "degraded" => "Usable with caveats.",
        _ => "Currently unavailable.",
    });

    if !caps.fit_tags.is_empty() {
        parts.push(match caps.fit_tags.first().map(String::as_str) {
            Some("fast_scan") => "Best fit: fast scan.",
            Some("deep_reasoning") => "Best fit: deeper reasoning.",
            Some("interactive") => "Best fit: interactive workflows.",
            Some("safest_read_only") => "Best fit: safest read-only review.",
            _ => "Fit is context-dependent.",
        });
    }

    if let Some(accept) = metrics.avg_accept_rate {
        if accept >= 0.6 {
            parts.push("Recent reviewer acceptance is strong.");
        } else if accept > 0.0 {
            parts.push("Recent reviewer acceptance is mixed.");
        }
    }

    if let Some(cost) = metrics.avg_cost_usd {
        if cost > 1.0 {
            parts.push("Cost is material when this provider runs.");
        }
    } else if caps.billing_risk == "paid_api" || caps.billing_risk == "subscription" {
        parts.push("This provider can consume paid capacity.");
    }

    if !warnings.is_empty() {
        parts.push("Review setup details before making it the default.");
    }

    parts.join(" ")
}

fn build_status(
    caps: &ProviderCapabilities,
    health: &ProviderHealth,
    credential_source: Option<CredentialSource>,
) -> (
    String,
    String,
    ProviderSetupState,
    bool,
    ProviderReleaseGateStatus,
    bool,
    bool,
    Vec<String>,
) {
    let mut warnings = Vec::new();
    let setup_state = determine_setup_state(caps, health, credential_source);
    let execution_supported = execution_supported(caps);
    let release_gate_status = release_gate_status(caps, &setup_state);
    let release_gate_passed = release_gate_passed(caps, &setup_state);
    let currently_runnable = currently_runnable(caps, &setup_state);
    let status_reason = if !health.available {
        health
            .message
            .clone()
            .unwrap_or_else(|| "Health check failed".to_string())
    } else if matches!(setup_state, ProviderSetupState::DiscoverableOnly) {
        warnings.push("Catalog-only provider; review execution is not enabled yet.".to_string());
        "Listed in the catalog, but review execution is not enabled yet.".to_string()
    } else if !release_gate_passed {
        match release_gate_status {
            ProviderReleaseGateStatus::BlockedConformance => {
                warnings.push("Conformance coverage is still incomplete.".to_string());
                "Healthy, but blocked until conformance coverage is complete.".to_string()
            }
            ProviderReleaseGateStatus::BlockedEval => {
                warnings.push("Eval coverage is still incomplete.".to_string());
                "Healthy, but blocked until eval coverage is complete.".to_string()
            }
            ProviderReleaseGateStatus::BlockedSetup => {
                "Setup is incomplete for review runs.".to_string()
            }
            ProviderReleaseGateStatus::Passed => "Ready for review runs".to_string(),
        }
    } else if caps.opt_in_only {
        warnings.push("Opt-in provider; excluded from auto mode.".to_string());
        "Available but opt-in only".to_string()
    } else {
        "Ready for review runs".to_string()
    };

    if health.available {
        if matches!(credential_source, Some(CredentialSource::None))
            && !caps.credential_fields.is_empty()
        {
            warnings.push("Credential-backed provider may be misconfigured.".to_string());
        }
        if caps.billing_risk == "paid_api" || caps.billing_risk == "subscription" {
            warnings.push("May incur paid usage.".to_string());
        }
        let status = if caps.opt_in_only
            || health.message.is_some()
            || !release_gate_passed
            || matches!(setup_state, ProviderSetupState::DiscoverableOnly)
        {
            "degraded".to_string()
        } else {
            "ready".to_string()
        };
        (
            status,
            status_reason,
            setup_state,
            execution_supported,
            release_gate_status,
            release_gate_passed,
            currently_runnable,
            warnings,
        )
    } else {
        (
            "unavailable".to_string(),
            status_reason,
            setup_state,
            execution_supported,
            release_gate_status,
            release_gate_passed,
            currently_runnable,
            warnings,
        )
    }
}

fn recommendation_score(provider: &ProviderControlPlaneProvider, preferred_provider: &str) -> f64 {
    let mut score = match provider.status.as_str() {
        "ready" => 100.0,
        "degraded" => 60.0,
        _ => 0.0,
    };

    if score <= 0.0 {
        return score;
    }

    let preferred_matches = canonical_provider_id(preferred_provider) == provider.provider_id;
    if preferred_matches {
        score += 20.0;
    } else if preferred_provider == "auto"
        && selection_eligible_for_auto(&provider.capabilities)
        && matches!(
            provider.release_gate_status,
            ProviderReleaseGateStatus::Passed
        )
    {
        score += 15.0;
    } else if preferred_provider == "auto"
        && !matches!(
            provider.capabilities.selection_eligibility,
            ProviderSelectionEligibility::AutoAllowed
        )
    {
        score -= 12.0;
    }

    if provider.capabilities.opt_in_only && preferred_provider == "auto" {
        score -= 30.0;
    }

    if let Some(accept) = provider.recent_metrics.avg_accept_rate {
        score += accept * 20.0;
    }
    if let Some(anchor) = provider.recent_metrics.avg_anchor_validity {
        score += anchor * 10.0;
    }
    if let Some(suppress) = provider.recent_metrics.avg_suppress_rate {
        score -= suppress * 8.0;
    }
    if let Some(latency_ms) = provider.recent_metrics.avg_latency_ms {
        score -= (latency_ms / 1000.0).min(15.0) * 0.35;
    }
    if let Some(cost) = provider.recent_metrics.avg_cost_usd {
        score -= cost.min(10.0) * 2.0;
    }

    score
}

fn provider_is_recommendable(
    provider: &ProviderControlPlaneProvider,
    preferred_provider: &str,
) -> bool {
    if preferred_provider == "auto" {
        provider.currently_runnable
            && selection_eligible_for_auto(&provider.capabilities)
            && matches!(
                provider.release_gate_status,
                ProviderReleaseGateStatus::Passed
            )
    } else {
        provider.currently_runnable && selection_eligible_for_manual(&provider.capabilities)
    }
}

fn recommendation_reason(
    provider: &ProviderControlPlaneProvider,
    preferred_provider: &str,
) -> String {
    let mut parts = Vec::new();
    parts.push(if provider.status == "ready" {
        format!("{} is ready now.", provider.display_name)
    } else {
        format!(
            "{} is the best available option right now.",
            provider.display_name
        )
    });

    if canonical_provider_id(preferred_provider) == provider.provider_id {
        parts.push("It matches the current preference.".to_string());
    } else if preferred_provider == "auto"
        && provider_is_recommendable(provider, preferred_provider)
    {
        parts.push("It fits the current auto-routing policy.".to_string());
    }

    if let Some(accept) = provider.recent_metrics.avg_accept_rate {
        parts.push(format!(
            "Recent reviewer acceptance is {}%.",
            (accept * 100.0).round() as i32
        ));
    }

    if let Some(latency_ms) = provider.recent_metrics.avg_latency_ms {
        parts.push(format!(
            "Average latency is about {:.1}s.",
            latency_ms / 1000.0
        ));
    }

    parts.join(" ")
}

fn trace_mode(
    preference: &str,
    selected_provider: &str,
    checks: &[ProviderSelectionCheck],
) -> ProviderSelectionMode {
    if preference == "auto" {
        return ProviderSelectionMode::Auto;
    }
    let selected = canonical_provider_id(selected_provider);
    if canonical_provider_id(preference) == selected
        && checks.first().is_some_and(|check| check.available)
    {
        ProviderSelectionMode::Preferred
    } else {
        ProviderSelectionMode::Fallback
    }
}

pub fn load_provider_control_inputs(
    conn: &Connection,
    workspace_id: Option<&str>,
) -> Result<(String, Vec<ReviewRun>), AppError> {
    let preferred_provider = if let Some(workspace_id) = workspace_id {
        let workspace = queries::get_workspace_by_id(conn, workspace_id)?;
        if let Some(workspace) = workspace {
            let workspace_path = Path::new(&workspace.local_path);
            let repo_config = config::load_repo_config(workspace_path);
            config::resolve_config(conn, repo_config.as_ref(), Some(workspace_path))
                .preferred_provider
        } else {
            queries::get_setting(conn, "preferred_provider")?.unwrap_or_else(|| "auto".to_string())
        }
    } else {
        queries::get_setting(conn, "preferred_provider")?.unwrap_or_else(|| "auto".to_string())
    };
    let recent_runs = queries::list_recent_review_runs_for_provider_control(
        conn,
        workspace_id,
        RECENT_RUN_WINDOW,
    )?;
    Ok((preferred_provider, recent_runs))
}

pub async fn build_provider_control_plane_snapshot(
    app: &AppHandle,
    preferred_provider: String,
    recent_runs: Vec<ReviewRun>,
    workspace_id: Option<String>,
) -> Result<ProviderControlPlaneSnapshot, AppError> {
    let statuses = credentials::all_credential_statuses()?;
    let recent_metrics = aggregate_recent_metrics(&recent_runs);
    let health_by_provider = provider_health_by_provider(app).await;

    let mut providers = Vec::new();
    for caps in provider_registry() {
        let credential_source = credential_source_for_provider(&caps.provider_id, &statuses);
        let health = health_by_provider
            .get(caps.provider_id.as_str())
            .cloned()
            .unwrap_or(ProviderHealth {
                available: false,
                version: None,
                message: Some("Provider health unavailable".to_string()),
            });
        let (
            status,
            status_reason,
            setup_state,
            execution_supported,
            release_gate_status,
            release_gate_passed,
            currently_runnable,
            warnings,
        ) = build_status(&caps, &health, credential_source);
        let recent = recent_metrics
            .get(caps.provider_id.as_str())
            .cloned()
            .unwrap_or(ProviderRecentMetrics {
                sample_count: 0,
                avg_latency_ms: None,
                avg_accept_rate: None,
                avg_edit_rate: None,
                avg_suppress_rate: None,
                avg_anchor_validity: None,
                avg_cost_usd: None,
            });
        let provider_warnings = warnings;
        let narrative = fit_narrative(&caps, &status, &recent, &provider_warnings);
        providers.push(ProviderControlPlaneProvider {
            provider_id: caps.provider_id.clone(),
            display_name: caps.display_name.clone(),
            status,
            status_reason,
            setup_state,
            execution_supported,
            release_gate_status,
            release_gate_passed,
            currently_runnable,
            credential_source,
            capabilities: caps,
            recent_metrics: recent,
            fit_narrative: narrative,
            recommended_default: false,
            warnings: provider_warnings,
        });
    }

    let recommended_provider_id = providers
        .iter()
        .filter(|provider| provider_is_recommendable(provider, &preferred_provider))
        .max_by(|left, right| {
            recommendation_score(left, &preferred_provider)
                .partial_cmp(&recommendation_score(right, &preferred_provider))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|provider| provider.provider_id.clone());

    let recommendation_reason = recommended_provider_id.as_ref().and_then(|provider_id| {
        providers
            .iter()
            .find(|provider| provider.provider_id == *provider_id)
            .map(|provider| recommendation_reason(provider, &preferred_provider))
    });

    for provider in &mut providers {
        provider.recommended_default =
            recommended_provider_id.as_deref() == Some(provider.provider_id.as_str());
    }

    Ok(ProviderControlPlaneSnapshot {
        providers,
        recommended_provider_id,
        recommendation_reason,
        preferred_provider,
        workspace_id,
        recent_window_size: RECENT_RUN_WINDOW,
        generated_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn provider_health_by_provider(app: &AppHandle) -> HashMap<String, ProviderHealth> {
    let mut health = HashMap::new();

    for caps in provider_registry() {
        if caps.execution_support_tier == "discoverable_only" {
            health.insert(
                caps.provider_id.clone(),
                ProviderHealth {
                    available: false,
                    version: None,
                    message: Some(
                        "Catalog entry only; review execution is not enabled yet.".into(),
                    ),
                },
            );
        }
    }

    let codex_app_manager = app.state::<Arc<CodexAppServerManager>>().inner().clone();
    health.insert(
        "codex_app_server".to_string(),
        CodexAppServerProvider::new(codex_app_manager)
            .health_check()
            .await,
    );

    health.insert(
        "codex".to_string(),
        CodexProvider::new(app.clone()).health_check().await,
    );
    health.insert(
        "claude".to_string(),
        ClaudeProvider::new().health_check().await,
    );

    if !health.contains_key("copilot") {
        let copilot_manager = app.state::<Arc<CopilotManager>>().inner().clone();
        health.insert(
            "copilot".to_string(),
            CopilotProvider::new(copilot_manager, None)
                .health_check()
                .await,
        );
    }

    if !health.contains_key("opencode") {
        let opencode_manager = app.state::<Arc<OpenCodeManager>>().inner().clone();
        health.insert(
            "opencode".to_string(),
            OpenCodeProvider::new(opencode_manager, None)
                .health_check()
                .await,
        );
    }

    let gemini_manager = app.state::<Arc<GeminiManager>>().inner().clone();
    health.insert(
        "gemini".to_string(),
        GeminiProvider::new(gemini_manager, None)
            .health_check()
            .await,
    );

    let cursor_manager = app.state::<Arc<CursorManager>>().inner().clone();
    health.insert(
        "cursor".to_string(),
        CursorProvider::new(cursor_manager, None)
            .health_check()
            .await,
    );

    let pi_manager = app.state::<Arc<PiManager>>().inner().clone();
    health.insert(
        "pi".to_string(),
        PiProvider::new(pi_manager, None).health_check().await,
    );

    let claude_code_manager = app.state::<Arc<ClaudeCodeManager>>().inner().clone();
    let app_data_dir = app.path().app_data_dir().unwrap_or_default();
    let sidecar_path = config::resolve_sidecar_path_pub("claude-code-bridge");
    health.insert(
        "claude_code".to_string(),
        ClaudeCodeProvider::new(claude_code_manager, sidecar_path, app_data_dir)
            .health_check()
            .await,
    );

    health
}

pub fn build_selection_trace(
    requested_provider: &str,
    selected_provider: &str,
    checks: Vec<ProviderSelectionCheck>,
) -> ProviderSelectionTrace {
    let requested = canonical_provider_id(requested_provider).to_string();
    let selected = canonical_provider_id(selected_provider).to_string();
    let selection_mode = trace_mode(&requested, &selected, &checks);
    let mut warnings = Vec::new();
    if matches!(selection_mode, ProviderSelectionMode::Fallback) && requested != selected {
        warnings.push(selection_fallback_warning(&requested, &selected, &checks));
    }

    ProviderSelectionTrace {
        requested_provider: requested.clone(),
        selected_provider: selected.clone(),
        selection_mode,
        checks,
        warnings,
    }
}

fn selection_fallback_warning(
    requested_provider: &str,
    selected_provider: &str,
    checks: &[ProviderSelectionCheck],
) -> String {
    if let Some(check) = checks
        .iter()
        .find(|check| check.provider_id == requested_provider)
    {
        match check.reason.as_str() {
            "discoverable_only" => {
                return format!(
                    "Requested provider '{}' is listed in the catalog but review execution is not enabled yet, so SignalPR selected '{}'.",
                    requested_provider, selected_provider
                );
            }
            "gate_blocked" => {
                return format!(
                    "Requested provider '{}' is healthy but still blocked by readiness checks, so SignalPR selected '{}'.",
                    requested_provider, selected_provider
                );
            }
            "opt_in_only" => {
                return format!(
                    "Requested provider '{}' requires explicit opt-in and is excluded from auto mode, so SignalPR selected '{}'.",
                    requested_provider, selected_provider
                );
            }
            "unsupported" => {
                return format!(
                    "Requested provider '{}' is not supported for review runs, so SignalPR selected '{}'.",
                    requested_provider, selected_provider
                );
            }
            "unhealthy" => {
                if let Some(message) = check.message.as_deref() {
                    return format!(
                        "Requested provider '{}' could not run ({message}), so SignalPR selected '{}'.",
                        requested_provider, selected_provider
                    );
                }
            }
            _ => {}
        }
    }

    format!(
        "Requested provider '{}' was unavailable, so SignalPR selected '{}'.",
        requested_provider, selected_provider
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::models::Workspace;
    use crate::storage::queries;
    use tempfile::tempdir;

    fn scorecard_json(provider_name: &str, accept: f64, latency_ms: i64) -> String {
        serde_json::to_string(&RunScorecard {
            lanes: vec![crate::metrics::LaneScorecard {
                lane_id: "security".into(),
                provider_name: provider_name.into(),
                lane_latency_ms: Some(latency_ms),
                raw_findings_count: 2,
                surfaced_findings_count: 2,
                reviewer_accept_rate: accept,
                reviewer_edit_rate: 0.1,
                suppress_rate: 0.1,
                anchor_validity: 1.0,
                submission_inclusion_rate: 1.0,
                cost_usd: Some(0.25),
            }],
            overall_surfaced: 2,
            overall_accept_rate: accept,
            overall_edit_rate: 0.1,
            overall_suppress_rate: 0.1,
            total_cost_usd: Some(0.25),
        })
        .unwrap()
    }

    #[test]
    fn aggregate_recent_metrics_handles_sparse_history() {
        let runs = vec![
            ReviewRun {
                id: "run-1".into(),
                pr_id: "pr-1".into(),
                status: "ready".into(),
                started_at: None,
                completed_at: None,
                error_message: None,
                head_sha_at_run: None,
                baseline_run_id: None,
                metrics_json: Some(scorecard_json("codex", 0.75, 1200)),
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
            ReviewRun {
                id: "run-2".into(),
                pr_id: "pr-2".into(),
                status: "ready".into(),
                started_at: None,
                completed_at: None,
                error_message: None,
                head_sha_at_run: None,
                baseline_run_id: None,
                metrics_json: Some(scorecard_json("codex-app-server", 0.25, 800)),
                analysis_diff_hash: None,
                analysis_diff_text: None,
                context_pack_json: None,
                local_checks_json: None,
                provider_selection_json: None,
                rerun_trigger_source: None,
                rerun_reason: None,
                rerun_scope: None,
            },
        ];

        let aggregated = aggregate_recent_metrics(&runs);
        let codex = aggregated.get("codex").unwrap();
        assert_eq!(codex.sample_count, 1);
        let app_server = aggregated.get("codex_app_server").unwrap();
        assert_eq!(app_server.sample_count, 1);
        assert_eq!(app_server.avg_latency_ms.unwrap().round(), 800.0);
    }

    #[test]
    fn build_selection_trace_marks_fallbacks() {
        let trace = build_selection_trace(
            "claude",
            "codex",
            vec![
                ProviderSelectionCheck {
                    provider_id: "claude".into(),
                    available: false,
                    reason: "health_check".into(),
                    message: Some("missing key".into()),
                },
                ProviderSelectionCheck {
                    provider_id: "codex".into(),
                    available: true,
                    reason: "selected".into(),
                    message: None,
                },
            ],
        );

        assert!(matches!(
            trace.selection_mode,
            ProviderSelectionMode::Fallback
        ));
        assert_eq!(trace.selected_provider, "codex");
        assert_eq!(trace.warnings.len(), 1);
    }

    #[test]
    fn build_selection_trace_uses_specific_gate_warning() {
        let trace = build_selection_trace(
            "codex",
            "claude",
            vec![
                ProviderSelectionCheck {
                    provider_id: "codex".into(),
                    available: false,
                    reason: "gate_blocked".into(),
                    message: None,
                },
                ProviderSelectionCheck {
                    provider_id: "claude".into(),
                    available: true,
                    reason: "selected".into(),
                    message: None,
                },
            ],
        );

        assert_eq!(
            trace.warnings,
            vec![
                "Requested provider 'codex' is healthy but still blocked by readiness checks, so SignalPR selected 'claude'."
                    .to_string()
            ]
        );
    }

    #[test]
    fn build_selection_trace_for_auto_does_not_emit_fallback_warning() {
        let trace = build_selection_trace(
            "auto",
            "codex",
            vec![ProviderSelectionCheck {
                provider_id: "codex".into(),
                available: true,
                reason: "selected".into(),
                message: None,
            }],
        );

        assert!(matches!(trace.selection_mode, ProviderSelectionMode::Auto));
        assert!(trace.warnings.is_empty());
    }

    #[test]
    fn build_status_marks_release_gated_provider_as_degraded() {
        let caps = provider_registry()
            .into_iter()
            .find(|caps| caps.provider_id == "codex")
            .expect("codex capabilities");
        let health = ProviderHealth {
            available: true,
            version: Some("1.0.0".into()),
            message: None,
        };

        let (status, reason, setup_state, _, release_gate_status, release_gate_passed, _, warnings) =
            build_status(&caps, &health, Some(CredentialSource::Environment));

        assert_eq!(status, "degraded");
        assert_eq!(setup_state, ProviderSetupState::Ready);
        assert_eq!(release_gate_status, ProviderReleaseGateStatus::BlockedEval);
        assert!(!release_gate_passed);
        assert_eq!(
            reason,
            "Healthy, but blocked until eval coverage is complete."
        );
        assert!(warnings
            .iter()
            .any(|warning| warning.contains("Eval coverage is still incomplete.")));
    }

    #[test]
    fn recommendation_reason_only_mentions_auto_policy_when_provider_is_auto_runnable() {
        let caps = provider_registry()
            .into_iter()
            .find(|caps| caps.provider_id == "codex")
            .expect("codex capabilities");
        let provider = ProviderControlPlaneProvider {
            provider_id: caps.provider_id.clone(),
            display_name: caps.display_name.clone(),
            status: "degraded".into(),
            status_reason: "blocked".into(),
            setup_state: ProviderSetupState::Ready,
            execution_supported: true,
            release_gate_status: ProviderReleaseGateStatus::BlockedEval,
            release_gate_passed: false,
            currently_runnable: true,
            credential_source: None,
            capabilities: caps,
            recent_metrics: ProviderRecentMetrics {
                sample_count: 0,
                avg_latency_ms: None,
                avg_accept_rate: None,
                avg_edit_rate: None,
                avg_suppress_rate: None,
                avg_anchor_validity: None,
                avg_cost_usd: None,
            },
            fit_narrative: String::new(),
            recommended_default: false,
            warnings: Vec::new(),
        };

        let reason = recommendation_reason(&provider, "auto");
        assert!(!reason.contains("auto-routing policy"));
    }

    #[test]
    fn load_provider_control_inputs_uses_workspace_repo_preference() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join(".signalpr.yml"),
            "preferred_provider: claude\n",
        )
        .unwrap();

        queries::upsert_setting(&conn, "preferred_provider", "codex").unwrap();
        queries::insert_workspace(
            &conn,
            &Workspace {
                id: "ws-1".into(),
                local_path: dir.path().display().to_string(),
                remote_owner: "octocat".into(),
                remote_repo: "hello-world".into(),
                created_at: "2026-05-16T00:00:00Z".into(),
                remote_host: "github.com".into(),
            },
        )
        .unwrap();

        let (preferred_provider, recent_runs) =
            load_provider_control_inputs(&conn, Some("ws-1")).unwrap();

        assert_eq!(preferred_provider, "claude");
        assert!(recent_runs.is_empty());
    }
}
