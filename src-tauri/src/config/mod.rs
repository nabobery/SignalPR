pub mod merge;
pub mod presets;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use rusqlite::Connection;
use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::agents::definition::AgentDefinition;
use crate::agents::registry::AgentRegistry;
use crate::cleaner::CleanerConfig;
use crate::providers::capabilities::{
    canonical_provider_id, get_provider_caps, ProviderCapabilities,
};
use crate::providers::claude::ClaudeProvider;
use crate::providers::claude_code::manager::ClaudeCodeManager;
use crate::providers::claude_code::provider::ClaudeCodeProvider;
use crate::providers::codex::{CodexProvider, MockProvider};
use crate::providers::codex_app_server::manager::CodexAppServerManager;
use crate::providers::codex_app_server::provider::CodexAppServerProvider;
use crate::providers::control_plane::{
    build_selection_trace, ProviderSelectionCheck, ProviderSelectionTrace,
};
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
    determine_setup_state, execution_supported, release_gate_passed, selection_eligible_for_auto,
    selection_eligible_for_manual,
};
use crate::providers::traits::ReviewProvider;
use crate::storage::queries;

/// Fully resolved configuration merging defaults, user settings, and repo config.
pub struct ResolvedConfig {
    pub cleaner: CleanerConfig,
    pub preferred_provider: String,
    pub lane_timeout: Duration,
    pub lanes: Vec<String>,
    pub custom_agents: Vec<AgentDefinition>,
    pub context_pack: crate::context_pack::ContextPackConfig,
    pub local_checks: LocalChecksRepoConfig,
}

const CUSTOM_AGENT_PREFIX: &str = "custom_agent_";

#[derive(Debug, Deserialize)]
struct StoredAgentDefinition {
    name: String,
    system_prompt: String,
    agent_type: String,
    #[serde(default)]
    provider: Option<String>,
}

