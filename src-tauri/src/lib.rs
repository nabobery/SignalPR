mod agents;
mod autofix;
mod channels;
mod cleaner;
mod commands;
mod config;
mod context_pack;
mod errors;
mod explainability;
mod local_checks;
mod metrics;
mod notifications;
mod orchestration;
mod preferences;
mod providers;
mod review_delta;
mod secrets;
mod storage;
mod tray;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use channels::manager::ChannelManager;
use providers::claude_code::manager::ClaudeCodeManager;
use providers::codex_app_server::manager::CodexAppServerManager;
use providers::copilot::manager::CopilotManager;
use providers::cursor::manager::CursorManager;
use providers::gemini::manager::GeminiManager;
use providers::opencode::manager::OpenCodeManager;
use providers::pi::manager::PiManager;
use serde::Serialize;

use commands::channels::ChannelListenerTokens;
use commands::review::ActiveReviews;
use storage::db::init_db;
use storage::event_log::EventLog;
use tauri::Emitter;
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
struct CodexLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

#[derive(Debug, Clone, Serialize)]
struct CopilotLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

#[derive(Debug, Clone, Serialize)]
struct OpenCodeLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

#[derive(Debug, Clone, Serialize)]
struct GeminiLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

#[derive(Debug, Clone, Serialize)]
struct CursorLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

#[derive(Debug, Clone, Serialize)]
struct PiLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

#[derive(Debug, Clone, Serialize)]
struct ClaudeCodeLaneDelta {
    lane_id: String,
    delta: String,
    buffer: String,
}

fn push_capped(buf: &mut String, delta: &str, max_len: usize) {
    if delta.is_empty() {
        return;
    }
    buf.push_str(delta);
    if buf.len() > max_len {
        // Walk forward from the overflow byte to the next codepoint
        // boundary so we never split a multi-byte sequence (CJK, emoji).
        // `String::drain(..n)` panics when `n` is mid-codepoint.
        let overflow = buf.len() - max_len;
        let mut drop = overflow;
        while drop < buf.len() && !buf.is_char_boundary(drop) {
            drop += 1;
        }
        buf.drain(..drop);
    }
}

