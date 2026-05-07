use serde::{Deserialize, Serialize};

use crate::errors::AppError;

#[cfg(not(test))]
const SERVICE_NAME: &str = "signalpr-integrations";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationSecretField {
    JiraApiToken,
    LinearApiKey,
}

impl IntegrationSecretField {
    pub fn from_integration_id(integration_id: &str) -> Option<Self> {
        match integration_id {
            "jira" => Some(Self::JiraApiToken),
            "linear" => Some(Self::LinearApiKey),
            _ => None,
        }
    }

    pub fn keychain_account(&self) -> &'static str {
        match self {
            Self::JiraApiToken => "integration_jira_api_token",
            Self::LinearApiKey => "integration_linear_api_key",
        }
    }

    pub fn env_var_name(&self) -> &'static str {
        match self {
            Self::JiraApiToken => "JIRA_API_TOKEN",
            Self::LinearApiKey => "LINEAR_API_KEY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationSecretSource {
    Environment,
    Keychain,
    None,
}

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
fn keyring_entry(field: IntegrationSecretField) -> Result<keyring::Entry, AppError> {
    keyring::Entry::new(SERVICE_NAME, field.keychain_account())
        .map_err(|e| AppError::Channel(format!("Keychain entry error: {e}")))
}

pub fn store_secret(field: IntegrationSecretField, value: &str) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "Secret value must be non-empty".to_string(),
        ));
    }

    #[cfg(test)]
    {
        test_store::set(field.keychain_account(), value);
        Ok(())
    }

    #[cfg(not(test))]
    {
        let entry = keyring_entry(field)?;
        entry
            .set_password(value)
            .map_err(|e| AppError::Channel(format!("Failed to store secret: {e}")))?;
        Ok(())
    }
}

pub fn get_secret(field: IntegrationSecretField) -> Result<Option<String>, AppError> {
    #[cfg(test)]
    {
        Ok(test_store::get(field.keychain_account()))
    }

    #[cfg(not(test))]
    {
        let entry = keyring_entry(field)?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Channel(format!("Failed to read secret: {e}"))),
        }
    }
}

pub fn delete_secret(field: IntegrationSecretField) -> Result<(), AppError> {
    #[cfg(test)]
    {
        test_store::delete(field.keychain_account());
        Ok(())
    }

    #[cfg(not(test))]
    {
        let entry = keyring_entry(field)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Channel(format!("Failed to delete secret: {e}"))),
        }
    }
}

pub fn resolve_secret(
    field: IntegrationSecretField,
) -> Result<(Option<String>, IntegrationSecretSource), AppError> {
    if let Ok(value) = std::env::var(field.env_var_name()) {
        if !value.trim().is_empty() {
            return Ok((Some(value), IntegrationSecretSource::Environment));
        }
    }

    match get_secret(field)? {
        Some(value) => Ok((Some(value), IntegrationSecretSource::Keychain)),
        None => Ok((None, IntegrationSecretSource::None)),
    }
}

pub fn has_secret(field: IntegrationSecretField) -> Result<bool, AppError> {
    let (value, _source) = resolve_secret(field)?;
    Ok(value.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() {
        test_store::clear();
    }

    #[test]
    fn round_trip_store_get_delete() {
        setup();
        let field = IntegrationSecretField::JiraApiToken;
        store_secret(field, "jira-token").unwrap();
        assert_eq!(get_secret(field).unwrap(), Some("jira-token".to_string()));
        delete_secret(field).unwrap();
        assert_eq!(get_secret(field).unwrap(), None);
    }

    #[test]
    fn resolve_secret_prefers_keychain_when_env_absent() {
        setup();
        let field = IntegrationSecretField::LinearApiKey;
        store_secret(field, "linear-token").unwrap();
        let (value, source) = resolve_secret(field).unwrap();
        assert_eq!(value, Some("linear-token".to_string()));
        assert_eq!(source, IntegrationSecretSource::Keychain);
    }

    #[test]
    fn store_rejects_empty_values() {
        setup();
        let field = IntegrationSecretField::LinearApiKey;
        assert!(store_secret(field, " ").is_err());
    }
}
