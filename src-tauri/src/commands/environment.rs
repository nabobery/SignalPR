use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

use crate::storage::models::ToolStatus;

#[tauri::command]
pub async fn inspect_environment(app: AppHandle) -> Result<Vec<ToolStatus>, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut results = vec![];

    results.push(check_gh(&app, &now).await);
    results.push(check_codex(&app, &now).await);

    Ok(results)
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
