use serde::{Deserialize, Serialize};

use crate::providers::acp::shared::{AcpConfigOptionDescriptor, AcpSessionCapabilities};

/// Tool governance tier for a provider. Determines what kind of actions
/// a provider is allowed to perform on behalf of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolGovernanceTier {
    ReadOnly,
    GuardedWrite,
    TrustedWrite,
}

impl ToolGovernanceTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::GuardedWrite => "guarded_write",
            Self::TrustedWrite => "trusted_write",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "read_only" => Some(Self::ReadOnly),
            "guarded_write" => Some(Self::GuardedWrite),
            "trusted_write" => Some(Self::TrustedWrite),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSelectionEligibility {
    AutoAllowed,
    ManualOnly,
    CatalogOnly,
    Unsupported,
}

/// Describes a credential field required by a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialFieldDescriptor {
    pub provider_id: String,
    pub field: String,
    pub env_var: String,
}

/// Static capabilities descriptor for a provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub provider_id: String,
    pub display_name: String,
    pub provider_family: String,
    pub transport_family: String,
    pub fit_tags: Vec<String>,
    pub billing_risk: String,
    pub setup_complexity: String,
    pub install_source: String,
    pub auth_mode: String,
    pub permission_model: String,
    pub opt_in_only: bool,
    pub in_auto_fallback: bool,
    pub selection_eligibility: ProviderSelectionEligibility,
    pub execution_support_tier: String,
    pub conformance_status: String,
    pub eval_status: String,
    pub credential_fields: Vec<CredentialFieldDescriptor>,
    pub interactive_permissions: bool,
    pub default_governance_tier: ToolGovernanceTier,
    pub supports_session_resume: bool,
    pub supports_checkpointing: bool,
    pub paid_eval_eligible: bool,
    pub supported_session_modes: Vec<String>,
    pub supported_config_options: Vec<AcpConfigOptionDescriptor>,
    pub session_capabilities: AcpSessionCapabilities,
}