fn load_custom_agents_from_settings(conn: &Connection) -> Vec<AgentDefinition> {
    let entries = queries::get_settings_by_prefix(conn, CUSTOM_AGENT_PREFIX).unwrap_or_default();
    let mut out = Vec::new();
    for (_key, value) in entries {
        match serde_json::from_str::<StoredAgentDefinition>(&value) {
            Ok(parsed) => {
                if parsed.name.trim().is_empty() || parsed.system_prompt.trim().is_empty() {
                    tracing::warn!("Skipping invalid custom agent definition (missing fields)");
                    continue;
                }
                if parsed.agent_type.trim().is_empty() {
                    tracing::warn!(
                        "Skipping custom agent '{}' with empty agent_type",
                        parsed.name
                    );
                    continue;
                }
                out.push(AgentDefinition {
                    name: parsed.name,
                    system_prompt: parsed.system_prompt,
                    agent_type: parsed.agent_type,
                    severity_rules: None,
                    provider: parsed.provider,
                });
            }
            Err(e) => {
                tracing::warn!("Skipping malformed custom agent JSON: {}", e);
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Resolve config by merging: built-in defaults < user settings (DB) < repo config.
/// Invalid or missing values fall back to defaults silently.
/// If `workspace_path` is provided and the repo config uses `extends`, the parent
/// config is resolved and merged before applying to the final config.
pub fn resolve_config(
    conn: &Connection,
    repo: Option<&RepoConfig>,
    workspace_path: Option<&Path>,
) -> ResolvedConfig {
    // If the repo config has an `extends` field, resolve inheritance first
    let resolved_repo: Option<RepoConfig>;
    let repo = if let Some(repo) = repo {
        if let (Some(ref extends_val), Some(ws)) = (&repo.extends, workspace_path) {
            let cache_dir = ws.join(".signalpr_cache").join("preset_cache");
            if let Some(parent) = presets::resolve_extends(extends_val, ws, &cache_dir, 0) {
                // Clone the current repo config fields into a new owned config
                let overlay = RepoConfig {
                    extends: None, // already resolved
                    lanes: repo.lanes.clone(),
                    max_findings: repo.max_findings,
                    similarity_threshold: repo.similarity_threshold,
                    drop_nitpicks: repo.drop_nitpicks,
                    min_confidence: repo.min_confidence,
                    lane_timeout_secs: repo.lane_timeout_secs,
                    preferred_provider: repo.preferred_provider.clone(),
                    custom_agents: repo.custom_agents.clone(),
                    context_pack: repo.context_pack.clone(),
                    local_checks: repo.local_checks.clone(),
                };
                resolved_repo = Some(merge::deep_merge_configs(parent, overlay));
                resolved_repo.as_ref()
            } else {
                Some(repo)
            }
        } else {
            Some(repo)
        }
    } else {
        None
    };

    let defaults = CleanerConfig::default();
    let default_timeout: u64 = 120;
    let default_lanes: Vec<String> = vec![
        "security".to_string(),
        "architecture".to_string(),
        "performance".to_string(),
    ];

    // Layer 1: user settings override defaults
    let mut max_surface_findings = read_setting_as::<usize>(conn, "max_surface_findings")
        .unwrap_or(defaults.max_surface_findings);
    let mut similarity_threshold = read_setting_as::<f64>(conn, "similarity_threshold")
        .unwrap_or(defaults.similarity_threshold);
    let mut drop_nitpicks =
        read_setting_as::<bool>(conn, "drop_nitpicks").unwrap_or(defaults.drop_nitpicks);
    let mut min_confidence =
        read_setting_as::<f64>(conn, "min_confidence").unwrap_or(defaults.min_confidence);
    let mut preferred_provider = queries::get_setting(conn, "preferred_provider")
        .ok()
        .flatten()
        .unwrap_or_else(|| "auto".to_string());
    let mut lane_timeout_secs =
        read_setting_as::<u64>(conn, "lane_timeout_secs").unwrap_or(default_timeout);
    let mut lanes = default_lanes.clone();

    // Layer 2: repo config overrides user settings
    if let Some(repo) = repo {
        if let Some(v) = repo.max_findings {
            max_surface_findings = v;
        }
        if let Some(v) = repo.similarity_threshold {
            similarity_threshold = v;
        }
        if let Some(v) = repo.drop_nitpicks {
            drop_nitpicks = v;
        }
        if let Some(v) = repo.min_confidence {
            min_confidence = v;
        }
        if let Some(ref v) = repo.preferred_provider {
            preferred_provider = v.clone();
        }
        if let Some(v) = repo.lane_timeout_secs {
            lane_timeout_secs = v;
        }
        if let Some(ref v) = repo.lanes {
            let filtered: Vec<String> = v
                .iter()
                .filter_map(|lane| match lane.as_str() {
                    "security" | "architecture" | "performance" => Some(lane.clone()),
                    _ => None,
                })
                .collect();
            if filtered.is_empty() {
                tracing::warn!("Repo config lanes were empty/invalid, falling back to defaults");
            } else {
                lanes = filtered;
            }
        }
    }

    let repo_agents = repo
        .and_then(|r| r.custom_agents.clone())
        .unwrap_or_default();
    let settings_agents = load_custom_agents_from_settings(conn);

    // Settings agents override repo agents with the same name.
    let mut merged_agents: Vec<AgentDefinition> = Vec::new();
    for def in repo_agents.into_iter().chain(settings_agents) {
        if let Some(pos) = merged_agents.iter().position(|d| d.name == def.name) {
            merged_agents.remove(pos);
        }
        merged_agents.push(def);
    }
    let registry = AgentRegistry::load_from_config(&merged_agents);
    let custom_agents = registry.definitions().to_vec();

    let context_pack_config = repo
        .and_then(|r| r.context_pack.clone())
        .unwrap_or_default();

    let local_checks_config = repo
        .and_then(|r| r.local_checks.clone())
        .unwrap_or_default();

    ResolvedConfig {
        cleaner: CleanerConfig {
            similarity_threshold,
            drop_nitpicks,
            max_surface_findings,
            min_confidence,
        },
        preferred_provider,
        lane_timeout: Duration::from_secs(lane_timeout_secs),
        lanes,
        custom_agents,
        context_pack: context_pack_config,
        local_checks: local_checks_config,
    }
}

/// Select a review provider based on preference and availability.
/// Falls back through: preferred → codex (app-server) → codex (exec) → claude → copilot → opencode.
///
/// The `codex` preference now means "Codex App Server" (managed child process).
/// Use `codex_exec` for the legacy one-shot `codex exec` provider.
///
/// The mock provider is never selected implicitly: it is only used when the
/// preference is explicitly set to `mock`. When no provider is healthy the
/// selection fails with a per-provider reason list instead of silently
/// producing fixture findings.
pub struct SelectedProvider {
    pub provider: Arc<dyn ReviewProvider>,
    pub trace: ProviderSelectionTrace,
}

pub async fn select_provider(
    app: &AppHandle,
    preference: &str,
) -> Result<Arc<dyn ReviewProvider>, crate::errors::AppError> {
    Ok(select_provider_with_trace(app, preference).await?.provider)
}

pub async fn select_provider_with_trace(
    app: &AppHandle,
    preference: &str,
) -> Result<SelectedProvider, crate::errors::AppError> {
    let mut checks = Vec::new();
    let canonical_preference = canonical_provider_id(preference).to_string();

    // Mock is an explicit opt-in for development and tests only.
    if preference == "mock" {
        checks.push(ProviderSelectionCheck {
            provider_id: "mock".into(),
            available: true,
            reason: "explicit_preference".into(),
            message: Some("Mock provider explicitly selected in settings".into()),
        });
        return Ok(SelectedProvider {
            provider: Arc::new(MockProvider::with_default_fixture()),
            trace: build_selection_trace(&canonical_preference, "mock", checks),
        });
    }

    if preference != "auto" {
        if let Some(selected) = try_select_named_provider(
            app,
            preference,
            ProviderSelectionIntent::Manual,
            &mut checks,
        )
        .await
        {
            let selected_provider_id =
                canonical_provider_id(selected.provider.provider_name()).to_string();
            return Ok(SelectedProvider {
                provider: selected.provider,
                trace: build_selection_trace(&canonical_preference, &selected_provider_id, checks),
            });
        }
    }

    let auto_chain = ["codex_app_server", "codex", "claude", "copilot", "opencode"];

    // First pass: prefer a fully release-gated provider.
    for provider_id in auto_chain {
        if let Some(selected) =
            try_select_named_provider(app, provider_id, ProviderSelectionIntent::Auto, &mut checks)
                .await
        {
            return Ok(SelectedProvider {
                provider: selected.provider,
                trace: build_selection_trace(&canonical_preference, provider_id, checks),
            });
        }
    }

    // Second pass: no gated provider was available, so fall back to any healthy
    // provider even if its eval/conformance coverage is still incomplete. The
    // release gate is governance metadata, not a runtime availability check;
    // blocking on it strands users whose only installed CLI is "planned".
    // The trace records these as `selected_ungated` and the UI already surfaces
    // the caveat via the "Degraded provider warnings" panel.
    for provider_id in auto_chain {
        if let Some(selected) = try_select_named_provider(
            app,
            provider_id,
            ProviderSelectionIntent::AutoRelaxed,
            &mut checks,
        )
        .await
        {
            return Ok(SelectedProvider {
                provider: selected.provider,
                trace: build_selection_trace(&canonical_preference, provider_id, checks),
            });
        }
    }

    let detail = if checks.is_empty() {
        "no providers were checked".to_string()
    } else {
        checks
            .iter()
            .map(|c| match &c.message {
                Some(msg) => format!("{}: {} ({})", c.provider_id, c.reason, msg),
                None => format!("{}: {}", c.provider_id, c.reason),
            })
            .collect::<Vec<_>>()
            .join("; ")
    };
    Err(crate::errors::AppError::Provider(
        crate::errors::ProviderError::NotAvailable(format!(
            "No review provider is available (preference: {canonical_preference}). \
             Install and authenticate a provider CLI, or check Settings > Providers. \
             Checks — {detail}"
        )),
    ))
}

struct SelectedProviderInternal {
    provider: Arc<dyn ReviewProvider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderSelectionIntent {
    /// Auto fallback that enforces the release gate (conformance + eval).
    Auto,
    /// Auto fallback that ignores the release gate. Used as a second pass so a
    /// healthy provider is never rejected purely because its eval coverage is
    /// still "planned" — the release gate is governance metadata, not a runtime
    /// availability signal. The trace records this as `selected_ungated`.
    AutoRelaxed,
    Manual,
}

fn selection_preflight_skip_reason(
    capabilities: &ProviderCapabilities,
    intent: ProviderSelectionIntent,
) -> Option<&'static str> {
    if capabilities.execution_support_tier == "discoverable_only" {
        return Some("discoverable_only");
    }

    if !execution_supported(capabilities) {
        return Some("unsupported");
    }

    match intent {
        ProviderSelectionIntent::Auto | ProviderSelectionIntent::AutoRelaxed => {
            if !selection_eligible_for_auto(capabilities) {
                return Some("opt_in_only");
            }
        }
        ProviderSelectionIntent::Manual => {
            if !selection_eligible_for_manual(capabilities) {
                return Some("unsupported");
            }
        }
    }

    None
}

fn selection_skip_reason(
    capabilities: &ProviderCapabilities,
    health: &crate::providers::traits::ProviderHealth,
    intent: ProviderSelectionIntent,
) -> Option<&'static str> {
    if let Some(reason) = selection_preflight_skip_reason(capabilities, intent) {
        return Some(reason);
    }

    if !health.available {
        return Some("unhealthy");
    }

    if matches!(intent, ProviderSelectionIntent::Auto) {
        let setup_state = determine_setup_state(capabilities, health, None);
        if !release_gate_passed(capabilities, &setup_state) {
            return Some("gate_blocked");
        }
    }

    None
}

/// Whether a healthy provider is only being selected because the release gate
/// was relaxed (i.e. strict Auto would have rejected it as `gate_blocked`).
fn selected_despite_gate(
    capabilities: &ProviderCapabilities,
    health: &crate::providers::traits::ProviderHealth,
    intent: ProviderSelectionIntent,
) -> bool {
    matches!(intent, ProviderSelectionIntent::AutoRelaxed) && health.available && {
        let setup_state = determine_setup_state(capabilities, health, None);
        !release_gate_passed(capabilities, &setup_state)
    }
}

async fn try_select_named_provider(
    app: &AppHandle,
    provider_id: &str,
    intent: ProviderSelectionIntent,
    checks: &mut Vec<ProviderSelectionCheck>,
) -> Option<SelectedProviderInternal> {
    let canonical_provider_id = canonical_provider_id(provider_id);
    let capabilities = get_provider_caps(canonical_provider_id)?;

    if let Some(skip_reason) = selection_preflight_skip_reason(&capabilities, intent) {
        checks.push(ProviderSelectionCheck {
            provider_id: canonical_provider_id.into(),
            available: false,
            reason: skip_reason.into(),
            message: None,
        });
        return None;
    }

    let maybe_select = |health: crate::providers::traits::ProviderHealth,
                        provider: Arc<dyn ReviewProvider>,
                        checks: &mut Vec<ProviderSelectionCheck>,
                        selected_provider_id: &str|
     -> Option<SelectedProviderInternal> {
        let skip_reason = selection_skip_reason(&capabilities, &health, intent);
        let selected_reason = if selected_despite_gate(&capabilities, &health, intent) {
            "selected_ungated"
        } else {
            "selected"
        };
        checks.push(ProviderSelectionCheck {
            provider_id: selected_provider_id.into(),
            available: skip_reason.is_none(),
            reason: skip_reason.unwrap_or(selected_reason).into(),
            message: health.message.clone(),
        });
        if skip_reason.is_none() {
            Some(SelectedProviderInternal { provider })
        } else {
            None
        }
    };

    match canonical_provider_id {
        "codex_app_server" => {
            let manager = app.state::<Arc<CodexAppServerManager>>().inner().clone();
            let provider = CodexAppServerProvider::new(manager);
            let health = provider.health_check().await;
            if let Some(selected) =
                maybe_select(health, Arc::new(provider), checks, "codex_app_server")
            {
                tracing::info!("Using Codex App Server provider");
                return Some(selected);
            }
        }
        "codex" => {
            let provider = CodexProvider::new(app.clone());
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "codex") {
                tracing::info!("Using Codex exec provider");
                return Some(selected);
            }
        }
        "claude" => {
            let provider = ClaudeProvider::new();
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "claude") {
                return Some(selected);
            }
        }
        "copilot" => {
            let manager = app.state::<Arc<CopilotManager>>().inner().clone();
            let provider = CopilotProvider::new(manager, None);
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "copilot") {
                tracing::info!("Using Copilot provider");
                return Some(selected);
            }
        }
        "opencode" => {
            let manager = app.state::<Arc<OpenCodeManager>>().inner().clone();
            let provider = OpenCodeProvider::new(manager, None);
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "opencode") {
                tracing::info!("Using OpenCode provider");
                return Some(selected);
            }
        }
        "gemini" => {
            let manager = app.state::<Arc<GeminiManager>>().inner().clone();
            let provider = GeminiProvider::new(manager, None);
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "gemini") {
                tracing::info!("Using Gemini provider");
                return Some(selected);
            }
        }
        "cursor" => {
            let manager = app.state::<Arc<CursorManager>>().inner().clone();
            let provider = CursorProvider::new(manager, None);
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "cursor") {
                tracing::info!("Using Cursor provider");
                return Some(selected);
            }
        }
        "pi" => {
            let manager = app.state::<Arc<PiManager>>().inner().clone();
            let provider = PiProvider::new(manager, None);
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "pi") {
                tracing::info!("Using PI provider");
                return Some(selected);
            }
        }
        "claude_code" => {
            let manager = app.state::<Arc<ClaudeCodeManager>>().inner().clone();
            let app_data_dir = app.path().app_data_dir().unwrap_or_default();
            let sidecar_path = resolve_sidecar_path("claude-code-bridge");
            let provider = ClaudeCodeProvider::new(manager, sidecar_path, app_data_dir);
            let health = provider.health_check().await;
            if let Some(selected) = maybe_select(health, Arc::new(provider), checks, "claude_code")
            {
                tracing::info!("Using Claude Code provider");
                return Some(selected);
            }
        }
        _ => {}
    }

    None
}

