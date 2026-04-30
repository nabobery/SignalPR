use serde::{Deserialize, Serialize};

use crate::errors::AppError;
use crate::providers::capabilities::{get_provider_caps, ToolGovernanceTier};

/// The effective governance tier for a provider, considering user overrides.
/// User can upgrade from ReadOnly to GuardedWrite (opt-in), but never beyond
/// what the provider supports (interactive_permissions required for GuardedWrite+).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveGovernance {
    pub provider_id: String,
    pub configured_tier: ToolGovernanceTier,
    pub effective_tier: ToolGovernanceTier,
    pub interactive_permissions: bool,
}

/// Resolve the effective governance tier for a provider given a user-configured setting.
/// Returns the minimum of (configured tier, what the provider actually supports).
pub fn resolve_effective_tier(
    provider_id: &str,
    configured_tier: Option<ToolGovernanceTier>,
) -> Result<EffectiveGovernance, AppError> {
    let caps = get_provider_caps(provider_id)
        .ok_or_else(|| AppError::InvalidInput(format!("Unknown provider: {provider_id}")))?;

    let configured = configured_tier.unwrap_or(caps.default_governance_tier);

    let effective = if !caps.interactive_permissions {
        // Non-interactive providers can only be read_only
        ToolGovernanceTier::ReadOnly
    } else {
        configured
    };

    Ok(EffectiveGovernance {
        provider_id: provider_id.to_string(),
        configured_tier: configured,
        effective_tier: effective,
        interactive_permissions: caps.interactive_permissions,
    })
}

/// Check if an approval action is allowed given the current governance tier.
/// Returns Ok(()) if approved, Err if the tier blocks approval.
pub fn check_approval_allowed(
    provider_id: &str,
    configured_tier: Option<ToolGovernanceTier>,
) -> Result<(), AppError> {
    let gov = resolve_effective_tier(provider_id, configured_tier)?;
    match gov.effective_tier {
        ToolGovernanceTier::ReadOnly => Err(AppError::InvalidInput(format!(
            "Provider '{}' is in read_only governance tier; approval blocked",
            provider_id
        ))),
        ToolGovernanceTier::GuardedWrite | ToolGovernanceTier::TrustedWrite => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_provider_cannot_approve() {
        let result = check_approval_allowed("claude_code", None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("read_only governance tier"));
    }

    #[test]
    fn test_claude_code_guarded_write_override_allows_approval() {
        let result = check_approval_allowed("claude_code", Some(ToolGovernanceTier::GuardedWrite));
        assert!(result.is_ok());
    }

    #[test]
    fn test_interactive_provider_default_allows_approval() {
        let result = check_approval_allowed("copilot", None);
        assert!(result.is_ok());
    }

    #[test]
    fn test_non_interactive_provider_forced_read_only() {
        let gov = resolve_effective_tier("gemini", Some(ToolGovernanceTier::GuardedWrite)).unwrap();
        assert_eq!(gov.effective_tier, ToolGovernanceTier::ReadOnly);
    }

    #[test]
    fn test_unknown_provider_returns_error() {
        let result = resolve_effective_tier("nonexistent", None);
        assert!(result.is_err());
    }
}
