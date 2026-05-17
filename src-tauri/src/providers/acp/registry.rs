use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::errors::AppError;
use crate::providers::acp::shared::{AcpConfigOptionDescriptor, AcpConfigOptionValue};
use crate::providers::capabilities::{provider_registry, ProviderCapabilities};
use crate::providers::control_plane::provider_health_by_provider;
use crate::providers::setup::{
    currently_runnable, determine_setup_state, execution_supported, release_gate_passed,
    release_gate_status, support_tier, ProviderReleaseGateStatus, ProviderSetupState,
};
use crate::providers::traits::ProviderHealth;
use crate::secrets::credentials::{self, CredentialSource};

const ACP_REGISTRY_URL: &str =
    "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
const ACP_REGISTRY_CACHE: &str = "acp-registry-cache.json";
const ACP_REGISTRY_CACHE_TTL_SECS: i64 = 60 * 60;
const ACP_REGISTRY_LIVE_FETCH_TIMEOUT_SECS: u64 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupActionKind {
    Verify,
    OpenDocs,
    OpenInstall,
    OpenAuthDocs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSetupAction {
    pub id: String,
    pub label: String,
    pub kind: SetupActionKind,
    pub enabled: bool,
    pub command_preview: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRegistryMetadata {
    pub registry_id: Option<String>,
    pub latest_version: Option<String>,
    pub install_source: String,
    pub distribution_channel: String,
    pub install_command: Option<String>,
    pub install_url: Option<String>,
    pub docs_url: Option<String>,
    pub auth_docs_url: Option<String>,
    pub config_options: Vec<AcpConfigOptionDescriptor>,
    pub supported_modes: Vec<String>,
    pub session_capabilities: crate::providers::acp::shared::AcpSessionCapabilities,
    pub setup_notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSetupCatalogEntry {
    pub provider_id: String,
    pub display_name: String,
    pub provider_family: String,
    pub setup_state: ProviderSetupState,
    pub readiness_reason: String,
    pub support_tier: String,
    pub execution_supported: bool,
    pub release_gate_status: ProviderReleaseGateStatus,
    pub release_gate_passed: bool,
    pub currently_runnable: bool,
    pub credential_source: Option<CredentialSource>,
    pub capabilities: ProviderCapabilities,
    pub registry: Option<ProviderRegistryMetadata>,
    pub actions: Vec<ProviderSetupAction>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSetupCatalogSnapshot {
    pub providers: Vec<ProviderSetupCatalogEntry>,
    pub registry_fetched_at: Option<String>,
    pub registry_source: String,
    pub generated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSetupProbeResult {
    pub provider_id: String,
    pub setup_state: ProviderSetupState,
    pub ready: bool,
    pub reason: String,
    pub checked_at: String,
}

#[derive(Debug, Clone)]
struct RegistrySeed {
    registry_candidates: &'static [&'static str],
    install_command: Option<&'static str>,
    install_url: Option<&'static str>,
    docs_url: Option<&'static str>,
    auth_docs_url: Option<&'static str>,
    setup_notes: &'static [&'static str],
    supported_modes: &'static [&'static str],
    config_options: &'static [(&'static str, &'static str, &'static [&'static str])],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedRegistryPayload {
    fetched_at: String,
    payload: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RegistryCacheFreshness {
    Fresh,
    Stale,
    InvalidTimestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RegistryAgentRecord {
    registry_id: String,
    latest_version: Option<String>,
    install_command: Option<String>,
    install_url: Option<String>,
    docs_url: Option<String>,
}

fn seeded_registry_metadata_by_provider() -> HashMap<&'static str, RegistrySeed> {
    HashMap::from([
        (
            "gemini",
            RegistrySeed {
                registry_candidates: &["gemini", "gemini-cli", "google-gemini"],
                install_command: Some("npm i -g @google/gemini-cli"),
                install_url: Some("https://github.com/google-gemini/gemini-cli"),
                docs_url: Some("https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/authentication.md"),
                auth_docs_url: Some("https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/authentication.md"),
                setup_notes: &[
                    "Use an API key or Vertex credentials. OAuth is intentionally unsupported for third-party harnesses.",
                    "SignalPR runs Gemini in ACP plan mode when available.",
                    "Review lanes stay deny-by-default for permission requests.",
                ],
                supported_modes: &["plan"],
                config_options: &[
                    ("mode", "Session Mode", &["plan"]),
                    ("model", "Model", &["gemini-2.5-pro", "gemini-2.5-flash", "auto"]),
                ],
            },
        ),
        (
            "cursor",
            RegistrySeed {
                registry_candidates: &["cursor", "cursor-cli", "cursor-agent"],
                install_command: Some("curl https://cursor.com/install -fsS | bash"),
                install_url: Some("https://cursor.com/docs/cli/acp"),
                docs_url: Some("https://cursor.com/docs/cli/acp"),
                auth_docs_url: Some("https://cursor.com/docs/cli/reference/authentication"),
                setup_notes: &[
                    "Generate CURSOR_API_KEY from Cursor Dashboard -> Cloud Agents -> User API Keys.",
                    "SignalPR uses Cursor over ACP and keeps review runs in ask mode.",
                    "Filesystem reads stay sandboxed to the PR worktree.",
                ],
                supported_modes: &["ask"],
                config_options: &[
                    ("mode", "Session Mode", &["ask"]),
                    (
                        "model",
                        "Model",
                        &["auto", "gpt-5.2", "sonnet-4.5", "sonnet-4.5-thinking", "opus-4.6"],
                    ),
                ],
            },
        ),
        (
            "copilot",
            RegistrySeed {
                registry_candidates: &["github-copilot", "copilot"],
                install_command: None,
                install_url: Some("https://agentclientprotocol.com/get-started/registry"),
                docs_url: Some("https://agentclientprotocol.com/get-started/registry"),
                auth_docs_url: None,
                setup_notes: &["Listed for ecosystem awareness only. Review runs stay unavailable."],
                supported_modes: &[],
                config_options: &[],
            },
        ),
        (
            "opencode",
            RegistrySeed {
                registry_candidates: &["opencode"],
                install_command: None,
                install_url: Some("https://agentclientprotocol.com/get-started/registry"),
                docs_url: Some("https://agentclientprotocol.com/get-started/registry"),
                auth_docs_url: None,
                setup_notes: &["Visible for discovery, but not yet available for review runs."],
                supported_modes: &[],
                config_options: &[],
            },
        ),
        (
            "pi",
            RegistrySeed {
                registry_candidates: &["pi-acp", "pi"],
                install_command: Some("npm i -g @mariozechner/pi-coding-agent"),
                install_url: Some("https://agentclientprotocol.com/get-started/registry"),
                docs_url: Some("https://agentclientprotocol.com/get-started/registry"),
                auth_docs_url: None,
                setup_notes: &["Visible for discovery, but not yet available for review runs."],
                supported_modes: &[],
                config_options: &[],
            },
        ),
    ])
}

fn config_option_seed(id: &str, name: &str, values: &[&str]) -> AcpConfigOptionDescriptor {
    AcpConfigOptionDescriptor {
        id: id.to_string(),
        name: name.to_string(),
        option_type: "select".to_string(),
        current_value: None,
        options: values
            .iter()
            .map(|value| AcpConfigOptionValue {
                value: (*value).to_string(),
                label: (*value).to_string(),
                description: None,
            })
            .collect(),
    }
}

fn cache_path(app: &AppHandle) -> Result<PathBuf, AppError> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|error| AppError::InvalidInput(error.to_string()))?;
    Ok(app_dir.join(ACP_REGISTRY_CACHE))
}

async fn read_cached_registry(app: &AppHandle) -> Option<CachedRegistryPayload> {
    let path = cache_path(app).ok()?;
    let bytes = tokio::fs::read(path).await.ok()?;
    serde_json::from_slice::<CachedRegistryPayload>(&bytes).ok()
}

async fn write_cached_registry(
    app: &AppHandle,
    payload: &CachedRegistryPayload,
) -> Result<(), AppError> {
    let path = cache_path(app)?;
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, serde_json::to_vec_pretty(payload)?).await?;
    Ok(())
}

async fn fetch_live_registry(app: &AppHandle) -> Result<CachedRegistryPayload, AppError> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(ACP_REGISTRY_LIVE_FETCH_TIMEOUT_SECS))
        .build()?;
    let payload = client
        .get(ACP_REGISTRY_URL)
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;

    let cached = CachedRegistryPayload {
        fetched_at: chrono::Utc::now().to_rfc3339(),
        payload,
    };
    write_cached_registry(app, &cached).await?;
    Ok(cached)
}

