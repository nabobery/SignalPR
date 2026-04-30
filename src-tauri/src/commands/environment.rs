use serde::Serialize;
use tauri::AppHandle;
use tauri::Manager;
use tauri_plugin_shell::ShellExt;

use crate::providers::claude_code::manager::ClaudeCodeManager;
use crate::storage::models::ToolStatus;

#[derive(Debug, Serialize)]
pub struct EnvironmentSummary {
    pub can_review: bool,
    pub can_submit: bool,
    pub available_providers: Vec<String>,
    pub warnings: Vec<String>,
    pub tools: Vec<ToolStatus>,
}

#[tauri::command]
pub async fn inspect_environment(
    app: AppHandle,
) -> Result<Vec<ToolStatus>, crate::errors::AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut results = vec![];

    results.push(check_gh(&app, &now).await);
    results.push(check_codex(&app, &now).await);
    results.push(check_copilot(&app, &now).await);
    results.push(check_opencode(&app, &now).await);
    results.push(check_gemini(&app, &now).await);
    results.push(check_claude_code(&app, &now));

    Ok(results)
}

#[tauri::command]
pub async fn get_environment_summary(
    app: AppHandle,
) -> Result<EnvironmentSummary, crate::errors::AppError> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut tools = vec![];

    tools.push(check_gh(&app, &now).await);
    tools.push(check_codex(&app, &now).await);
    tools.push(check_claude(&now));
    tools.push(check_copilot(&app, &now).await);
    tools.push(check_opencode(&app, &now).await);
    tools.push(check_gemini(&app, &now).await);

    tools.push(check_claude_code(&app, &now));

    let can_submit = tools
        .iter()
        .any(|t| t.tool_name == "gh" && t.status == "ready");
    let available_providers: Vec<String> = tools
        .iter()
        .filter(|t| {
            (t.tool_name == "codex"
                || t.tool_name == "claude"
                || t.tool_name == "copilot"
                || t.tool_name == "opencode"
                || t.tool_name == "gemini"
                || t.tool_name == "claude_code")
                && t.status == "ready"
        })
        .map(|t| t.tool_name.clone())
        .collect();
    let can_review = !available_providers.is_empty();

    let mut warnings = Vec::new();
    if !can_review {
        warnings.push("No AI providers available".into());
    }
    if !can_submit {
        warnings.push("GitHub CLI not ready".into());
    }

    Ok(EnvironmentSummary {
        can_review,
        can_submit,
        available_providers,
        warnings,
        tools,
    })
}

async fn check_gh(app: &AppHandle, now: &str) -> ToolStatus {
    let shell = app.shell();

    // Check version
    let version = match shell.command("gh").args(["--version"]).output().await {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(2))
                .map(|v| v.to_string())
        }
        _ => None,
    };

    if version.is_none() {
        return ToolStatus {
            tool_name: "gh".into(),
            status: "missing".into(),
            version: None,
            message: Some("Install GitHub CLI: https://cli.github.com/".into()),
            checked_at: now.into(),
        };
    }

    // Check auth
    let authenticated = match shell.command("gh").args(["auth", "status"]).output().await {
        Ok(output) => output.status.success(),
        Err(_) => false,
    };

    if !authenticated {
        return ToolStatus {
            tool_name: "gh".into(),
            status: "unauthenticated".into(),
            version,
            message: Some("Run: gh auth login".into()),
            checked_at: now.into(),
        };
    }

    ToolStatus {
        tool_name: "gh".into(),
        status: "ready".into(),
        version,
        message: None,
        checked_at: now.into(),
    }
}

fn check_claude(now: &str) -> ToolStatus {
    match std::env::var("ANTHROPIC_API_KEY") {
        Ok(val) if !val.is_empty() => ToolStatus {
            tool_name: "claude".into(),
            status: "ready".into(),
            version: None,
            message: None,
            checked_at: now.into(),
        },
        _ => ToolStatus {
            tool_name: "claude".into(),
            status: "missing".into(),
            version: None,
            message: Some("Set ANTHROPIC_API_KEY environment variable".into()),
            checked_at: now.into(),
        },
    }
}

