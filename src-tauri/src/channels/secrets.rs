use crate::errors::AppError;

#[cfg(not(test))]
const SERVICE_NAME: &str = "signalpr";

fn account_for_source(source: &str) -> Result<&'static str, AppError> {
    match source {
        "slack" => Ok("slack_app_token"),
        "discord" => Ok("discord_bot_token"),
        _ => Err(AppError::InvalidInput(format!(
            "Unknown channel source '{}'",
            source
        ))),
    }
}

#[cfg(not(test))]
fn entry_for_source(source: &str) -> Result<keyring::Entry, AppError> {
    let account = account_for_source(source)?;
    keyring::Entry::new(SERVICE_NAME, account)
        .map_err(|e| AppError::Channel(format!("Keychain entry error: {e}")))
}

#[cfg(test)]
mod test_store {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static STORE: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();

    pub fn set(key: &str, value: &str) {
        let store = STORE.get_or_init(|| Mutex::new(HashMap::new()));
        store
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
    }

    pub fn get(key: &str) -> Option<String> {
        let store = STORE.get_or_init(|| Mutex::new(HashMap::new()));
        store.lock().unwrap().get(key).cloned()
    }

    pub fn delete(key: &str) {
        let store = STORE.get_or_init(|| Mutex::new(HashMap::new()));
        store.lock().unwrap().remove(key);
    }
}

/// Store a channel token in the OS keychain.
///
/// Never writes tokens to SQLite or logs.
pub fn store_token(source: &str, token: &str) -> Result<(), AppError> {
    if token.trim().is_empty() {
        return Err(AppError::InvalidInput("Token must be non-empty".into()));
    }

    #[cfg(test)]
    {
        let account = account_for_source(source)?;
        test_store::set(account, token);
        return Ok(());
    }

    #[cfg(not(test))]
    {
        let entry = entry_for_source(source)?;
        entry
            .set_password(token)
            .map_err(|e| AppError::Channel(format!("Failed to store token: {e}")))?;
        Ok(())
    }
}

/// Retrieve a stored channel token, if any.
pub fn get_token(source: &str) -> Result<Option<String>, AppError> {
    #[cfg(test)]
    {
        let account = account_for_source(source)?;
        return Ok(test_store::get(account));
    }

    #[cfg(not(test))]
    {
        let entry = entry_for_source(source)?;
        match entry.get_password() {
            Ok(pw) => Ok(Some(pw)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::Channel(format!("Failed to read token: {e}"))),
        }
    }
}

/// Delete a stored channel token. Returns Ok(()) whether or not a token was stored.
pub fn delete_token(source: &str) -> Result<(), AppError> {
    #[cfg(test)]
    {
        let account = account_for_source(source)?;
        test_store::delete(account);
        return Ok(());
    }

    #[cfg(not(test))]
    {
        let entry = entry_for_source(source)?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::Channel(format!("Failed to delete token: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_store_get_delete_round_trip() {
        store_token("slack", "xapp-test-token").unwrap();
        assert_eq!(get_token("slack").unwrap(), Some("xapp-test-token".into()));
        delete_token("slack").unwrap();
        assert_eq!(get_token("slack").unwrap(), None);
    }

    #[test]
    fn test_unknown_source_rejected() {
        assert!(store_token("unknown", "t").is_err());
        assert!(get_token("unknown").is_err());
        assert!(delete_token("unknown").is_err());
    }
}
