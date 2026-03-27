use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::channels::manager::ChannelManager;
use crate::channels::secrets;
use crate::channels::ChannelStatus;
use crate::errors::AppError;
use crate::storage::db::AppDb;
use crate::storage::queries;

const CHANNELS_ENABLED_KEY: &str = "channels_enabled";

fn set_channels_enabled(conn: &rusqlite::Connection, enabled: bool) -> Result<(), AppError> {
    queries::upsert_setting(
        conn,
        CHANNELS_ENABLED_KEY,
        if enabled { "true" } else { "false" },
    )?;
    Ok(())
}

/// Holds the cancellation tokens for active channel listeners.
pub struct ChannelListenerTokens(pub Mutex<ChannelListenerState>);

#[derive(Default)]
pub struct ChannelListenerState {
    pub slack_cancel: Option<CancellationToken>,
    pub discord_cancel: Option<CancellationToken>,
}

#[tauri::command]
pub async fn configure_channel(source: String, token: String) -> Result<(), AppError> {
    secrets::store_token(&source, &token)?;
    Ok(())
}

#[tauri::command]
pub async fn remove_channel(
    source: String,
    tokens: tauri::State<'_, ChannelListenerTokens>,
    manager: tauri::State<'_, Arc<ChannelManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    // Stop the listener if running
    stop_single_listener(&source, &tokens).await;
    secrets::delete_token(&source)?;
    manager.update_status(ChannelStatus {
        source: source.clone(),
        connected: false,
        message: Some("Token removed".into()),
    });

    // If no tokens remain, ensure channels are disabled on next startup.
    if secrets::get_token("slack")?.is_none() && secrets::get_token("discord")?.is_none() {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        set_channels_enabled(&conn, false)?;
    }
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

/// Start channel listeners for all configured sources.
#[tauri::command]
pub async fn start_channel_listeners(
    tokens: tauri::State<'_, ChannelListenerTokens>,
    manager: tauri::State<'_, Arc<ChannelManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    // Persist "enabled" intent so auto-start on next launch respects the user's choice.
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        set_channels_enabled(&conn, true)?;
    }

    let mut state = tokens.0.lock().await;

    // Start Slack if token exists and not already running
    if state.slack_cancel.is_none() {
        if let Some(token) = secrets::get_token("slack")? {
            let cancel = CancellationToken::new();
            let mgr = manager.inner().clone();
            let c = cancel.clone();
            tokio::spawn(async move {
                crate::channels::slack::start_slack_listener(token, mgr, c).await;
            });
            state.slack_cancel = Some(cancel);
        }
    }

    // Start Discord if token exists and not already running
    if state.discord_cancel.is_none() {
        if let Some(token) = secrets::get_token("discord")? {
            let cancel = CancellationToken::new();
            let mgr = manager.inner().clone();
            let c = cancel.clone();
            tokio::spawn(async move {
                crate::channels::discord::start_discord_listener(token, mgr, c).await;
            });
            state.discord_cancel = Some(cancel);
        }
    }

    Ok(())
}

/// Stop all channel listeners.
#[tauri::command]
pub async fn stop_channel_listeners(
    tokens: tauri::State<'_, ChannelListenerTokens>,
    manager: tauri::State<'_, Arc<ChannelManager>>,
    db: tauri::State<'_, AppDb>,
) -> Result<(), AppError> {
    // Persist "disabled" intent so auto-start on next launch respects the user's choice.
    {
        let conn =
            db.0.lock()
                .map_err(|e| AppError::InvalidInput(e.to_string()))?;
        set_channels_enabled(&conn, false)?;
    }

    let mut state = tokens.0.lock().await;

    if let Some(cancel) = state.slack_cancel.take() {
        cancel.cancel();
        manager.update_status(ChannelStatus {
            source: "slack".into(),
            connected: false,
            message: Some("Stopped".into()),
        });
    }

    if let Some(cancel) = state.discord_cancel.take() {
        cancel.cancel();
        manager.update_status(ChannelStatus {
            source: "discord".into(),
            connected: false,
            message: Some("Stopped".into()),
        });
    }

    Ok(())
}

/// Helper: stop a single listener by source name.
async fn stop_single_listener(source: &str, tokens: &tauri::State<'_, ChannelListenerTokens>) {
    let mut state = tokens.0.lock().await;
    match source {
        "slack" => {
            if let Some(cancel) = state.slack_cancel.take() {
                cancel.cancel();
            }
        }
        "discord" => {
            if let Some(cancel) = state.discord_cancel.take() {
                cancel.cancel();
            }
        }
        _ => {}
    }
}
