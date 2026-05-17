use serde::{Deserialize, Serialize};

use crate::providers::capabilities::{ProviderCapabilities, ProviderSelectionEligibility};
use crate::providers::traits::ProviderHealth;
use crate::secrets::credentials::CredentialSource;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSetupState {
    Ready,
    NeedsInstall,
    NeedsAuth,
    DiscoverableOnly,
    Unsupported,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderReleaseGateStatus {
    Passed,
    BlockedConformance,
    BlockedEval,
    BlockedSetup,
}

pub fn execution_supported(capabilities: &ProviderCapabilities) -> bool {
    capabilities.execution_support_tier == "supported"
}

pub fn selection_eligible_for_auto(capabilities: &ProviderCapabilities) -> bool {
    execution_supported(capabilities)
        && matches!(
            capabilities.selection_eligibility,
            ProviderSelectionEligibility::AutoAllowed
        )
}

pub fn selection_eligible_for_manual(capabilities: &ProviderCapabilities) -> bool {
    execution_supported(capabilities)
        && matches!(
            capabilities.selection_eligibility,
            ProviderSelectionEligibility::AutoAllowed | ProviderSelectionEligibility::ManualOnly
        )
}

pub fn release_gate_status(
    capabilities: &ProviderCapabilities,
    setup_state: &ProviderSetupState,
) -> ProviderReleaseGateStatus {
    if !execution_supported(capabilities) || !matches!(setup_state, ProviderSetupState::Ready) {
        return ProviderReleaseGateStatus::BlockedSetup;
    }

    if capabilities.conformance_status != "covered" {
        return ProviderReleaseGateStatus::BlockedConformance;
    }

    if capabilities.eval_status != "covered" {
        return ProviderReleaseGateStatus::BlockedEval;
    }

    ProviderReleaseGateStatus::Passed
}

pub fn release_gate_passed(
    capabilities: &ProviderCapabilities,
    setup_state: &ProviderSetupState,
) -> bool {
    matches!(
        release_gate_status(capabilities, setup_state),
        ProviderReleaseGateStatus::Passed
    )
}

pub fn determine_setup_state(
    capabilities: &ProviderCapabilities,
    health: &ProviderHealth,
    credential_source: Option<CredentialSource>,
) -> ProviderSetupState {
    if capabilities.execution_support_tier == "unsupported" {
        return ProviderSetupState::Unsupported;
    }

    if capabilities.execution_support_tier == "discoverable_only" {
        return ProviderSetupState::DiscoverableOnly;
    }

    if health.available {
        return ProviderSetupState::Ready;
    }

    if !capabilities.credential_fields.is_empty()
        && matches!(credential_source, None | Some(CredentialSource::None))
    {
        return ProviderSetupState::NeedsAuth;
    }

    if let Some(message) = health.message.as_deref() {
        let message = message.to_lowercase();
        if message.contains("not set")
            || message.contains("credentials")
            || message.contains("api key")
            || message.contains("auth")
        {
            return ProviderSetupState::NeedsAuth;
        }
    }

    ProviderSetupState::NeedsInstall
}

pub fn currently_runnable(
    capabilities: &ProviderCapabilities,
    setup_state: &ProviderSetupState,
) -> bool {
    execution_supported(capabilities) && matches!(setup_state, ProviderSetupState::Ready)
}

pub fn support_tier(capabilities: &ProviderCapabilities) -> &'static str {
    match capabilities.execution_support_tier.as_str() {
        "supported" => "supported",
        "discoverable_only" => "discoverable",
        _ => "unsupported",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::acp::shared::AcpSessionCapabilities;
    use crate::providers::capabilities::{
        CredentialFieldDescriptor, ProviderSelectionEligibility, ToolGovernanceTier,
    };

    fn sample_caps() -> ProviderCapabilities {
        ProviderCapabilities {
            provider_id: "sample".into(),
            display_name: "Sample".into(),
            provider_family: "cli_bridge".into(),
            transport_family: "acp_stdio_ndjson".into(),
            fit_tags: vec![],
            billing_risk: "included".into(),
            setup_complexity: "moderate".into(),
            install_source: "manual_cli".into(),
            auth_mode: "api_key".into(),
            permission_model: "deny_by_default".into(),
            opt_in_only: false,
            in_auto_fallback: false,
            selection_eligibility: ProviderSelectionEligibility::AutoAllowed,
            execution_support_tier: "supported".into(),
            conformance_status: "covered".into(),
            eval_status: "planned".into(),
            credential_fields: vec![CredentialFieldDescriptor {
                provider_id: "sample".into(),
                field: "api_key".into(),
                env_var: "SAMPLE_API_KEY".into(),
            }],
            interactive_permissions: false,
            default_governance_tier: ToolGovernanceTier::ReadOnly,
            supports_session_resume: false,
            supports_checkpointing: false,
            paid_eval_eligible: false,
            supported_session_modes: vec![],
            supported_config_options: vec![],
            session_capabilities: AcpSessionCapabilities::default(),
        }
    }

    #[test]
    fn determine_setup_state_prefers_auth_signals() {
        let caps = sample_caps();
        let health = ProviderHealth {
            available: false,
            version: None,
            message: Some("SAMPLE_API_KEY not set".into()),
        };
        assert_eq!(
            determine_setup_state(&caps, &health, Some(CredentialSource::None)),
            ProviderSetupState::NeedsAuth
        );
    }

    #[test]
    fn release_gate_distinguishes_setup_conformance_and_eval() {
        let caps = sample_caps();
        assert!(execution_supported(&caps));
        assert_eq!(
            release_gate_status(&caps, &ProviderSetupState::Ready),
            ProviderReleaseGateStatus::BlockedEval
        );
        assert!(!release_gate_passed(&caps, &ProviderSetupState::Ready));

        let mut conformance_blocked = caps.clone();
        conformance_blocked.eval_status = "covered".into();
        conformance_blocked.conformance_status = "planned".into();
        assert_eq!(
            release_gate_status(&conformance_blocked, &ProviderSetupState::Ready),
            ProviderReleaseGateStatus::BlockedConformance
        );

        let mut setup_blocked = caps.clone();
        setup_blocked.eval_status = "covered".into();
        assert_eq!(
            release_gate_status(&setup_blocked, &ProviderSetupState::NeedsAuth),
            ProviderReleaseGateStatus::BlockedSetup
        );

        let mut passed = caps.clone();
        passed.eval_status = "covered".into();
        assert_eq!(
            release_gate_status(&passed, &ProviderSetupState::Ready),
            ProviderReleaseGateStatus::Passed
        );
        assert!(release_gate_passed(&passed, &ProviderSetupState::Ready));
    }

    #[test]
    fn selection_eligibility_distinguishes_auto_manual_and_catalog_only() {
        let auto_allowed = sample_caps();
        assert!(selection_eligible_for_auto(&auto_allowed));
        assert!(selection_eligible_for_manual(&auto_allowed));

        let mut manual_only = auto_allowed.clone();
        manual_only.selection_eligibility = ProviderSelectionEligibility::ManualOnly;
        assert!(!selection_eligible_for_auto(&manual_only));
        assert!(selection_eligible_for_manual(&manual_only));

        let mut catalog_only = auto_allowed.clone();
        catalog_only.selection_eligibility = ProviderSelectionEligibility::CatalogOnly;
        catalog_only.execution_support_tier = "discoverable_only".into();
        assert!(!selection_eligible_for_auto(&catalog_only));
        assert!(!selection_eligible_for_manual(&catalog_only));
    }
}