async fn check_copilot(app: &AppHandle, now: &str) -> ToolStatus {
    let shell = app.shell();
    let cli = std::env::var("COPILOT_CLI_PATH").unwrap_or_else(|_| "copilot".to_string());

    match shell.command(&cli).args(["--version"]).output().await {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ToolStatus {
                tool_name: "copilot".into(),
                status: "ready".into(),
                version: Some(version),
                message: None,
                checked_at: now.into(),
            }
        }
        _ => ToolStatus {
            tool_name: "copilot".into(),
            status: "missing".into(),
            version: None,
            message: Some("Optional: Install GitHub Copilot CLI".into()),
            checked_at: now.into(),
        },
    }
}

async fn check_codex(app: &AppHandle, now: &str) -> ToolStatus {
    let shell = app.shell();

    match shell.command("codex").args(["--version"]).output().await {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ToolStatus {
                tool_name: "codex".into(),
                status: "ready".into(),
                version: Some(version),
                message: None,
                checked_at: now.into(),
            }
        }
        _ => ToolStatus {
            tool_name: "codex".into(),
            status: "missing".into(),
            version: None,
            message: Some("Optional: Install Codex CLI from https://openai.com/codex/".into()),
            checked_at: now.into(),
        },
    }
}

async fn check_opencode(app: &AppHandle, now: &str) -> ToolStatus {
    let shell = app.shell();
    let cli = std::env::var("OPENCODE_CLI_PATH").unwrap_or_else(|_| "opencode".to_string());

    match shell.command(&cli).args(["--version"]).output().await {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            ToolStatus {
                tool_name: "opencode".into(),
                status: "ready".into(),
                version: Some(version),
                message: None,
                checked_at: now.into(),
            }
        }
        _ => ToolStatus {
            tool_name: "opencode".into(),
            status: "missing".into(),
            version: None,
            message: Some("Optional: Install OpenCode CLI (https://opencode.ai)".into()),
            checked_at: now.into(),
        },
    }
}