/// Returns the static capability registry for all known providers.
pub fn provider_registry() -> Vec<ProviderCapabilities> {
    vec![
        ProviderCapabilities {
            provider_id: "codex".into(),
            display_name: "Codex CLI".into(),
            provider_family: "local_cli".into(),
            transport_family: "shell_execute".into(),
            fit_tags: vec!["balanced".into(), "fast_scan".into()],
            billing_risk: "included".into(),
            setup_complexity: "moderate".into(),
            install_source: "manual_cli".into(),
            auth_mode: "local_session".into(),
            permission_model: "host_governed".into(),
            opt_in_only: false,
            in_auto_fallback: true,
            selection_eligibility: ProviderSelectionEligibility::AutoAllowed,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "planned".into(),
            credential_fields: vec![],
            interactive_permissions: false,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "codex_app_server".into(),
            display_name: "Codex App Server".into(),
            provider_family: "managed_local_agent".into(),
            transport_family: "jsonrpc_stdio".into(),
            fit_tags: vec!["balanced".into(), "interactive".into()],
            billing_risk: "included".into(),
            setup_complexity: "moderate".into(),
            install_source: "bundled_app".into(),
            auth_mode: "desktop_session".into(),
            permission_model: "interactive_approval".into(),
            opt_in_only: false,
            in_auto_fallback: true,
            selection_eligibility: ProviderSelectionEligibility::AutoAllowed,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "planned".into(),
            credential_fields: vec![],
            interactive_permissions: true,
            default_governance_tier: ToolGovernanceTier::GuardedWrite,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "claude".into(),
            display_name: "Claude (Direct API)".into(),
            provider_family: "direct_api".into(),
            transport_family: "https_api".into(),
            fit_tags: vec!["deep_reasoning".into(), "safest_read_only".into()],
            billing_risk: "paid_api".into(),
            setup_complexity: "simple".into(),
            install_source: "direct_api".into(),
            auth_mode: "api_key".into(),
            permission_model: "host_governed".into(),
            opt_in_only: false,
            in_auto_fallback: true,
            selection_eligibility: ProviderSelectionEligibility::AutoAllowed,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "covered".into(),
            credential_fields: vec![CredentialFieldDescriptor {
                provider_id: "claude".into(),
                field: "api_key".into(),
                env_var: "ANTHROPIC_API_KEY".into(),
            }],
            interactive_permissions: false,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: true,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "copilot".into(),
            display_name: "GitHub Copilot".into(),
            provider_family: "connected_agent".into(),
            transport_family: "jsonrpc_content_length".into(),
            fit_tags: vec!["interactive".into(), "balanced".into()],
            billing_risk: "subscription".into(),
            setup_complexity: "moderate".into(),
            install_source: "editor_agent".into(),
            auth_mode: "account_login".into(),
            permission_model: "interactive_approval".into(),
            opt_in_only: false,
            in_auto_fallback: true,
            selection_eligibility: ProviderSelectionEligibility::CatalogOnly,
            execution_support_tier: "discoverable_only".into(),
            conformance_status: "planned".into(),
            eval_status: "planned".into(),
            credential_fields: vec![],
            interactive_permissions: true,
            default_governance_tier: ToolGovernanceTier::GuardedWrite,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "opencode".into(),
            display_name: "OpenCode".into(),
            provider_family: "connected_agent".into(),
            transport_family: "http_sse".into(),
            fit_tags: vec!["interactive".into(), "experimental".into()],
            billing_risk: "self_hosted".into(),
            setup_complexity: "advanced".into(),
            install_source: "self_hosted".into(),
            auth_mode: "password".into(),
            permission_model: "interactive_approval".into(),
            opt_in_only: false,
            in_auto_fallback: true,
            selection_eligibility: ProviderSelectionEligibility::CatalogOnly,
            execution_support_tier: "discoverable_only".into(),
            conformance_status: "planned".into(),
            eval_status: "planned".into(),
            credential_fields: vec![CredentialFieldDescriptor {
                provider_id: "opencode".into(),
                field: "server_password".into(),
                env_var: "OPENCODE_SERVER_PASSWORD".into(),
            }],
            interactive_permissions: true,
            default_governance_tier: ToolGovernanceTier::GuardedWrite,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "gemini".into(),
            display_name: "Gemini CLI".into(),
            provider_family: "cli_bridge".into(),
            transport_family: "acp_stdio_ndjson".into(),
            fit_tags: vec!["fast_scan".into(), "experimental".into()],
            billing_risk: "paid_api".into(),
            setup_complexity: "moderate".into(),
            install_source: "acp_registry".into(),
            auth_mode: "api_key".into(),
            permission_model: "deny_by_default".into(),
            opt_in_only: true,
            in_auto_fallback: false,
            selection_eligibility: ProviderSelectionEligibility::ManualOnly,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "planned".into(),
            credential_fields: vec![
                CredentialFieldDescriptor {
                    provider_id: "gemini".into(),
                    field: "api_key".into(),
                    env_var: "GEMINI_API_KEY".into(),
                },
                CredentialFieldDescriptor {
                    provider_id: "gemini".into(),
                    field: "google_api_key".into(),
                    env_var: "GOOGLE_API_KEY".into(),
                },
            ],
            interactive_permissions: false,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec!["plan".into()],
            supported_config_options: vec![
                AcpConfigOptionDescriptor {
                    id: "mode".into(),
                    name: "Session Mode".into(),
                    option_type: "select".into(),
                    current_value: None,
                    options: vec![crate::providers::acp::shared::AcpConfigOptionValue {
                        value: "plan".into(),
                        label: "plan".into(),
                        description: None,
                    }],
                },
                AcpConfigOptionDescriptor {
                    id: "model".into(),
                    name: "Model".into(),
                    option_type: "select".into(),
                    current_value: None,
                    options: vec![
                        crate::providers::acp::shared::AcpConfigOptionValue {
                            value: "gemini-2.5-pro".into(),
                            label: "gemini-2.5-pro".into(),
                            description: None,
                        },
                        crate::providers::acp::shared::AcpConfigOptionValue {
                            value: "gemini-2.5-flash".into(),
                            label: "gemini-2.5-flash".into(),
                            description: None,
                        },
                        crate::providers::acp::shared::AcpConfigOptionValue {
                            value: "auto".into(),
                            label: "auto".into(),
                            description: None,
                        },
                    ],
                },
            ],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "cursor".into(),
            display_name: "Cursor CLI".into(),
            provider_family: "cli_bridge".into(),
            transport_family: "acp_stdio_ndjson".into(),
            fit_tags: vec!["interactive".into(), "experimental".into()],
            billing_risk: "subscription".into(),
            setup_complexity: "advanced".into(),
            install_source: "acp_registry".into(),
            auth_mode: "api_key".into(),
            permission_model: "deny_by_default".into(),
            opt_in_only: true,
            in_auto_fallback: false,
            selection_eligibility: ProviderSelectionEligibility::ManualOnly,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "planned".into(),
            credential_fields: vec![CredentialFieldDescriptor {
                provider_id: "cursor".into(),
                field: "api_key".into(),
                env_var: "CURSOR_API_KEY".into(),
            }],
            interactive_permissions: false,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec!["ask".into()],
            supported_config_options: vec![
                AcpConfigOptionDescriptor {
                    id: "mode".into(),
                    name: "Session Mode".into(),
                    option_type: "select".into(),
                    current_value: None,
                    options: vec![crate::providers::acp::shared::AcpConfigOptionValue {
                        value: "ask".into(),
                        label: "ask".into(),
                        description: None,
                    }],
                },
                AcpConfigOptionDescriptor {
                    id: "model".into(),
                    name: "Model".into(),
                    option_type: "select".into(),
                    current_value: None,
                    options: vec![
                        crate::providers::acp::shared::AcpConfigOptionValue {
                            value: "auto".into(),
                            label: "auto".into(),
                            description: None,
                        },
                        crate::providers::acp::shared::AcpConfigOptionValue {
                            value: "gpt-5.2".into(),
                            label: "gpt-5.2".into(),
                            description: None,
                        },
                    ],
                },
            ],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "pi".into(),
            display_name: "PI Agent".into(),
            provider_family: "cli_bridge".into(),
            transport_family: "jsonl_rpc".into(),
            fit_tags: vec!["experimental".into(), "safest_read_only".into()],
            billing_risk: "custom".into(),
            setup_complexity: "advanced".into(),
            install_source: "manual_cli".into(),
            auth_mode: "provider_native".into(),
            permission_model: "tools_disabled".into(),
            opt_in_only: true,
            in_auto_fallback: false,
            selection_eligibility: ProviderSelectionEligibility::CatalogOnly,
            execution_support_tier: "discoverable_only".into(),
            conformance_status: "planned".into(),
            eval_status: "planned".into(),
            credential_fields: vec![],
            interactive_permissions: false,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
        ProviderCapabilities {
            provider_id: "claude_code".into(),
            display_name: "Claude Code".into(),
            provider_family: "managed_cli".into(),
            transport_family: "sidecar_jsonrpc".into(),
            fit_tags: vec![
                "deep_reasoning".into(),
                "resume".into(),
                "experimental".into(),
            ],
            billing_risk: "paid_api".into(),
            setup_complexity: "advanced".into(),
            install_source: "bundled_sidecar".into(),
            auth_mode: "api_key".into(),
            permission_model: "interactive_approval".into(),
            opt_in_only: true,
            in_auto_fallback: false,
            selection_eligibility: ProviderSelectionEligibility::ManualOnly,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "planned".into(),
            credential_fields: vec![CredentialFieldDescriptor {
                provider_id: "claude_code".into(),
                field: "api_key".into(),
                env_var: "ANTHROPIC_API_KEY".into(),
            }],
            interactive_permissions: true,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: true,
            supports_checkpointing: true,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        },
    ]
}

/// Look up a single provider's capabilities by ID.
pub fn get_provider_caps(provider_id: &str) -> Option<ProviderCapabilities> {
    provider_registry()
        .into_iter()
        .find(|p| p.provider_id == canonical_provider_id(provider_id))
}

pub fn canonical_provider_id(provider_id: &str) -> &str {
    match provider_id {
        "codex_app_server" | "codex-app-server" => "codex_app_server",
        "codex_exec" | "codex" => "codex",
        "copilot_sdk" | "copilot" => "copilot",
        "opencode_sdk" | "opencode" => "opencode",
        "gemini_cli" | "gemini" => "gemini",
        "cursor_cli" | "cursor" => "cursor",
        "pi_cli" | "pi" => "pi",
        "claude_code" => "claude_code",
        "claude" => "claude",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_contains_all_known_providers() {
        let registry = provider_registry();
        let ids: Vec<&str> = registry.iter().map(|p| p.provider_id.as_str()).collect();
        for expected in [
            "codex",
            "codex_app_server",
            "claude",
            "copilot",
            "opencode",
            "gemini",
            "cursor",
            "pi",
            "claude_code",
        ] {
            assert!(ids.contains(&expected), "Missing provider: {expected}");
        }
    }

    #[test]
    fn test_claude_code_is_opt_in_and_not_in_auto() {
        let caps = get_provider_caps("claude_code").unwrap();
        assert!(caps.opt_in_only);
        assert!(!caps.in_auto_fallback);
        assert_eq!(
            caps.selection_eligibility,
            ProviderSelectionEligibility::ManualOnly
        );
    }

    #[test]
    fn test_claude_code_default_tier_is_read_only() {
        let caps = get_provider_caps("claude_code").unwrap();
        assert_eq!(caps.default_governance_tier, ToolGovernanceTier::ReadOnly);
    }

    #[test]
    fn test_gemini_is_opt_in() {
        let caps = get_provider_caps("gemini").unwrap();
        assert!(caps.opt_in_only);
        assert!(!caps.in_auto_fallback);
        assert_eq!(
            caps.selection_eligibility,
            ProviderSelectionEligibility::ManualOnly
        );
    }

    #[test]
    fn test_copilot_is_interactive() {
        let caps = get_provider_caps("copilot").unwrap();
        assert!(caps.interactive_permissions);
        assert_eq!(
            caps.selection_eligibility,
            ProviderSelectionEligibility::CatalogOnly
        );
        assert_eq!(
            caps.default_governance_tier,
            ToolGovernanceTier::GuardedWrite
        );
    }

    #[test]
    fn test_governance_tier_serialization() {
        assert_eq!(ToolGovernanceTier::ReadOnly.as_str(), "read_only");
        assert_eq!(
            ToolGovernanceTier::from_str("guarded_write"),
            Some(ToolGovernanceTier::GuardedWrite)
        );
        assert_eq!(
            ToolGovernanceTier::from_str("trusted_write"),
            Some(ToolGovernanceTier::TrustedWrite)
        );
        assert_eq!(ToolGovernanceTier::from_str("invalid"), None);
    }
}
