use crate::errors::AppError;
use crate::providers::capabilities::{self, ProviderCapabilities};
use crate::secrets::credentials::{self, CredentialStatus, ProviderCredentialField};
use crate::storage::db::AppDb;
use crate::storage::models::AgentRun;
use crate::storage::queries;

/// Returns the credential status for all known provider secret fields.
/// Never exposes actual secret values — only source (env/keychain/none).
#[tauri::command]
pub async fn get_provider_credential_statuses() -> Result<Vec<CredentialStatus>, AppError> {
    credentials::all_credential_statuses()
}

/// Store a provider secret in the OS keychain.
/// `provider_id` + `field` are mapped to the internal credential field enum.
#[tauri::command]
pub async fn store_provider_secret(
    provider_id: String,
    field: String,
    value: String,
) -> Result<(), AppError> {
    let cred_field = ProviderCredentialField::from_provider_and_field(&provider_id, &field)
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown credential field '{field}' for provider '{provider_id}'"
            ))
        })?;
    credentials::store_secret(cred_field, &value)
}

/// Delete a provider secret from the OS keychain.
#[tauri::command]
pub async fn delete_provider_secret(provider_id: String, field: String) -> Result<(), AppError> {
    let cred_field = ProviderCredentialField::from_provider_and_field(&provider_id, &field)
        .ok_or_else(|| {
            AppError::InvalidInput(format!(
                "Unknown credential field '{field}' for provider '{provider_id}'"
            ))
        })?;
    credentials::delete_secret(cred_field)
}

/// Returns the full provider capabilities registry.
#[tauri::command]
pub async fn get_provider_capabilities() -> Result<Vec<ProviderCapabilities>, AppError> {
    Ok(capabilities::provider_registry())
}

/// Returns agent run metadata (including session/checkpoint info) for a review run.
#[tauri::command]
pub async fn get_agent_run_metadata(
    run_id: String,
    db: tauri::State<'_, AppDb>,
) -> Result<Vec<AgentRun>, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    Ok(queries::get_agent_runs_for_review(&conn, &run_id)?)
}