/// Check whether the Gemini CLI is installed and authenticated via an
/// API-key env var. OAuth is not supported for SignalPR — see the Gemini
/// CLI ToS notice at
/// https://github.com/google-gemini/gemini-cli/blob/main/docs/resources/tos-privacy.md
async fn check_gemini(app: &AppHandle, now: &str) -> ToolStatus {
    let shell = app.shell();
    let cli = std::env::var("GEMINI_CLI_PATH").unwrap_or_else(|_| "gemini".to_string());

    let version = match shell.command(&cli).args(["--version"]).output().await {
        Ok(output) if output.status.success() => {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        _ => None,
    };

    if version.is_none() {
        return ToolStatus {
            tool_name: "gemini".into(),
            status: "missing".into(),
            version: None,
            message: Some("Optional: Install Gemini CLI (`npm i -g @google/gemini-cli`)".into()),
            checked_at: now.into(),
        };
    }

    // Binary is present; verify an API-key auth env var is set so health
    // checks fail fast rather than blocking on a first-run interactive prompt.
    let has_auth = std::env::var("GEMINI_API_KEY").is_ok()
        || std::env::var("GOOGLE_API_KEY").is_ok()
        || std::env::var("GOOGLE_APPLICATION_CREDENTIALS").is_ok();

    if !has_auth {
        return ToolStatus {
            tool_name: "gemini".into(),
            status: "unauthenticated".into(),
            version,
            message: Some(
                "Set GEMINI_API_KEY in your shell before launching SignalPR. \
                 OAuth is not supported for third-party harnesses."
                    .into(),
            ),
            checked_at: now.into(),
        };
    }

    ToolStatus {
        tool_name: "gemini".into(),
        status: "ready".into(),
        version,
        message: None,
        checked_at: now.into(),
    }
}

fn check_claude_code(app: &AppHandle, now: &str) -> ToolStatus {
    let app_data_dir = app.path().app_data_dir().unwrap_or_default();
    let sidecar_path = crate::config::resolve_sidecar_path_pub("claude-code-bridge");

    if let Err(error) =
        ClaudeCodeManager::validate_sidecar_binary(std::path::Path::new(&sidecar_path))
    {
        return ToolStatus {
            tool_name: "claude_code".into(),
            status: "missing".into(),
            version: None,
            message: Some(error),
            checked_at: now.into(),
        };
    }

    let has_key = std::env::var("ANTHROPIC_API_KEY").is_ok();
    match ClaudeCodeManager::check_health(&sidecar_path, &app_data_dir, !has_key) {
        Ok(info) => {
            if !has_key {
                return ToolStatus {
                    tool_name: "claude_code".into(),
                    status: "unauthenticated".into(),
                    version: Some(format!("bridge={}", info.bridge_version)),
                    message: Some("Set ANTHROPIC_API_KEY to use Claude Code provider.".into()),
                    checked_at: now.into(),
                };
            }
            ToolStatus {
                tool_name: "claude_code".into(),
                status: "ready".into(),
                version: Some(format!(
                    "bridge={} sdk={}",
                    info.bridge_version, info.sdk_version
                )),
                message: None,
                checked_at: now.into(),
            }
        }
        Err(e) => ToolStatus {
            tool_name: "claude_code".into(),
            status: "degraded".into(),
            version: None,
            message: Some(format!("Health check failed: {}", e)),
            checked_at: now.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(name: &str, status: &str) -> crate::storage::models::ToolStatus {
        crate::storage::models::ToolStatus {
            tool_name: name.into(),
            status: status.into(),
            version: None,
            message: None,
            checked_at: "2026-01-01".into(),
        }
    }

    fn build_summary(tools: &[crate::storage::models::ToolStatus]) -> EnvironmentSummary {
        let can_submit = tools
            .iter()
            .any(|t| t.tool_name == "gh" && t.status == "ready");
        let available_providers: Vec<String> = tools
            .iter()
            .filter(|t| {
                (t.tool_name == "codex"
                    || t.tool_name == "claude"
                    || t.tool_name == "copilot"
                    || t.tool_name == "opencode"
                    || t.tool_name == "gemini")
                    && t.status == "ready"
            })
            .map(|t| t.tool_name.clone())
            .collect();
        let can_review = !available_providers.is_empty();
        let mut warnings = Vec::new();
        if !can_review {
            warnings.push("No AI providers available".into());
        }
        if !can_submit {
            warnings.push("GitHub CLI not ready".into());
        }
        EnvironmentSummary {
            can_review,
            can_submit,
            available_providers,
            warnings,
            tools: tools.to_vec(),
        }
    }

    #[test]
    fn test_no_providers_cant_review() {
        let summary = build_summary(&[tool("gh", "ready"), tool("codex", "missing")]);
        assert!(!summary.can_review);
        assert!(summary.can_submit);
    }

    #[test]
    fn test_partial_providers_can_review() {
        let summary = build_summary(&[
            tool("gh", "ready"),
            tool("codex", "missing"),
            tool("claude", "ready"),
        ]);
        assert!(summary.can_review);
        assert_eq!(summary.available_providers, vec!["claude"]);
    }

    #[test]
    fn test_all_providers_ready() {
        let summary = build_summary(&[
            tool("gh", "ready"),
            tool("codex", "ready"),
            tool("claude", "ready"),
        ]);
        assert!(summary.can_review);
        assert_eq!(summary.available_providers.len(), 2);
    }

    #[test]
    fn test_gemini_ready_included_in_available_providers() {
        let summary = build_summary(&[tool("gh", "ready"), tool("gemini", "ready")]);
        assert!(summary.can_review);
        assert_eq!(summary.available_providers, vec!["gemini"]);
    }

    #[test]
    fn test_gemini_unauthenticated_not_in_available_providers() {
        let summary = build_summary(&[
            tool("gh", "ready"),
            tool("gemini", "unauthenticated"),
            tool("claude", "ready"),
        ]);
        // Unauthenticated gemini must not count as an available provider
        // (we'd fail on session/new otherwise).
        assert_eq!(summary.available_providers, vec!["claude"]);
    }

    #[test]
    fn test_gemini_missing_not_in_available_providers() {
        let summary = build_summary(&[tool("gh", "ready"), tool("gemini", "missing")]);
        assert!(!summary.can_review);
    }
}
