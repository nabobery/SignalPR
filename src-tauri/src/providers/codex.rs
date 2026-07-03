use std::path::{Path, PathBuf};
use std::process::Stdio;

use async_trait::async_trait;
use tauri_plugin_shell::ShellExt;
use tokio_util::sync::CancellationToken;

use crate::errors::ProviderError;

use super::prompts::OUTPUT_SCHEMA;
use super::traits::{CodexReviewOutput, ProviderHealth, RawFinding, ReviewInput, ReviewProvider};

/// Live Codex provider using `codex exec` CLI with structured output schema.
/// Providers are "prompt in, JSON out" — prompt construction is owned by the
/// orchestration layer via `providers::prompts`.
#[allow(dead_code)]
pub struct CodexProvider {
    app_handle: tauri::AppHandle,
    /// None means "use the user's configured default model" — hardcoding a
    /// model breaks ChatGPT-subscription accounts, which reject explicit
    /// API-only model ids.
    model: Option<String>,
}

#[allow(dead_code)]
impl CodexProvider {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            model: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_model(app_handle: tauri::AppHandle, model: String) -> Self {
        Self {
            app_handle,
            model: Some(model),
        }
    }
}

/// Build the `codex exec` argument list. Extracted for regression testing —
/// codex-cli renames flags between releases (`--output-schema-file` became
/// `--output-schema` in 0.14x) and a stale flag fails as an opaque broken pipe.
fn build_exec_args(model: Option<&str>, schema_path: &str) -> Vec<String> {
    let mut args = vec!["exec".to_string()];
    if let Some(model) = model {
        args.push("--model".into());
        args.push(model.to_string());
    }
    args.push("--output-schema".into());
    args.push(schema_path.to_string());
    args.push("-".into()); // read prompt from stdin
    args
}

#[async_trait]
impl ReviewProvider for CodexProvider {
    fn provider_name(&self) -> &str {
        "codex"
    }

    async fn health_check(&self) -> ProviderHealth {
        let shell = self.app_handle.shell();
        match shell.command("codex").args(["--version"]).output().await {
            Ok(output) if output.status.success() => ProviderHealth {
                available: true,
                version: Some(String::from_utf8_lossy(&output.stdout).trim().to_string()),
                message: None,
            },
            _ => ProviderHealth {
                available: false,
                version: None,
                message: Some("Codex CLI not found".into()),
            },
        }
    }

    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        if cancel.is_cancelled() {
            return Err(ProviderError::Cancelled);
        }
        let tmp_dir = tempfile::tempdir()?;

        // Write output schema to temp file (use input schema or fall back to default)
        let schema_path = tmp_dir.path().join("output-schema.json");
        let schema = if input.output_schema.is_empty() {
            OUTPUT_SCHEMA
        } else {
            &input.output_schema
        };
        std::fs::write(&schema_path, schema)?;

        // Build full prompt from input
        let prompt = format!("{}\n\nPR Diff:\n{}", input.system_prompt, input.diff);

        // Execute codex exec (prompt via stdin to avoid argv size limits)
        let mut cmd = tokio::process::Command::new("codex");
        cmd.args(build_exec_args(
            self.model.as_deref(),
            &schema_path.to_string_lossy(),
        ))
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| ProviderError::CodexFailed(format!("Failed to spawn codex: {}", e)))?;

        // A write failure here usually means codex exited immediately (bad
        // flag, auth error). Don't bail yet — fall through so the child's
        // stderr (the actual cause) is captured and reported instead of an
        // opaque EPIPE.
        let mut prompt_write_error: Option<std::io::Error> = None;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            if let Err(e) = stdin.write_all(prompt.as_bytes()).await {
                prompt_write_error = Some(e);
            }
        }

        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::CodexFailed("Missing stdout pipe".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProviderError::CodexFailed("Missing stderr pipe".into()))?;

        use tokio::io::AsyncReadExt;
        let stdout_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            stdout.read_to_end(&mut buf).await?;
            Ok::<_, std::io::Error>(buf)
        });
        let stderr_task = tokio::spawn(async move {
            let mut buf = Vec::new();
            stderr.read_to_end(&mut buf).await?;
            Ok::<_, std::io::Error>(buf)
        });

        let status = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Err(ProviderError::Cancelled);
            }
            status = child.wait() => status
        }
        .map_err(|e| ProviderError::CodexFailed(format!("Failed waiting for codex: {}", e)))?;

        let stdout_buf = stdout_task
            .await
            .map_err(|e| ProviderError::CodexFailed(format!("stdout join error: {}", e)))?
            .map_err(|e| ProviderError::CodexFailed(format!("stdout read error: {}", e)))?;
        let stderr_buf = stderr_task
            .await
            .map_err(|e| ProviderError::CodexFailed(format!("stderr join error: {}", e)))?
            .map_err(|e| ProviderError::CodexFailed(format!("stderr read error: {}", e)))?;

        if !status.success() || prompt_write_error.is_some() {
            let stderr = String::from_utf8_lossy(&stderr_buf).trim().to_string();
            let detail = if !stderr.is_empty() {
                stderr
            } else if let Some(e) = prompt_write_error {
                format!("Failed to write prompt: {}", e)
            } else {
                format!("exit status {:?} with empty stderr", status.code())
            };
            return Err(ProviderError::CodexFailed(format!(
                "Codex exited with error: {}",
                detail
            )));
        }

        let result: CodexReviewOutput = serde_json::from_slice(&stdout_buf)?;
        Ok(result)
    }
}

/// Mock provider that reads findings from a fixture JSON file.
/// Used for development and testing when Codex CLI is not available.
pub struct MockProvider {
    fixture_path: PathBuf,
}

