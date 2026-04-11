use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

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

    let can_submit = tools
        .iter()
        .any(|t| t.tool_name == "gh" && t.status == "ready");
    let available_providers: Vec<String> = tools
        .iter()
        .filter(|t| {
            (t.tool_name == "codex" || t.tool_name == "claude" || t.tool_name == "copilot")
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
                (t.tool_name == "codex" || t.tool_name == "claude" || t.tool_name == "copilot")
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
}