fn extract_agent_message_text(item: &serde_json::Value) -> Option<&str> {
    match item.get("type").and_then(|v| v.as_str()) {
        Some("agentMessage") => item.get("text").and_then(|v| v.as_str()),
        Some("exitedReviewMode") => item.get("review").and_then(|v| v.as_str()),
        _ => None,
    }
}

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

            // Shared Codex App Server manager (lazy-started on first use)
            let codex_manager = Arc::new(CodexAppServerManager::new());
            app.manage(codex_manager);

            // Shared Copilot manager (lazy-started on first use)
            let copilot_manager = Arc::new(CopilotManager::new());
            app.manage(copilot_manager);

            // Shared OpenCode manager (lazy-started on first use)
            let opencode_manager = Arc::new(OpenCodeManager::new());
            app.manage(opencode_manager);

            // Shared Gemini manager (lazy-started on first use).
            // API-key auth only — OAuth is not supported (Google ToS restriction).
            let gemini_manager = Arc::new(GeminiManager::new());
            app.manage(gemini_manager);

            // Shared Cursor manager (lazy-started on first use).
            // API-key auth only — CURSOR_API_KEY must be set.
            let cursor_manager = Arc::new(CursorManager::new());
            app.manage(cursor_manager);

            // Shared PI manager (lazy-started on first use).
            // Requires `pi` CLI installed and API keys configured in PI's config.
            let pi_manager = Arc::new(PiManager::new());
            app.manage(pi_manager);

            // Shared Claude Code manager (per-review sidecar spawning).
            // Requires ANTHROPIC_API_KEY and the compiled sidecar binary.
            let claude_code_manager = Arc::new(ClaudeCodeManager::new());
            app.manage(claude_code_manager);

            // Channel listener lifecycle tokens
            app.manage(ChannelListenerTokens(tokio::sync::Mutex::new(
                commands::channels::ChannelListenerState::default(),
            )));

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

            // Forward Codex App Server approval requests to the frontend.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<CodexAppServerManager>>().inner().clone();
                let mut rx = manager.subscribe_approvals();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(req) => {
                                let _ = app_handle.emit("codex_approval_requested", req);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Forward Codex App Server streaming deltas to the frontend (lane-scoped).
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<CodexAppServerManager>>().inner().clone();
                let mut rx = manager.subscribe_notifications();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let notif = match rx.recv().await {
                            Ok(n) => n,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match notif.method.as_str() {
                            "item/agentMessage/delta" => {
                                let Some(params) = notif.params.as_ref() else {
                                    continue;
                                };
                                let Some(thread_id) =
                                    params.get("threadId").and_then(|v| v.as_str())
                                else {
                                    continue;
                                };
                                let Some(delta) = params.get("delta").and_then(|v| v.as_str())
                                else {
                                    continue;
                                };

                                let buf = buffers.entry(thread_id.to_string()).or_default();
                                push_capped(buf, delta, MAX_LANE_BUFFER);

                                let lane_id = manager
                                    .lane_for_thread(thread_id)
                                    .await
                                    .unwrap_or_else(|| thread_id.to_string());
                                let _ = app_handle.emit(
                                    "codex_lane_delta",
                                    CodexLaneDelta {
                                        lane_id,
                                        delta: delta.to_string(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "item/completed" => {
                                let Some(params) = notif.params.as_ref() else {
                                    continue;
                                };
                                let Some(thread_id) =
                                    params.get("threadId").and_then(|v| v.as_str())
                                else {
                                    continue;
                                };
                                let Some(item) = params.get("item") else {
                                    continue;
                                };
                                let Some(text) = extract_agent_message_text(item) else {
                                    continue;
                                };

                                let buf = buffers.entry(thread_id.to_string()).or_default();
                                buf.clear();
                                push_capped(buf, text, MAX_LANE_BUFFER);

                                let lane_id = manager
                                    .lane_for_thread(thread_id)
                                    .await
                                    .unwrap_or_else(|| thread_id.to_string());
                                let _ = app_handle.emit(
                                    "codex_lane_delta",
                                    CodexLaneDelta {
                                        lane_id,
                                        delta: String::new(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "turn/completed" => {
                                // Clear per-thread buffers to avoid unbounded growth.
                                if let Some(params) = notif.params.as_ref() {
                                    if let Some(thread_id) =
                                        params.get("threadId").and_then(|v| v.as_str())
                                    {
                                        buffers.remove(thread_id);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Forward Copilot permission requests to the frontend.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<CopilotManager>>().inner().clone();
                let mut rx = manager.subscribe_permissions();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(req) => {
                                let _ = app_handle.emit("copilot_permission_requested", req);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Forward Copilot streaming deltas to the frontend (lane-scoped).
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<CopilotManager>>().inner().clone();
                let mut rx = manager.subscribe_events();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let event = match rx.recv().await {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match event.event_type.as_str() {
                            "assistant.message_delta" => {
                                let delta = event
                                    .event
                                    .get("deltaContent")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default();

                                if delta.is_empty() {
                                    continue;
                                }

                                let buf = buffers.entry(event.session_id.clone()).or_default();
                                push_capped(buf, delta, MAX_LANE_BUFFER);

                                let lane_id = manager
                                    .lane_for_session(&event.session_id)
                                    .await
                                    .unwrap_or_else(|| event.session_id.clone());
                                let _ = app_handle.emit(
                                    "copilot_lane_delta",
                                    CopilotLaneDelta {
                                        lane_id,
                                        delta: delta.to_string(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "session.idle" | "session.error" => {
                                buffers.remove(&event.session_id);
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Forward OpenCode permission requests to the frontend.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<OpenCodeManager>>().inner().clone();
                let mut rx = manager.subscribe_permissions();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(req) => {
                                let _ = app_handle.emit("opencode_permission_requested", req);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Forward OpenCode streaming deltas to the frontend (lane-scoped).
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<OpenCodeManager>>().inner().clone();
                let mut rx = manager.subscribe_events();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let event = match rx.recv().await {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match event.event_type.as_str() {
                            "message.part.updated" => {
                                let delta = event
                                    .data
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default();

                                if delta.is_empty() {
                                    continue;
                                }

                                let buf = buffers.entry(event.session_id.clone()).or_default();
                                push_capped(buf, delta, MAX_LANE_BUFFER);

                                let lane_id = manager
                                    .lane_for_session(&event.session_id)
                                    .await
                                    .unwrap_or_else(|| event.session_id.clone());
                                let _ = app_handle.emit(
                                    "opencode_lane_delta",
                                    OpenCodeLaneDelta {
                                        lane_id,
                                        delta: delta.to_string(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "session.status" => {
                                let status = event
                                    .data
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default();
                                if status == "idle" || status == "error" || status == "completed" {
                                    buffers.remove(&event.session_id);
                                }
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Forward Gemini permission requests to the frontend.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<GeminiManager>>().inner().clone();
                let mut rx = manager.subscribe_permissions();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(req) => {
                                let _ = app_handle.emit("gemini_permission_requested", req);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Forward Gemini streaming deltas to the frontend (lane-scoped).
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<GeminiManager>>().inner().clone();
                let mut rx = manager.subscribe_events();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let event = match rx.recv().await {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match event.event_type.as_str() {
                            "agent_message_chunk" if !event.delta.is_empty() => {
                                let buf = buffers.entry(event.session_id.clone()).or_default();
                                push_capped(buf, &event.delta, MAX_LANE_BUFFER);

                                let lane_id = manager
                                    .lane_for_session(&event.session_id)
                                    .await
                                    .unwrap_or_else(|| event.session_id.clone());
                                let _ = app_handle.emit(
                                    "gemini_lane_delta",
                                    GeminiLaneDelta {
                                        lane_id,
                                        delta: event.delta.clone(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "session.prompt_complete" => {
                                // Synthetic event from GeminiManager::prompt;
                                // clear the per-lane delta buffer so it can't
                                // grow across turns.
                                buffers.remove(&event.session_id);
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Forward Cursor permission requests to the frontend.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<CursorManager>>().inner().clone();
                let mut rx = manager.subscribe_permissions();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(req) => {
                                let _ = app_handle.emit("cursor_permission_requested", req);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Forward Cursor streaming deltas to the frontend (lane-scoped).
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<CursorManager>>().inner().clone();
                let mut rx = manager.subscribe_events();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let event = match rx.recv().await {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match event.event_type.as_str() {
                            "agent_message_chunk" if !event.delta.is_empty() => {
                                let buf = buffers.entry(event.session_id.clone()).or_default();
                                push_capped(buf, &event.delta, MAX_LANE_BUFFER);

                                let lane_id = manager
                                    .lane_for_session(&event.session_id)
                                    .await
                                    .unwrap_or_else(|| event.session_id.clone());
                                let _ = app_handle.emit(
                                    "cursor_lane_delta",
                                    CursorLaneDelta {
                                        lane_id,
                                        delta: event.delta.clone(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "session.prompt_complete" => {
                                buffers.remove(&event.session_id);
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Forward PI streaming deltas to the frontend (lane-scoped).
            // PI has no permission model, so no permission forwarding block.
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<PiManager>>().inner().clone();
                let mut rx = manager.subscribe_events();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let event = match rx.recv().await {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match event.event_type.as_str() {
                            "message_update" if !event.delta.is_empty() => {
                                let buf = buffers.entry(event.lane_id.clone()).or_default();
                                push_capped(buf, &event.delta, MAX_LANE_BUFFER);
                                let _ = app_handle.emit(
                                    "pi_lane_delta",
                                    PiLaneDelta {
                                        lane_id: event.lane_id.clone(),
                                        delta: event.delta.clone(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "agent_end" => {
                                // Clear per-lane buffers when the full agent
                                // run finishes (not on turn_end, since PI may
                                // do multi-turn tool loops).
                                buffers.remove(&event.lane_id);
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Forward Claude Code permission requests to the frontend.
            {
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<ClaudeCodeManager>>().inner().clone();
                let mut rx = manager.subscribe_permissions();
                tauri::async_runtime::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok(req) => {
                                let _ = app_handle.emit("claude_code_permission_requested", req);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        }
                    }
                });
            }

            // Forward Claude Code streaming deltas to the frontend (lane-scoped).
            {
                const MAX_LANE_BUFFER: usize = 16 * 1024;
                let app_handle = app.handle().clone();
                let manager = app.state::<Arc<ClaudeCodeManager>>().inner().clone();
                let mut rx = manager.subscribe_events();
                tauri::async_runtime::spawn(async move {
                    let mut buffers: HashMap<String, String> = HashMap::new();
                    loop {
                        let event = match rx.recv().await {
                            Ok(e) => e,
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        };

                        match event.event_type.as_str() {
                            "review.delta" if !event.delta.is_empty() => {
                                let buf = buffers.entry(event.lane_id.clone()).or_default();
                                push_capped(buf, &event.delta, MAX_LANE_BUFFER);
                                let _ = app_handle.emit(
                                    "claude_code_lane_delta",
                                    ClaudeCodeLaneDelta {
                                        lane_id: event.lane_id.clone(),
                                        delta: event.delta.clone(),
                                        buffer: buf.clone(),
                                    },
                                );
                            }
                            "review.completed" | "review.error" => {
                                buffers.remove(&event.lane_id);
                            }
                            _ => {}
                        }
                    }
                });
            }

            // Set up system tray
            tray::setup_tray(app.handle())?;

            // Start GitHub notification poller (if enabled in settings)
            let poll_cancel = tokio_util::sync::CancellationToken::new();
            notifications::github_poll::maybe_start_poller(app.handle(), poll_cancel);

            // Auto-start channel listeners if tokens are configured
            {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Respect user intent: only auto-start when explicitly enabled.
                    let channels_enabled = {
                        let db = app_handle.state::<storage::db::AppDb>();
                        let conn = match db.0.lock() {
                            Ok(c) => c,
                            Err(_) => return,
                        };
                        storage::queries::get_setting(&conn, "channels_enabled")
                            .ok()
                            .flatten()
                            .as_deref()
                            == Some("true")
                    };
                    if !channels_enabled {
                        tracing::debug!("Channels disabled; skipping auto-start");
                        return;
                    }

                    let tokens = app_handle.state::<ChannelListenerTokens>();
                    let manager = app_handle.state::<Arc<ChannelManager>>();
                    let mut state = tokens.0.lock().await;

                    if let Ok(Some(token)) = channels::secrets::get_token("slack") {
                        let cancel = tokio_util::sync::CancellationToken::new();
                        let mgr = manager.inner().clone();
                        let c = cancel.clone();
                        tokio::spawn(async move {
                            channels::slack::start_slack_listener(token, mgr, c).await;
                        });
                        state.slack_cancel = Some(cancel);
                        tracing::info!("Auto-started Slack listener");
                    }

                    if let Ok(Some(token)) = channels::secrets::get_token("discord") {
                        let cancel = tokio_util::sync::CancellationToken::new();
                        let mgr = manager.inner().clone();
                        let c = cancel.clone();
                        tokio::spawn(async move {
                            channels::discord::start_discord_listener(token, mgr, c).await;
                        });
                        state.discord_cancel = Some(cancel);
                        tracing::info!("Auto-started Discord listener");
                    }
                });
            }

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
            commands::codex::resolve_codex_approval,
            commands::copilot::resolve_copilot_permission,
            commands::opencode::resolve_opencode_permission,
            commands::gemini::resolve_gemini_permission,
            commands::cursor::resolve_cursor_permission,
            commands::claude_code::resolve_claude_code_permission,
            commands::agents::get_agent_definitions,
            commands::agents::save_agent_definition,
            commands::agents::delete_agent_definition,
            commands::channels::configure_channel,
            commands::channels::remove_channel,
            commands::channels::get_channel_status,
            commands::channels::has_channel_token,
            commands::channels::start_channel_listeners,
            commands::channels::stop_channel_listeners,
            commands::providers::get_provider_credential_statuses,
            commands::providers::store_provider_secret,
            commands::providers::delete_provider_secret,
            commands::providers::get_provider_capabilities,
            commands::providers::get_agent_run_metadata,
            commands::inbox::get_inbox_overview,
            commands::drafts::get_review_draft,
            commands::drafts::save_review_draft,
            commands::rerun::rerun_review,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_capped_ascii_truncation_unchanged() {
        let mut buf = String::from("abcdef");
        push_capped(&mut buf, "ghij", 5);
        // Total would be 10 bytes, cap is 5 — oldest 5 bytes drop.
        assert_eq!(buf.len(), 5);
        assert_eq!(buf, "fghij");
    }

    #[test]
    fn test_push_capped_empty_noop() {
        let mut buf = String::from("hello");
        push_capped(&mut buf, "", 2);
        assert_eq!(buf, "hello");
    }

    #[test]
    fn test_push_capped_under_cap_no_truncation() {
        let mut buf = String::from("hi");
        push_capped(&mut buf, " there", 100);
        assert_eq!(buf, "hi there");
    }

    #[test]
    fn test_push_capped_cjk_no_panic() {
        // 10_000 three-byte codepoints; cap at 1024 bytes. Byte boundary
        // would land mid-codepoint if we used a naive drain.
        let mut buf = String::new();
        let delta = "日".repeat(10_000);
        push_capped(&mut buf, &delta, 1024);
        assert!(buf.len() <= 1024 + 3, "buf len {}", buf.len());
        // The trailing content must be valid UTF-8 and end on a `日`.
        assert!(buf.chars().all(|c| c == '日'));
    }

    #[test]
    fn test_push_capped_emoji_no_panic() {
        // 4-byte emoji codepoints — same boundary hazard.
        let mut buf = String::new();
        let delta = "😀".repeat(10_000);
        push_capped(&mut buf, &delta, 1024);
        assert!(buf.len() <= 1024 + 4, "buf len {}", buf.len());
        assert!(buf.chars().all(|c| c == '😀'));
    }

    #[test]
    fn test_push_capped_repeated_appends_stay_bounded() {
        let mut buf = String::new();
        for _ in 0..100 {
            push_capped(&mut buf, "日本語", 16);
        }
        // Must never exceed cap by more than one codepoint width.
        assert!(buf.len() <= 16 + 3);
        assert!(!buf.is_empty());
    }
}
