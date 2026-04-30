use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[cfg(not(test))]
const SERVICE_NAME: &str = "signalpr-provider";

/// Known provider credential fields. Each variant maps to a specific
/// keychain account and environment variable fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialField {
    AnthropicApiKey,
    GeminiApiKey,
    GoogleApiKey,
    CursorApiKey,
    OpenCodeServerPassword,
}

impl ProviderCredentialField {
    pub fn all() -> &'static [Self] {
        &[
            Self::AnthropicApiKey,
            Self::GeminiApiKey,
            Self::GoogleApiKey,
            Self::CursorApiKey,
            Self::OpenCodeServerPassword,
        ]
    }

    pub fn keychain_account(&self) -> &'static str {
        match self {
            Self::AnthropicApiKey => "anthropic_api_key",
            Self::GeminiApiKey => "gemini_api_key",
            Self::GoogleApiKey => "google_api_key",
            Self::CursorApiKey => "cursor_api_key",
            Self::OpenCodeServerPassword => "opencode_server_password",
        }
    }

    pub fn env_var_name(&self) -> &'static str {
        match self {
            Self::AnthropicApiKey => "ANTHROPIC_API_KEY",
            Self::GeminiApiKey => "GEMINI_API_KEY",
            Self::GoogleApiKey => "GOOGLE_API_KEY",
            Self::CursorApiKey => "CURSOR_API_KEY",
            Self::OpenCodeServerPassword => "OPENCODE_SERVER_PASSWORD",
        }
    }

    /// Provider IDs that use this credential field.
    pub fn provider_ids(&self) -> &'static [&'static str] {
        match self {
            Self::AnthropicApiKey => &["claude", "claude_code"],
            Self::GeminiApiKey => &["gemini"],
            Self::GoogleApiKey => &["gemini"],
            Self::CursorApiKey => &["cursor"],
            Self::OpenCodeServerPassword => &["opencode"],
        }
    }

    pub fn from_provider_and_field(provider_id: &str, field: &str) -> Option<Self> {
        match (provider_id, field) {
            ("claude" | "claude_code", "api_key") => Some(Self::AnthropicApiKey),
            ("gemini", "api_key") => Some(Self::GeminiApiKey),
            ("gemini", "google_api_key") => Some(Self::GoogleApiKey),
            ("cursor", "api_key") => Some(Self::CursorApiKey),
            ("opencode", "server_password") => Some(Self::OpenCodeServerPassword),
            _ => None,
        }
    }
}

/// Where a credential value was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    Environment,
    Keychain,
    None,
}

/// Status of a single credential field (never exposes the actual value).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialStatus {
    pub field: ProviderCredentialField,
    pub source: CredentialSource,
    pub provider_ids: Vec<String>,
}

// --- Keychain abstraction (test-friendly) ---

#[cfg(test)]
mod test_store {
    use std::collections::HashMap;
    use std::sync::Mutex;

    static STORE: std::sync::LazyLock<Mutex<HashMap<String, String>>> =
        std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

    pub fn set(key: &str, value: &str) {
        STORE
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
    }

    pub fn get(key: &str) -> Option<String> {
        STORE.lock().unwrap().get(key).cloned()
    }

    pub fn delete(key: &str) {
        STORE.lock().unwrap().remove(key);
    }

    pub fn clear() {
        STORE.lock().unwrap().clear();
    }
}

#[cfg(not(test))]
fn keyring_entry(field: &ProviderCredentialField) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE_NAME, field.keychain_account())
        .map_err(|e| AppError::Channel(format!("Keychain entry error: {e}")))
}

/// Store a provider secret in the OS keychain. Never writes to SQLite or logs.
pub fn store_secret(field: ProviderCredentialField, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Secret value must be non-empty".into(),
        ));
    }

    #[cfg(test)]
    {
        test_store::set(field.keychain_account(), value);
        Ok(())
    }

    #[cfg(not(test))]
    {
        let entry = keyring_entry(&field)?;
        entry
            .set_password(value)
            .map_err(|e| AppError::Channel(format!("Failed to store secret: {e}")))?;
        Ok(())
    }
}

/// Retrieve a secret from keychain (internal use only — never expose to frontend).
pub fn get_secret(field: ProviderCredentialField) -> Result<Option<String>, AppError> {
    #[cfg(test)]
    {
        Ok(test_store::get(field.keychain_account()))
    }

    #[cfg(not(test))]
    {
        let entry = keyring_entry(&field)?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Channel(format!("Failed to read secret: {e}"))),
        }
    }
}

/// Delete a provider secret from keychain.
pub fn delete_secret(field: ProviderCredentialField) -> Result<(), AppError> {
    #[cfg(test)]
    {
        test_store::delete(field.keychain_account());
        Ok(())
    }

    #[cfg(not(test))]
    {
        let entry = keyring_entry(&field)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Channel(format!("Failed to delete secret: {e}"))),
        }
    }
}