#[allow(dead_code)]
impl MockProvider {
    pub fn new(fixture_path: PathBuf) -> Self {
        Self { fixture_path }
    }

    pub fn with_default_fixture() -> Self {
        Self {
            fixture_path: PathBuf::new(), // Will use inline fixture
        }
    }
}

#[async_trait]
impl ReviewProvider for MockProvider {
    fn provider_name(&self) -> &str {
        "mock"
    }

    async fn health_check(&self) -> ProviderHealth {
        ProviderHealth {
            available: true,
            version: Some("mock-1.0".into()),
            message: Some("Using mock review data".into()),
        }
    }

    async fn run_review(
        &self,
        _input: &ReviewInput,
        _cwd: &Path,
        _cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        if self.fixture_path.exists() {
            let data = std::fs::read_to_string(&self.fixture_path)?;
            let output: CodexReviewOutput = serde_json::from_str(&data)?;
            return Ok(output);
        }

        // Return a default set of mock findings
        Ok(CodexReviewOutput {
            findings: vec![
                RawFinding {
                    title: "Authentication bypass in middleware".into(),
                    body: "The auth middleware can be bypassed by sending requests directly to the handler, skipping token validation.".into(),
                    file_path: Some("src/auth/middleware.ts".into()),
                    line_start: Some(15),
                    line_end: Some(28),
                    severity: "blocker".into(),
                    confidence: 0.92,
                    evidence: Some(vec!["Handler registered without middleware wrapper".into()]),
                    agent_type: "security".into(),
                    lane_id: None,
                    provider_name: None,
                    fix_suggestion: None,
                },
                RawFinding {
                    title: "N+1 query in user list endpoint".into(),
                    body: "Each user in the list triggers a separate database query to fetch their role. Use a JOIN or batch query instead.".into(),
                    file_path: Some("src/api/users.ts".into()),
                    line_start: Some(42),
                    line_end: Some(55),
                    severity: "warning".into(),
                    confidence: 0.85,
                    evidence: Some(vec!["db.query called inside for loop at line 48".into()]),
                    agent_type: "performance".into(),
                    lane_id: None,
                    provider_name: None,
                    fix_suggestion: None,
                },
                RawFinding {
                    title: "Direct dependency on internal module".into(),
                    body: "The API layer directly imports from the database layer, bypassing the service boundary.".into(),
                    file_path: Some("src/api/routes.ts".into()),
                    line_start: Some(3),
                    line_end: Some(3),
                    severity: "info".into(),
                    confidence: 0.7,
                    evidence: None,
                    agent_type: "architecture".into(),
                    lane_id: None,
                    provider_name: None,
                    fix_suggestion: None,
                },
            ],
            overall_assessment: Some("The PR introduces a security risk that should be addressed before merging. Performance and architecture findings are lower priority.".into()),
            overall_confidence: Some(0.85),
            provider_session_id: None,
            resume_cursor: None,
            checkpoint_metadata_json: None,
            cost_usd: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::prompts;

    #[test]
    fn test_build_exec_args_uses_output_schema_flag() {
        // Regression guard: codex-cli renamed --output-schema-file to
        // --output-schema; a stale flag fails as an opaque broken pipe.
        let args = build_exec_args(Some("gpt-5.2-codex"), "/tmp/schema.json");
        assert!(args.contains(&"--output-schema".to_string()));
        assert!(!args.iter().any(|a| a == "--output-schema-file"));
        assert_eq!(args.first().map(String::as_str), Some("exec"));
        assert_eq!(args.last().map(String::as_str), Some("-"));
        let model_pos = args.iter().position(|a| a == "--model").unwrap();
        assert_eq!(args[model_pos + 1], "gpt-5.2-codex");
    }

    #[test]
    fn test_build_exec_args_omits_model_when_none() {
        // No --model means codex uses the user's configured default, which is
        // required for ChatGPT-subscription accounts.
        let args = build_exec_args(None, "/tmp/schema.json");
        assert!(!args.iter().any(|a| a == "--model"));
        assert!(args.contains(&"--output-schema".to_string()));
    }

    #[test]
    fn test_prompt_concatenation() {
        let input =
            prompts::build_review_input(prompts::AgentFocus::General, &"x".repeat(600_000), None);
        let prompt = format!("{}\n\nPR Diff:\n{}", input.system_prompt, input.diff);
        assert!(prompt.len() > 600_000);
        assert!(prompt.contains("code reviewer"));
    }

    #[tokio::test]
    async fn test_mock_provider_default_fixture() {
        let provider = MockProvider::with_default_fixture();
        let health = provider.health_check().await;
        assert!(health.available);
        assert_eq!(provider.provider_name(), "mock");

        let input = prompts::build_review_input(prompts::AgentFocus::General, "some diff", None);
        let result = provider
            .run_review(&input, Path::new("/tmp"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.findings.len(), 3);
        assert_eq!(result.findings[0].severity, "blocker");
        assert_eq!(result.findings[0].agent_type, "security");
    }

    #[tokio::test]
    async fn test_mock_provider_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let fixture = tmp.path().join("fixture.json");
        std::fs::write(
            &fixture,
            r#"{"findings": [{"title": "Test", "body": "Body", "severity": "warning", "confidence": 0.5, "agent_type": "security"}]}"#,
        )
        .unwrap();

        let provider = MockProvider::new(fixture);
        let input = prompts::build_review_input(prompts::AgentFocus::Security, "diff", None);
        let result = provider
            .run_review(&input, Path::new("/tmp"), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(result.findings.len(), 1);
        assert_eq!(result.findings[0].title, "Test");
    }
}
