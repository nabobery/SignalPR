use std::sync::Arc;

use crate::channels::manager::ChannelManager;
use crate::channels::secrets;
use crate::channels::ChannelStatus;
use crate::errors::AppError;

#[tauri::command]
pub async fn configure_channel(source: String, token: String) -> Result<(), AppError> {
    secrets::store_token(&source, &token)?;
    Ok(())
}

#[tauri::command]
pub async fn remove_channel(source: String) -> Result<(), AppError> {
    secrets::delete_token(&source)?;
    Ok(())
}

#[tauri::command]
pub async fn get_channel_status(
    manager: tauri::State<'_, Arc<ChannelManager>>,
) -> Result<Vec<ChannelStatus>, AppError> {
    let statuses = manager.get_statuses();
    // Always include the known sources so the UI can render stable rows.
    let mut out = Vec::new();
    for source in ["slack", "discord"] {
        if let Some(s) = statuses.iter().find(|s| s.source == source) {
            out.push(s.clone());
        } else {
            out.push(ChannelStatus {
                source: source.to_string(),
                connected: false,
                message: Some("Not connected".into()),
            });
        }
    }
    Ok(out)
}

#[tauri::command]
pub async fn has_channel_token(source: String) -> Result<bool, AppError> {
    Ok(secrets::get_token(&source)?.is_some())
}