/// Resolve the effective value for a credential field.
/// Precedence: environment variable > keychain.
/// Returns (value, source) — value is None when neither source has it.
pub fn resolve_credential(
    field: ProviderCredentialField,
) -> Result<(Option<String>, CredentialSource), AppError> {
    if let Ok(val) = std::env::var(field.env_var_name()) {
        if !val.is_empty() {
            return Ok((Some(val), CredentialSource::Environment));
        }
    }

    match get_secret(field)? {
        Some(val) => Ok((Some(val), CredentialSource::Keychain)),
        None => Ok((None, CredentialSource::None)),
    }
}

/// Get the status of a credential field without exposing its value.
pub fn credential_status(field: ProviderCredentialField) -> Result<CredentialStatus, AppError> {
    let (_, source) = resolve_credential(field)?;
    Ok(CredentialStatus {
        field,
        source,
        provider_ids: field.provider_ids().iter().map(|s| s.to_string()).collect(),
    })
}

/// Get statuses for all known credential fields.
pub fn all_credential_statuses() -> Result<Vec<CredentialStatus>, AppError> {
    ProviderCredentialField::all()
        .iter()
        .map(|f| credential_status(*f))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        test_store::clear();
    }

    #[test]
    fn test_store_get_delete_round_trip() {
        setup();
        store_secret(ProviderCredentialField::AnthropicApiKey, "sk-test-123").unwrap();
        assert_eq!(
            get_secret(ProviderCredentialField::AnthropicApiKey).unwrap(),
            Some("sk-test-123".into())
        );
        delete_secret(ProviderCredentialField::AnthropicApiKey).unwrap();
        assert_eq!(
            get_secret(ProviderCredentialField::AnthropicApiKey).unwrap(),
            None
        );
    }

    #[test]
    fn test_empty_value_rejected() {
        setup();
        assert!(store_secret(ProviderCredentialField::GeminiApiKey, "").is_err());
        assert!(store_secret(ProviderCredentialField::GeminiApiKey, "   ").is_err());
    }

    #[test]
    fn test_delete_nonexistent_is_ok() {
        setup();
        assert!(delete_secret(ProviderCredentialField::CursorApiKey).is_ok());
    }

    #[test]
    fn test_credential_status_none_when_empty() {
        setup();
        let status = credential_status(ProviderCredentialField::OpenCodeServerPassword).unwrap();
        assert_eq!(status.source, CredentialSource::None);
    }

    #[test]
    fn test_credential_status_keychain_source() {
        setup();
        store_secret(ProviderCredentialField::CursorApiKey, "cursor-key").unwrap();
        let status = credential_status(ProviderCredentialField::CursorApiKey).unwrap();
        assert_eq!(status.source, CredentialSource::Keychain);
    }

    #[test]
    fn test_env_overrides_keychain() {
        setup();
        store_secret(ProviderCredentialField::AnthropicApiKey, "keychain-val").unwrap();
        // env var is already set in the test process (ANTHROPIC_API_KEY might be set)
        // We test the logic by using a field where we can control env
        std::env::set_var("OPENCODE_SERVER_PASSWORD", "env-val");
        store_secret(
            ProviderCredentialField::OpenCodeServerPassword,
            "keychain-val",
        )
        .unwrap();
        let (val, source) =
            resolve_credential(ProviderCredentialField::OpenCodeServerPassword).unwrap();
        assert_eq!(source, CredentialSource::Environment);
        assert_eq!(val.unwrap(), "env-val");
        std::env::remove_var("OPENCODE_SERVER_PASSWORD");
    }

    #[test]
    fn test_all_credential_statuses() {
        setup();
        let statuses = all_credential_statuses().unwrap();
        assert_eq!(statuses.len(), ProviderCredentialField::all().len());
    }

    #[test]
    fn test_from_provider_and_field() {
        assert_eq!(
            ProviderCredentialField::from_provider_and_field("claude", "api_key"),
            Some(ProviderCredentialField::AnthropicApiKey)
        );
        assert_eq!(
            ProviderCredentialField::from_provider_and_field("gemini", "api_key"),
            Some(ProviderCredentialField::GeminiApiKey)
        );
        assert_eq!(
            ProviderCredentialField::from_provider_and_field("cursor", "api_key"),
            Some(ProviderCredentialField::CursorApiKey)
        );
        assert_eq!(
            ProviderCredentialField::from_provider_and_field("opencode", "server_password"),
            Some(ProviderCredentialField::OpenCodeServerPassword)
        );
        assert_eq!(
            ProviderCredentialField::from_provider_and_field("unknown", "field"),
            None
        );
    }

    #[test]
    fn test_provider_ids_contain_expected_providers() {
        let field = ProviderCredentialField::AnthropicApiKey;
        let ids = field.provider_ids();
        assert!(ids.contains(&"claude"));
        assert!(ids.contains(&"claude_code"));
    }
}
