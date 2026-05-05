use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::providers::traits::RawFinding;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalChecksSummary {
    pub total_errors: usize,
    pub included_count: usize,
    pub tools_run: Vec<String>,
    pub items: Vec<LocalCheckItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalCheckItem {
    pub tool: String,
    pub file: String,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub severity: String,
    pub message: String,
    pub rule_id: Option<String>,
}

pub struct LocalChecksRunner<'a> {
    workspace_path: &'a Path,
    changed_files: &'a [String],
    enable_oxlint: bool,
    enable_clippy: bool,
    cancel: CancellationToken,
}

impl<'a> LocalChecksRunner<'a> {
    pub fn new(
        workspace_path: &'a Path,
        changed_files: &'a [String],
        cancel: CancellationToken,
    ) -> Self {
        Self {
            workspace_path,
            changed_files,
            enable_oxlint: false,
            enable_clippy: false,
            cancel,
        }
    }

    pub fn with_config(mut self, config: Option<&crate::config::LocalChecksRepoConfig>) -> Self {
        if let Some(cfg) = config {
            if let Some(false) = cfg.enabled {
                self.enable_oxlint = false;
                self.enable_clippy = false;
                return self;
            }
            if let Some(v) = cfg.oxlint {
                self.enable_oxlint = v;
            }
            if let Some(v) = cfg.clippy {
                self.enable_clippy = v;
            }
        }
        self
    }

    pub async fn run(&self) -> LocalChecksSummary {
        let mut items = Vec::new();
        let mut tools_run = Vec::new();
        let changed: HashSet<&str> = self.changed_files.iter().map(|s| s.as_str()).collect();

        if self.enable_oxlint && self.has_ts_files() {
            tools_run.push("oxlint".to_string());
            match self.run_oxlint().await {
                Ok(results) => {
                    let filtered = filter_to_changed(&results, &changed);
                    items.extend(filtered);
                }
                Err(e) => {
                    tracing::warn!("oxlint failed (fail-open): {}", e);
                }
            }
        }

        if self.enable_clippy && self.has_rs_files() {
            tools_run.push("clippy".to_string());
            match self.run_clippy().await {
                Ok(results) => {
                    let filtered = filter_to_changed(&results, &changed);
                    items.extend(filtered);
                }
                Err(e) => {
                    tracing::warn!("cargo clippy failed (fail-open): {}", e);
                }
            }
        }

        let total_errors = items.len();
        let included_count = items.len();

        LocalChecksSummary {
            total_errors,
            included_count,
            tools_run,
            items,
        }
    }