fn cache_freshness(payload: &CachedRegistryPayload) -> RegistryCacheFreshness {
    let Ok(fetched_at) = chrono::DateTime::parse_from_rfc3339(&payload.fetched_at) else {
        return RegistryCacheFreshness::InvalidTimestamp;
    };
    let age = chrono::Utc::now().signed_duration_since(fetched_at.with_timezone(&chrono::Utc));
    if age.num_seconds() <= ACP_REGISTRY_CACHE_TTL_SECS {
        RegistryCacheFreshness::Fresh
    } else {
        RegistryCacheFreshness::Stale
    }
}

fn maybe_refresh_registry_in_background(app: &AppHandle, freshness: RegistryCacheFreshness) {
    if freshness == RegistryCacheFreshness::Fresh {
        return;
    }

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = fetch_live_registry(&app).await;
    });
}

async fn load_registry_payload(app: &AppHandle) -> (Option<CachedRegistryPayload>, String) {
    if let Some(payload) = read_cached_registry(app).await {
        let freshness = cache_freshness(&payload);
        maybe_refresh_registry_in_background(app, freshness);
        let source = match freshness {
            RegistryCacheFreshness::Fresh => "cache_fresh",
            RegistryCacheFreshness::Stale => "cache_stale",
            RegistryCacheFreshness::InvalidTimestamp => "cache_unknown_age",
        };
        return (Some(payload), source.to_string());
    }

    match fetch_live_registry(app).await {
        Ok(payload) => (Some(payload), "live".to_string()),
        Err(_) => (None, "seed".to_string()),
    }
}