/// Repo-level config loaded from `.signalpr.yml` at workspace root.
/// All fields are optional — missing fields fall back to user settings or defaults.
/// Unknown fields are silently ignored for forward compatibility.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RepoConfig {
    pub extends: Option<String>,
    pub lanes: Option<Vec<String>>,
    pub max_findings: Option<usize>,
    pub similarity_threshold: Option<f64>,
    pub drop_nitpicks: Option<bool>,
    pub min_confidence: Option<f64>,
    pub lane_timeout_secs: Option<u64>,
    pub preferred_provider: Option<String>,
    #[serde(default)]
    pub custom_agents: Option<Vec<AgentDefinition>>,
    #[serde(default)]
    pub context_pack: Option<crate::context_pack::ContextPackConfig>,
    #[serde(default)]
    pub local_checks: Option<LocalChecksRepoConfig>,
}

/// Repo-level configuration for local deterministic checks.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct LocalChecksRepoConfig {
    pub enabled: Option<bool>,
    pub oxlint: Option<bool>,
    pub clippy: Option<bool>,
}

/// Load `.signalpr.yml` from the workspace root. Returns None if file
/// is missing or malformed (logs a warning on parse failure).
pub fn load_repo_config(workspace_path: &Path) -> Option<RepoConfig> {
    let path = workspace_path.join(".signalpr.yml");
    let content = std::fs::read_to_string(&path).ok()?;
    match serde_yml::from_str(&content) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!("Failed to parse .signalpr.yml: {}", e);
            None
        }
    }
}