    async fn run_oxlint(&self) -> Result<Vec<LocalCheckItem>, String> {
        let ts_files: Vec<&str> = self
            .changed_files
            .iter()
            .filter(|f| is_ts_file(f))
            .map(|s| s.as_str())
            .collect();

        if ts_files.is_empty() {
            return Ok(Vec::new());
        }

        let bin = resolve_oxlint_binary(self.workspace_path);
        let mut cmd = tokio::process::Command::new(&bin);
        cmd.arg("--format")
            .arg("json")
            .args(&ts_files)
            .current_dir(self.workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        run_with_timeout(&mut cmd, self.cancel.clone(), "oxlint")
            .await
            .and_then(|stdout| parse_oxlint_json(&stdout))
    }

    async fn run_clippy(&self) -> Result<Vec<LocalCheckItem>, String> {
        let mut cmd = tokio::process::Command::new("cargo");
        cmd.arg("clippy")
            .arg("--message-format=json")
            .arg("--quiet")
            .current_dir(self.workspace_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        run_with_timeout(&mut cmd, self.cancel.clone(), "clippy")
            .await
            .and_then(|stdout| parse_clippy_json(&stdout))
    }

    fn has_ts_files(&self) -> bool {
        self.changed_files.iter().any(|f| is_ts_file(f))
    }

    fn has_rs_files(&self) -> bool {
        self.changed_files.iter().any(|f| f.ends_with(".rs"))
    }
}

/// Resolve `oxlint` binary: prefer workspace `node_modules/.bin/oxlint`, fall back to PATH.
fn resolve_oxlint_binary(workspace: &Path) -> String {
    let local = workspace.join("node_modules/.bin/oxlint");
    if local.exists() {
        return local.display().to_string();
    }
    "oxlint".to_string()
}

/// Run a command with a timeout and cancellation support.
async fn run_with_timeout(
    cmd: &mut tokio::process::Command,
    cancel: CancellationToken,
    tool_name: &str,
) -> Result<String, String> {
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{tool_name} spawn failed: {e}"))?;

    tokio::select! {
        result = tokio::time::timeout(DEFAULT_TIMEOUT, child.wait()) => {
            match result {
                Ok(Ok(_status)) => {
                    let stdout = child.stdout.take();
                    if let Some(mut out) = stdout {
                        let mut buf = Vec::new();
                        tokio::io::AsyncReadExt::read_to_end(&mut out, &mut buf)
                            .await
                            .map_err(|e| format!("{tool_name} stdout read: {e}"))?;
                        Ok(String::from_utf8_lossy(&buf).to_string())
                    } else {
                        Ok(String::new())
                    }
                }
                Ok(Err(e)) => Err(format!("{tool_name} execution error: {e}")),
                Err(_) => {
                    let _ = child.kill().await;
                    Err(format!("{tool_name} timed out after {}s", DEFAULT_TIMEOUT.as_secs()))
                }
            }
        }
        _ = cancel.cancelled() => {
            let _ = child.kill().await;
            Err(format!("{tool_name} cancelled"))
        }
    }
}

fn is_ts_file(f: &str) -> bool {
    f.ends_with(".ts") || f.ends_with(".tsx") || f.ends_with(".js") || f.ends_with(".jsx")
}

fn filter_to_changed(items: &[LocalCheckItem], changed: &HashSet<&str>) -> Vec<LocalCheckItem> {
    items
        .iter()
        .filter(|item| {
            changed.contains(item.file.as_str())
                || changed
                    .iter()
                    .any(|cf| item.file.ends_with(cf) || cf.ends_with(&item.file))
        })
        .cloned()
        .collect()
}

/// Parse oxlint JSON output.
/// Accepts both:
///  - top-level object with `diagnostics: [...]` (oxc ≥0.5)
///  - top-level array (older oxlint versions)
pub fn parse_oxlint_json(raw: &str) -> Result<Vec<LocalCheckItem>, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let diagnostics: Vec<serde_json::Value> = if trimmed.starts_with('{') {
        let obj: serde_json::Value =
            serde_json::from_str(trimmed).map_err(|e| format!("oxlint JSON parse error: {e}"))?;
        obj.get("diagnostics")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    } else {
        serde_json::from_str(trimmed).map_err(|e| format!("oxlint JSON parse error: {e}"))?
    };

    let mut items = Vec::new();
    for diag in &diagnostics {
        let severity_str = diag
            .get("severity")
            .and_then(|v| v.as_str())
            .unwrap_or("error");

        let message = diag
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let file = diag
            .get("filename")
            .or_else(|| diag.get("file"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let line = diag
            .get("line")
            .or_else(|| diag.pointer("/labels/0/span/start/line"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let column = diag
            .get("column")
            .or_else(|| diag.pointer("/labels/0/span/start/col"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let rule_id = diag
            .get("rule")
            .or_else(|| diag.get("code"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if !file.is_empty() {
            items.push(LocalCheckItem {
                tool: "oxlint".into(),
                file,
                line,
                column,
                severity: severity_str.to_string(),
                message,
                rule_id,
            });
        }
    }

    Ok(items)
}

/// Parse cargo clippy `--message-format=json` output.
/// Each line is a JSON object. We look for `compiler-message` entries.
pub fn parse_clippy_json(raw: &str) -> Result<Vec<LocalCheckItem>, String> {
    let mut items = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let obj: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let reason = obj.get("reason").and_then(|v| v.as_str());
        if reason != Some("compiler-message") {
            continue;
        }

        let msg = match obj.get("message") {
            Some(m) => m,
            None => continue,
        };

        let level = msg.get("level").and_then(|v| v.as_str()).unwrap_or("");

        let message_text = msg
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let code = msg
            .pointer("/code/code")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let (file, line_num, col_num) = msg
            .get("spans")
            .and_then(|spans| spans.as_array())
            .and_then(|spans| spans.first())
            .map(|span| {
                let f = span
                    .get("file_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let l = span
                    .get("line_start")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                let c = span
                    .get("column_start")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                (f, l, c)
            })
            .unwrap_or_default();

        if !file.is_empty() {
            items.push(LocalCheckItem {
                tool: "clippy".into(),
                file,
                line: line_num,
                column: col_num,
                severity: level.to_string(),
                message: message_text,
                rule_id: code,
            });
        }
    }

    Ok(items)
}

/// Convert local check items into `RawFinding`s so they go through the cleaner pipeline.
pub fn items_to_raw_findings(items: &[LocalCheckItem]) -> Vec<RawFinding> {
    items
        .iter()
        .map(|item| {
            let severity = match item.severity.as_str() {
                "error" => "critical",
                "warning" => "warning",
                "note" | "help" => "info",
                _ => "warning",
            };

            RawFinding {
                title: format!(
                    "[{}] {}",
                    item.tool,
                    item.rule_id.as_deref().unwrap_or("check")
                ),
                body: item.message.clone(),
                file_path: Some(item.file.clone()),
                line_start: item.line.map(|l| l as i32),
                line_end: item.line.map(|l| l as i32),
                severity: severity.to_string(),
                confidence: 1.0,
                evidence: None,
                agent_type: "local_checks".to_string(),
                lane_id: Some("local_checks".to_string()),
                provider_name: Some(item.tool.clone()),
                fix_suggestion: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_oxlint_empty() {
        let result = parse_oxlint_json("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_oxlint_array_format() {
        let json = r#"[
            {"severity": "error", "message": "Unused variable", "filename": "src/app.ts", "line": 10, "column": 5, "rule": "no-unused-vars"},
            {"severity": "warning", "message": "Some warning", "filename": "src/app.ts", "line": 20, "column": 1, "rule": "semi"}
        ]"#;
        let result = parse_oxlint_json(json).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].message, "Unused variable");
        assert_eq!(result[0].severity, "error");
        assert_eq!(result[1].severity, "warning");
    }

    #[test]
    fn test_parse_oxlint_object_format() {
        let json = r#"{
            "diagnostics": [
                {"severity": "error", "message": "Unused variable", "filename": "src/app.ts", "line": 10, "column": 5, "rule": "no-unused-vars"}
            ]
        }"#;
        let result = parse_oxlint_json(json).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].message, "Unused variable");
    }

    #[test]
    fn test_parse_oxlint_object_no_diagnostics() {
        let json = r#"{"summary": "ok"}"#;
        let result = parse_oxlint_json(json).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_clippy_json() {
        let line1 = r#"{"reason":"compiler-message","message":{"level":"error","message":"unused import","code":{"code":"unused_imports"},"spans":[{"file_name":"src/main.rs","line_start":3,"column_start":5}]}}"#;
        let line2 = r#"{"reason":"compiler-message","message":{"level":"warning","message":"some warning","code":{"code":"dead_code"},"spans":[{"file_name":"src/lib.rs","line_start":10,"column_start":1}]}}"#;
        let line3 = r#"{"reason":"build-finished","success":true}"#;
        let raw = format!("{}\n{}\n{}", line1, line2, line3);

        let result = parse_clippy_json(&raw).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].tool, "clippy");
        assert_eq!(result[0].file, "src/main.rs");
        assert_eq!(result[0].severity, "error");
        assert_eq!(result[1].severity, "warning");
    }

    #[test]
    fn test_parse_clippy_empty() {
        let result = parse_clippy_json("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_clippy_malformed_lines_skipped() {
        let raw = "not json\n{\"reason\":\"build-finished\"}\n";
        let result = parse_clippy_json(raw).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_filter_to_changed() {
        let items = vec![
            LocalCheckItem {
                tool: "oxlint".into(),
                file: "src/app.ts".into(),
                line: Some(10),
                column: None,
                severity: "error".into(),
                message: "err".into(),
                rule_id: None,
            },
            LocalCheckItem {
                tool: "oxlint".into(),
                file: "src/unrelated.ts".into(),
                line: Some(5),
                column: None,
                severity: "error".into(),
                message: "err2".into(),
                rule_id: None,
            },
        ];

        let changed: HashSet<&str> = ["src/app.ts"].into_iter().collect();
        let filtered = filter_to_changed(&items, &changed);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].file, "src/app.ts");
    }

    #[test]
    fn test_items_to_raw_findings() {
        let items = vec![LocalCheckItem {
            tool: "oxlint".into(),
            file: "src/app.ts".into(),
            line: Some(10),
            column: Some(5),
            severity: "error".into(),
            message: "Unused variable".into(),
            rule_id: Some("no-unused-vars".into()),
        }];

        let findings = items_to_raw_findings(&items);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].severity, "critical");
        assert_eq!(findings[0].confidence, 1.0);
        assert_eq!(findings[0].lane_id, Some("local_checks".to_string()));
        assert_eq!(findings[0].provider_name, Some("oxlint".to_string()));
    }

    #[test]
    fn test_items_severity_mapping() {
        let make = |sev: &str| LocalCheckItem {
            tool: "test".into(),
            file: "f.ts".into(),
            line: None,
            column: None,
            severity: sev.into(),
            message: "m".into(),
            rule_id: None,
        };

        let items = vec![make("error"), make("warning"), make("note")];
        let findings = items_to_raw_findings(&items);
        assert_eq!(findings[0].severity, "critical");
        assert_eq!(findings[1].severity, "warning");
        assert_eq!(findings[2].severity, "info");
    }

    #[test]
    fn test_summary_structure() {
        let summary = LocalChecksSummary {
            total_errors: 2,
            included_count: 2,
            tools_run: vec!["oxlint".into()],
            items: vec![
                LocalCheckItem {
                    tool: "oxlint".into(),
                    file: "a.ts".into(),
                    line: Some(1),
                    column: None,
                    severity: "error".into(),
                    message: "err1".into(),
                    rule_id: None,
                },
                LocalCheckItem {
                    tool: "oxlint".into(),
                    file: "b.ts".into(),
                    line: Some(2),
                    column: None,
                    severity: "error".into(),
                    message: "err2".into(),
                    rule_id: None,
                },
            ],
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("total_errors"));
        let parsed: LocalChecksSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.total_errors, 2);
        assert_eq!(parsed.items.len(), 2);
    }

    #[test]
    fn test_default_runner_opts_in_disabled() {
        let cancel = CancellationToken::new();
        let runner = LocalChecksRunner::new(Path::new("/tmp"), &[], cancel);
        assert!(!runner.enable_oxlint);
        assert!(!runner.enable_clippy);
    }

    #[test]
    fn test_runner_config_enables() {
        let cancel = CancellationToken::new();
        let cfg = crate::config::LocalChecksRepoConfig {
            enabled: Some(true),
            oxlint: Some(true),
            clippy: Some(false),
        };
        let runner = LocalChecksRunner::new(Path::new("/tmp"), &[], cancel).with_config(Some(&cfg));
        assert!(runner.enable_oxlint);
        assert!(!runner.enable_clippy);
    }

    #[test]
    fn test_runner_config_disabled_overrides() {
        let cancel = CancellationToken::new();
        let cfg = crate::config::LocalChecksRepoConfig {
            enabled: Some(false),
            oxlint: Some(true),
            clippy: Some(true),
        };
        let runner = LocalChecksRunner::new(Path::new("/tmp"), &[], cancel).with_config(Some(&cfg));
        assert!(!runner.enable_oxlint);
        assert!(!runner.enable_clippy);
    }
}