fn normalized_registry_id(value: &str) -> String {
    value.trim().to_lowercase().replace('_', "-")
}

fn registry_id_matches(object: &serde_json::Map<String, Value>, candidates: &[&str]) -> bool {
    let Some(candidate_id) = [
        object.get("id").and_then(|value| value.as_str()),
        object.get("agentId").and_then(|value| value.as_str()),
        object.get("slug").and_then(|value| value.as_str()),
    ]
    .into_iter()
    .flatten()
    .next() else {
        return false;
    };

    let candidate_id = normalized_registry_id(candidate_id);
    candidates
        .iter()
        .map(|candidate| normalized_registry_id(candidate))
        .any(|candidate| candidate == candidate_id)
}

fn parse_registry_agent_record(
    object: &serde_json::Map<String, Value>,
) -> Option<RegistryAgentRecord> {
    let registry_id = object
        .get("id")
        .or_else(|| object.get("agentId"))
        .or_else(|| object.get("slug"))
        .and_then(|value| value.as_str())?
        .to_string();

    let latest_version = object
        .get("version")
        .or_else(|| object.get("latestVersion"))
        .or_else(|| object.get("currentVersion"))
        .and_then(|value| {
            value
                .as_str()
                .map(String::from)
                .or_else(|| value.as_u64().map(|number| number.to_string()))
        });

    let install_command = object
        .get("installCommand")
        .and_then(|value| value.as_str())
        .map(String::from);
    let install_url = object
        .get("installUrl")
        .or_else(|| object.get("downloadUrl"))
        .and_then(|value| value.as_str())
        .map(String::from);
    let docs_url = object
        .get("docsUrl")
        .or_else(|| object.get("documentation"))
        .or_else(|| object.get("homepage"))
        .or_else(|| object.get("website"))
        .and_then(|value| value.as_str())
        .map(String::from);

    if latest_version.is_none()
        && install_command.is_none()
        && install_url.is_none()
        && docs_url.is_none()
    {
        return None;
    }

    Some(RegistryAgentRecord {
        registry_id,
        latest_version,
        install_command,
        install_url,
        docs_url,
    })
}

