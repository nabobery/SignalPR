mod cleaner;
mod commands;
mod errors;
mod notifications;
mod orchestration;
mod providers;
mod storage;
mod tray;

use std::collections::HashMap;
use std::sync::Mutex;

use commands::review::ActiveReviews;
use storage::db::init_db;
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

            // Set up system tray
            tray::setup_tray(app.handle())?;

            // Start GitHub notification poller (if enabled in settings)
            let poll_cancel = tokio_util::sync::CancellationToken::new();
            notifications::github_poll::maybe_start_poller(app.handle(), poll_cancel);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::environment::inspect_environment,
            commands::intake::open_from_url,
            commands::intake::confirm_workspace,
            commands::review::start_review,
            commands::review::cancel_review,
            commands::review::get_review_snapshot,
            commands::review::get_incomplete_reviews,
            commands::review::resume_review,
            commands::findings::update_finding,
            commands::submission::submit_review,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
