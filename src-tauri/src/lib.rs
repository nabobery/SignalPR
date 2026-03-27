mod agents;
mod autofix;
mod channels;
mod cleaner;
mod commands;
mod config;
mod errors;
mod notifications;
mod orchestration;
mod preferences;
mod providers;
mod storage;
mod tray;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use channels::manager::ChannelManager;

use commands::review::ActiveReviews;
use storage::db::init_db;
use storage::event_log::EventLog;
use tauri::Emitter;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_dir)?;
            let db = init_db(&app_dir)?;
            app.manage(db);
            app.manage(ActiveReviews(Mutex::new(HashMap::new())));

            // Event log for review pipeline diagnostics
            let event_log = Arc::new(EventLog::new(&app_dir));
            app.manage(event_log);

            // Channel manager for Slack/Discord listeners
            let channel_manager = Arc::new(ChannelManager::new());
            app.manage(channel_manager);

            // Forward channel events to the frontend event bus.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<ChannelManager>>().inner().clone();
                let mut rx = manager.subscribe();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(event) => {
                                let _ = app_handle.emit("channel_review_requested", event);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Set up system tray
            tray::setup_tray(app.handle())?;

            // Start GitHub notification poller (if enabled in settings)
            let poll_cancel = tokio_util::sync::CancellationToken::new();
            notifications::github_poll::maybe_start_poller(app.handle(), poll_cancel);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::environment::inspect_environment,
            commands::environment::get_environment_summary,
            commands::intake::open_from_url,
            commands::intake::confirm_workspace,
            commands::review::start_review,
            commands::review::cancel_review,
            commands::review::get_review_snapshot,
            commands::review::get_incomplete_reviews,
            commands::review::resume_review,
            commands::findings::update_finding,
            commands::submission::submit_review,
            commands::submission::get_submission_history,
            commands::settings::get_settings,
            commands::settings::update_setting,
            commands::diagnostics::export_diagnostic_bundle,
            commands::diagnostics::get_event_log,
            commands::preferences::record_decision,
            commands::preferences::get_preferences,
            commands::autofix::preview_fix,
            commands::autofix::apply_fix,
            commands::autofix::accept_fix,
            commands::autofix::reject_fix,
            commands::agents::get_agent_definitions,
            commands::agents::save_agent_definition,
            commands::agents::delete_agent_definition,
            commands::channels::configure_channel,
            commands::channels::remove_channel,
            commands::channels::get_channel_status,
            commands::channels::has_channel_token,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