fn extract_registry_record(payload: &Value, candidates: &[&str]) -> Option<RegistryAgentRecord> {
    let mut collections = Vec::new();

    match payload {
        Value::Array(array) => collections.push(array),
        Value::Object(map) => {
            for candidate in candidates {
                if let Some(object) = map
                    .get(&normalized_registry_id(candidate))
                    .and_then(|value| value.as_object())
                {
                    let mut with_id = object.clone();
                    with_id
                        .entry("id".to_string())
                        .or_insert_with(|| Value::String(normalized_registry_id(candidate)));
                    if let Some(record) = parse_registry_agent_record(&with_id) {
                        return Some(record);
                    }
                }
            }
            for key in ["agents", "entries", "items"] {
                if let Some(array) = map.get(key).and_then(|value| value.as_array()) {
                    collections.push(array);
                }
            }
        }
        _ => {}
    }

    for collection in collections {
        for value in collection {
            let Some(object) = value.as_object() else {
                continue;
            };
            if !registry_id_matches(object, candidates) {
                continue;
            }
            if let Some(record) = parse_registry_agent_record(object) {
                return Some(record);
            }
        }
    }

    None
}

pub async fn build_provider_setup_catalog(
    app: &AppHandle,
) -> Result<ProviderSetupCatalogSnapshot, AppError> {
    let seeds = seeded_registry_metadata_by_provider();
    let (registry_payload, registry_source) = load_registry_payload(app).await;
    let registry_fetched_at = registry_payload
        .as_ref()
        .map(|payload| payload.fetched_at.clone());
    let statuses = credentials::all_credential_statuses()?;
    let health = provider_health_by_provider(app).await;

    let mut providers = Vec::new();
    for capabilities in provider_registry() {
        let health_state = health.get(capabilities.provider_id.as_str());
        let credential_source = statuses
            .iter()
            .filter(|status| {
                status
                    .provider_ids
                    .iter()
                    .any(|provider_id| provider_id == &capabilities.provider_id)
            })
            .map(|status| status.source)
            .max_by_key(|source| match source {
                CredentialSource::Environment => 3,
                CredentialSource::Keychain => 2,
                CredentialSource::None => 1,
            });

        let health_state = health_state.cloned().unwrap_or(ProviderHealth {
            available: false,
            version: None,
            message: Some("Provider health unavailable".to_string()),
        });
        let setup_state = determine_setup_state(&capabilities, &health_state, credential_source);

        let seed = seeds.get(capabilities.provider_id.as_str());
        let registry_record = registry_payload.as_ref().and_then(|payload| {
            seed.and_then(|seed| {
                extract_registry_record(&payload.payload, seed.registry_candidates)
            })
        });
        let execution_supported = execution_supported(&capabilities);
        let release_gate_status = release_gate_status(&capabilities, &setup_state);
        let release_gate_passed = release_gate_passed(&capabilities, &setup_state);
        let currently_runnable = currently_runnable(&capabilities, &setup_state);

        let registry = seed.map(|seed| ProviderRegistryMetadata {
            registry_id: registry_record
                .as_ref()
                .map(|record| record.registry_id.clone()),
            latest_version: registry_record
                .as_ref()
                .and_then(|record| record.latest_version.clone()),
            install_source: capabilities.install_source.clone(),
            distribution_channel: if registry_record.is_some() {
                "acp_registry".to_string()
            } else {
                capabilities.install_source.clone()
            },
            install_command: registry_record
                .as_ref()
                .and_then(|record| record.install_command.clone())
                .or_else(|| seed.install_command.map(String::from)),
            install_url: registry_record
                .as_ref()
                .and_then(|record| record.install_url.clone())
                .or_else(|| seed.install_url.map(String::from)),
            docs_url: registry_record
                .as_ref()
                .and_then(|record| record.docs_url.clone())
                .or_else(|| seed.docs_url.map(String::from)),
            auth_docs_url: seed.auth_docs_url.map(String::from),
            config_options: if capabilities.supported_config_options.is_empty() {
                seed.config_options
                    .iter()
                    .map(|(id, name, values)| config_option_seed(id, name, values))
                    .collect()
            } else {
                capabilities.supported_config_options.clone()
            },
            supported_modes: if capabilities.supported_session_modes.is_empty() {
                seed.supported_modes
                    .iter()
                    .map(|value| (*value).to_string())
                    .collect()
            } else {
                capabilities.supported_session_modes.clone()
            },
            session_capabilities: capabilities.session_capabilities.clone(),
            setup_notes: seed
                .setup_notes
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
        });

        let mut actions = vec![ProviderSetupAction {
            id: "verify".to_string(),
            label: "Verify setup".to_string(),
            kind: SetupActionKind::Verify,
            enabled: execution_supported,
            command_preview: None,
            url: None,
        }];
        if let Some(registry) = registry.as_ref() {
            if let Some(url) = registry.install_url.as_ref() {
                actions.push(ProviderSetupAction {
                    id: "install".to_string(),
                    label: "Install docs".to_string(),
                    kind: SetupActionKind::OpenInstall,
                    enabled: true,
                    command_preview: registry.install_command.clone(),
                    url: Some(url.clone()),
                });
            }
            if let Some(url) = registry.docs_url.as_ref() {
                actions.push(ProviderSetupAction {
                    id: "docs".to_string(),
                    label: "Provider docs".to_string(),
                    kind: SetupActionKind::OpenDocs,
                    enabled: true,
                    command_preview: None,
                    url: Some(url.clone()),
                });
            }
            if let Some(url) = registry.auth_docs_url.as_ref() {
                actions.push(ProviderSetupAction {
                    id: "auth".to_string(),
                    label: "Auth docs".to_string(),
                    kind: SetupActionKind::OpenAuthDocs,
                    enabled: true,
                    command_preview: None,
                    url: Some(url.clone()),
                });
            }
        }

        let readiness_reason = health_state
            .message
            .clone()
            .unwrap_or_else(|| match setup_state {
                ProviderSetupState::Ready => "Provider is ready for review runs.".to_string(),
                ProviderSetupState::NeedsInstall => {
                    "Provider CLI or transport is not ready on this machine.".to_string()
                }
                ProviderSetupState::NeedsAuth => {
                    "Provider still needs credentials or authentication.".to_string()
                }
                ProviderSetupState::DiscoverableOnly => {
                    "Provider is visible in the catalog but is not available for review runs yet."
                        .to_string()
                }
                ProviderSetupState::Unsupported => {
                    "Provider is not part of the supported execution surface.".to_string()
                }
            });

        let mut warnings = Vec::new();
        if execution_supported && capabilities.conformance_status != "covered" {
            warnings.push("Conformance coverage is still in progress.".to_string());
        }
        if execution_supported && capabilities.eval_status != "covered" {
            warnings.push("Evaluation coverage is still in progress.".to_string());
        }

        providers.push(ProviderSetupCatalogEntry {
            provider_id: capabilities.provider_id.clone(),
            display_name: capabilities.display_name.clone(),
            provider_family: capabilities.provider_family.clone(),
            setup_state,
            readiness_reason,
            support_tier: support_tier(&capabilities).to_string(),
            execution_supported,
            release_gate_status,
            release_gate_passed,
            currently_runnable,
            credential_source,
            capabilities,
            registry,
            actions,
            warnings,
        });
    }

    Ok(ProviderSetupCatalogSnapshot {
        providers,
        registry_fetched_at,
        registry_source,
        generated_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub async fn probe_provider_setup(
    app: &AppHandle,
    provider_id: &str,
) -> Result<ProviderSetupProbeResult, AppError> {
    let capabilities = provider_registry()
        .into_iter()
        .find(|entry| entry.provider_id == provider_id)
        .ok_or_else(|| AppError::NotFound(format!("Unknown provider '{}'", provider_id)))?;
    let statuses = credentials::all_credential_statuses()?;
    let health = provider_health_by_provider(app).await;
    let credential_source = statuses
        .iter()
        .filter(|status| {
            status
                .provider_ids
                .iter()
                .any(|entry_provider_id| entry_provider_id == provider_id)
        })
        .map(|status| status.source)
        .max_by_key(|source| match source {
            CredentialSource::Environment => 3,
            CredentialSource::Keychain => 2,
            CredentialSource::None => 1,
        });
    let health_state = health.get(provider_id).cloned().unwrap_or(ProviderHealth {
        available: false,
        version: None,
        message: Some("Provider health unavailable".to_string()),
    });
    let setup_state = determine_setup_state(&capabilities, &health_state, credential_source);
    let reason = health_state.message.unwrap_or_else(|| match setup_state {
        ProviderSetupState::Ready => "Provider is ready for review runs.".to_string(),
        ProviderSetupState::NeedsInstall => {
            "Provider CLI or transport is not ready on this machine.".to_string()
        }
        ProviderSetupState::NeedsAuth => {
            "Provider still needs credentials or authentication.".to_string()
        }
        ProviderSetupState::DiscoverableOnly => {
            "Provider is visible in the catalog but is not available for review runs yet."
                .to_string()
        }
        ProviderSetupState::Unsupported => {
            "Provider is not part of the supported execution surface.".to_string()
        }
    });

    Ok(ProviderSetupProbeResult {
        provider_id: provider_id.to_string(),
        ready: currently_runnable(&capabilities, &setup_state),
        setup_state,
        reason,
        checked_at: chrono::Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::capabilities::provider_registry;
    use serde_json::json;

    #[test]
    fn determine_setup_state_distinguishes_auth_from_install() {
        let caps = provider_registry()
            .into_iter()
            .find(|provider| provider.provider_id == "cursor")
            .unwrap();
        let missing_auth = ProviderHealth {
            available: false,
            version: None,
            message: Some("CURSOR_API_KEY not set".into()),
        };
        assert!(matches!(
            determine_setup_state(&caps, &missing_auth, Some(CredentialSource::None),),
            ProviderSetupState::NeedsAuth
        ));
        let spawn_failure = ProviderHealth {
            available: false,
            version: None,
            message: Some("failed to spawn".into()),
        };
        assert!(matches!(
            determine_setup_state(&caps, &spawn_failure, Some(CredentialSource::Environment),),
            ProviderSetupState::NeedsInstall
        ));
    }

    #[test]
    fn cache_freshness_marks_stale_payloads() {
        let payload = CachedRegistryPayload {
            fetched_at: (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339(),
            payload: json!({}),
        };
        assert_eq!(cache_freshness(&payload), RegistryCacheFreshness::Stale);
    }

    #[test]
    fn extract_registry_record_matches_exact_ids_in_agents_array() {
        let payload = json!({
            "agents": [
                {
                    "id": "cursor",
                    "version": "2026.05.09",
                    "installCommand": "cursor install",
                    "docsUrl": "https://cursor.com/docs/cli/acp"
                }
            ]
        });
        let record = extract_registry_record(&payload, &["cursor"]).expect("record");
        assert_eq!(record.registry_id, "cursor");
        assert_eq!(record.latest_version.as_deref(), Some("2026.05.09"));
    }

    #[test]
    fn extract_registry_record_rejects_nested_fuzzy_matches() {
        let payload = json!({
            "agents": [
                {
                    "id": "not-cursor",
                    "name": "Not Cursor",
                    "metadata": {
                        "slug": "cursor"
                    }
                }
            ]
        });
        assert!(extract_registry_record(&payload, &["cursor"]).is_none());
    }
}