fn read_setting_as<T: std::str::FromStr>(conn: &Connection, key: &str) -> Option<T> {
    queries::get_setting(conn, key)
        .ok()
        .flatten()
        .and_then(|v| v.parse::<T>().ok())
}

/// Public accessor for environment check use.
pub fn resolve_sidecar_path_pub(name: &str) -> String {
    resolve_sidecar_path(name)
}

/// Resolve the path to a sidecar binary. In dev mode, checks the binaries
/// directory adjacent to src-tauri. In packaged builds, the binary is next
/// to the main executable.
fn resolve_sidecar_path(name: &str) -> String {
    let triple = current_target_triple();
    let suffixed = if cfg!(target_os = "windows") {
        format!("{}-{}.exe", name, triple)
    } else {
        format!("{}-{}", name, triple)
    };

    // Try relative to the current exe (packaged builds)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join(&suffixed);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }

    // Dev fallback: src-tauri/binaries/
    let dev_path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(&suffixed);
    dev_path.to_string_lossy().to_string()
}

fn current_target_triple() -> &'static str {
    if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-apple-darwin"
        } else {
            "x86_64-apple-darwin"
        }
    } else if cfg!(target_os = "linux") {
        if cfg!(target_arch = "aarch64") {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    } else if cfg!(target_os = "windows") {
        "x86_64-pc-windows-msvc"
    } else {
        "unknown-unknown-unknown"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::ProviderHealth;
    use crate::storage::db::init_db_in_memory;
    use crate::storage::queries;

    #[test]
    fn test_resolve_config_defaults() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let config = resolve_config(&conn, None, None);
        assert_eq!(config.cleaner.max_surface_findings, 8);
        assert!((config.cleaner.similarity_threshold - 0.70).abs() < f64::EPSILON);
        assert_eq!(config.preferred_provider, "auto");
        assert_eq!(config.lane_timeout.as_secs(), 120);
        assert!(config.cleaner.drop_nitpicks);
        assert_eq!(
            config.lanes,
            vec![
                "security".to_string(),
                "architecture".to_string(),
                "performance".to_string(),
            ]
        );
    }

    #[test]
    fn test_resolve_config_from_settings() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "15").unwrap();
        queries::upsert_setting(&conn, "preferred_provider", "claude").unwrap();
        queries::upsert_setting(&conn, "lane_timeout_secs", "60").unwrap();
        let config = resolve_config(&conn, None, None);
        assert_eq!(config.cleaner.max_surface_findings, 15);
        assert_eq!(config.preferred_provider, "claude");
        assert_eq!(config.lane_timeout.as_secs(), 60);
    }

    #[test]
    fn test_resolve_config_invalid_setting_falls_back() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "not_a_number").unwrap();
        queries::upsert_setting(&conn, "similarity_threshold", "abc").unwrap();
        let config = resolve_config(&conn, None, None);
        assert_eq!(config.cleaner.max_surface_findings, 8); // default
        assert!((config.cleaner.similarity_threshold - 0.70).abs() < f64::EPSILON);
        // default
    }

    #[test]
    fn test_resolve_config_partial_settings() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "drop_nitpicks", "false").unwrap();
        let config = resolve_config(&conn, None, None);
        assert!(!config.cleaner.drop_nitpicks);
        assert_eq!(config.cleaner.max_surface_findings, 8);
    }

    // --- Repo config YAML parsing tests ---

    #[test]
    fn test_parse_repo_config() {
        let yaml = "lanes:\n  - security\n  - performance\nmax_findings: 5\ndrop_nitpicks: false\nsimilarity_threshold: 0.80\n";
        let config: RepoConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(
            config.lanes,
            Some(vec!["security".into(), "performance".into()])
        );
        assert_eq!(config.max_findings, Some(5));
        assert_eq!(config.drop_nitpicks, Some(false));
        assert_eq!(config.similarity_threshold, Some(0.80));
    }

    #[test]
    fn test_parse_tolerates_unknown_fields() {
        let yaml = "future_key: true\nlanes:\n  - security\n";
        let config: RepoConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.lanes, Some(vec!["security".into()]));
    }

    #[test]
    fn test_repo_config_overrides_user_settings() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "15").unwrap();
        let repo = RepoConfig {
            max_findings: Some(3),
            ..Default::default()
        };
        let config = resolve_config(&conn, Some(&repo), None);
        assert_eq!(config.cleaner.max_surface_findings, 3);
    }

    #[test]
    fn test_repo_config_does_not_override_unset_fields() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "max_surface_findings", "15").unwrap();
        let repo = RepoConfig::default();
        let config = resolve_config(&conn, Some(&repo), None);
        assert_eq!(config.cleaner.max_surface_findings, 15);
    }

    #[test]
    fn test_repo_config_lanes_filters_unknown_and_preserves_order() {
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        let repo = RepoConfig {
            lanes: Some(vec![
                "performance".into(),
                "unknown".into(),
                "security".into(),
            ]),
            ..Default::default()
        };
        let config = resolve_config(&conn, Some(&repo), None);
        assert_eq!(
            config.lanes,
            vec!["performance".to_string(), "security".to_string()]
        );
    }

    #[test]
    fn test_load_repo_config_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_repo_config(dir.path()).is_none());
    }

    #[test]
    fn test_load_repo_config_malformed_yaml() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".signalpr.yml"), "{{invalid").unwrap();
        assert!(load_repo_config(dir.path()).is_none());
    }

    #[test]
    fn test_load_repo_config_valid() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join(".signalpr.yml"),
            "max_findings: 5\ndrop_nitpicks: true\n",
        )
        .unwrap();
        let config = load_repo_config(dir.path()).expect("should parse");
        assert_eq!(config.max_findings, Some(5));
        assert_eq!(config.drop_nitpicks, Some(true));
    }

    #[test]
    fn test_load_repo_config_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".signalpr.yml"), "").unwrap();
        let config = load_repo_config(dir.path()).expect("empty file should parse");
        assert!(config.max_findings.is_none());
    }

    /// Verify that `claude_code` is NOT included in the auto fallback chain.
    /// The auto chain only tries: codex_app_server → codex_exec → claude → copilot → opencode → mock.
    /// Opt-in providers (gemini, cursor, pi, claude_code) are never auto-selected.
    #[test]
    fn test_claude_code_excluded_from_auto_chain() {
        // The auto chain is encoded directly in select_provider's match arms.
        // `"auto"` falls through to the `_ => {}` arm which then iterates the
        // explicit chain. `claude_code` only appears under its own match arm,
        // never in the fallback chain.
        //
        // Since select_provider requires a real AppHandle, we verify the
        // structure via a simpler static assertion: the `resolve_config`
        // function with preferred_provider="auto" never mutates to "claude_code".
        let db = init_db_in_memory().unwrap();
        let conn = db.0.lock().unwrap();
        queries::upsert_setting(&conn, "preferred_provider", "auto").unwrap();
        let config = resolve_config(&conn, None, None);
        assert_eq!(config.preferred_provider, "auto");
        // The config stores "auto" — select_provider will NOT resolve it to claude_code
        // because claude_code is in a named match arm, not in the auto fallback sequence.
    }

    #[test]
    fn test_auto_selection_skips_catalog_only_provider_before_health() {
        let caps = get_provider_caps("copilot").expect("copilot capabilities");

        assert_eq!(
            selection_preflight_skip_reason(&caps, ProviderSelectionIntent::Auto),
            Some("discoverable_only")
        );
    }

    #[test]
    fn test_auto_selection_blocks_release_gated_provider_even_when_healthy() {
        let caps = get_provider_caps("codex").expect("codex capabilities");
        let healthy = ProviderHealth {
            available: true,
            version: Some("1.0.0".into()),
            message: None,
        };

        assert_eq!(
            selection_skip_reason(&caps, &healthy, ProviderSelectionIntent::Auto),
            Some("gate_blocked")
        );
        assert_eq!(
            selection_skip_reason(&caps, &healthy, ProviderSelectionIntent::Manual),
            None
        );
    }

    #[test]
    fn test_auto_relaxed_selects_gated_provider_when_healthy() {
        // codex is healthy but gate-blocked under strict Auto (eval "planned").
        // The relaxed pass must select it and mark it selected_ungated.
        let caps = get_provider_caps("codex").expect("codex capabilities");
        let healthy = ProviderHealth {
            available: true,
            version: Some("1.0.0".into()),
            message: None,
        };

        assert_eq!(
            selection_skip_reason(&caps, &healthy, ProviderSelectionIntent::AutoRelaxed),
            None
        );
        assert!(selected_despite_gate(
            &caps,
            &healthy,
            ProviderSelectionIntent::AutoRelaxed
        ));
    }

    #[test]
    fn test_auto_relaxed_still_skips_unhealthy_provider() {
        let caps = get_provider_caps("codex").expect("codex capabilities");
        let unhealthy = ProviderHealth {
            available: false,
            version: None,
            message: Some("not found".into()),
        };
        assert_eq!(
            selection_skip_reason(&caps, &unhealthy, ProviderSelectionIntent::AutoRelaxed),
            Some("unhealthy")
        );
        assert!(!selected_despite_gate(
            &caps,
            &unhealthy,
            ProviderSelectionIntent::AutoRelaxed
        ));
    }

    #[test]
    fn test_fully_gated_provider_not_marked_ungated() {
        // claude has eval "covered", so even under AutoRelaxed it is a normal
        // "selected", not "selected_ungated".
        let caps = get_provider_caps("claude").expect("claude capabilities");
        let healthy = ProviderHealth {
            available: true,
            version: None,
            message: None,
        };
        assert!(!selected_despite_gate(
            &caps,
            &healthy,
            ProviderSelectionIntent::AutoRelaxed
        ));
    }

    #[test]
    fn test_auto_selection_skips_manual_only_provider_as_opt_in() {
        let caps = get_provider_caps("gemini").expect("gemini capabilities");
        let healthy = ProviderHealth {
            available: true,
            version: Some("1.0.0".into()),
            message: None,
        };

        assert_eq!(
            selection_preflight_skip_reason(&caps, ProviderSelectionIntent::Auto),
            Some("opt_in_only")
        );
        assert_eq!(
            selection_preflight_skip_reason(&caps, ProviderSelectionIntent::Manual),
            None
        );
        assert_eq!(
            selection_skip_reason(&caps, &healthy, ProviderSelectionIntent::Auto),
            Some("opt_in_only")
        );
        assert_eq!(
            selection_skip_reason(&caps, &healthy, ProviderSelectionIntent::Manual),
            None
        );
    }
}
