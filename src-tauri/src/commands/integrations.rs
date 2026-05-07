use crate::errors::AppError;
use crate::secrets::integrations as integration_secrets;
use crate::secrets::integrations::IntegrationSecretField;
use crate::storage::db::AppDb;
use crate::storage::queries;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntegrationStatus {
    pub id: String,
    pub enabled: bool,
    pub has_secret: bool,
    pub settings: serde_json::Value,
}

/// Returns integration enablement and status (no secret values exposed).
#[tauri::command]
pub async fn get_integration_statuses(
    db: tauri::State<'_, AppDb>,
) -> Result<Vec<IntegrationStatus>, AppError> {
    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;

    let jira_enabled = queries::get_setting(&conn, "integration_jira_enabled")
        .unwrap_or(None)
        .map(|v| v == "true")
        .unwrap_or(false);

    let linear_enabled = queries::get_setting(&conn, "integration_linear_enabled")
        .unwrap_or(None)
        .map(|v| v == "true")
        .unwrap_or(false);

    let jira_base_url = queries::get_setting(&conn, "integration_jira_base_url")
        .unwrap_or(None)
        .unwrap_or_default();
    let jira_email = queries::get_setting(&conn, "integration_jira_email")
        .unwrap_or(None)
        .unwrap_or_default();

    let linear_workspace = queries::get_setting(&conn, "integration_linear_workspace")
        .unwrap_or(None)
        .unwrap_or_default();

    let jira_has_secret = integration_secrets::has_secret(IntegrationSecretField::JiraApiToken)
        .unwrap_or_else(|err| {
            tracing::warn!("Failed to resolve Jira integration secret status: {}", err);
            false
        });
    let linear_has_secret = integration_secrets::has_secret(IntegrationSecretField::LinearApiKey)
        .unwrap_or_else(|err| {
            tracing::warn!(
                "Failed to resolve Linear integration secret status: {}",
                err
            );
            false
        });

    Ok(vec![
        IntegrationStatus {
            id: "jira".into(),
            enabled: jira_enabled,
            has_secret: jira_has_secret,
            settings: serde_json::json!({
                "base_url": jira_base_url,
                "email": jira_email,
                "project_keys": queries::get_setting(&conn, "integration_jira_project_keys")
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            }),
        },
        IntegrationStatus {
            id: "linear".into(),
            enabled: linear_enabled,
            has_secret: linear_has_secret,
            settings: serde_json::json!({
                "workspace": linear_workspace,
                "team_keys": queries::get_setting(&conn, "integration_linear_team_keys")
                    .ok()
                    .flatten()
                    .unwrap_or_default(),
            }),
        },
    ])
}

/// Store an integration secret in the OS keychain.
#[tauri::command]
pub async fn store_integration_secret(
    integration_id: String,
    value: String,
) -> Result<(), AppError> {
    let field = IntegrationSecretField::from_integration_id(&integration_id)
        .ok_or_else(|| AppError::InvalidInput(format!("Unknown integration: {integration_id}")))?;
    integration_secrets::store_secret(field, &value)
}

/// Delete an integration secret from the OS keychain.
#[tauri::command]
pub async fn delete_integration_secret(integration_id: String) -> Result<(), AppError> {
    let field = IntegrationSecretField::from_integration_id(&integration_id)
        .ok_or_else(|| AppError::InvalidInput(format!("Unknown integration: {integration_id}")))?;
    integration_secrets::delete_secret(field)
}

/// Update an integration setting (non-secret values like base_url, email, enabled).
#[tauri::command]
pub async fn update_integration_setting(
    key: String,
    value: String,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    let allowed_keys = [
        "integration_jira_enabled",
        "integration_jira_base_url",
        "integration_jira_email",
        "integration_jira_project_keys",
        "integration_linear_enabled",
        "integration_linear_workspace",
        "integration_linear_team_keys",
    ];

    if !allowed_keys.contains(&key.as_str()) {
        return Err(AppError::InvalidInput(format!(
            "Unknown integration setting key: {key}"
        )));
    }

    let conn =
        db.0.lock()
            .map_err(|e| AppError::InvalidInput(e.to_string()))?;
    queries::upsert_setting(&conn, &key, &value)?;
    Ok(())
}
