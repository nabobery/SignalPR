use serde::Serialize;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("State transition error: {0}")]
    InvalidTransition(String),

    #[error("Transient error: {0}")]
    Transient(String),

    #[error("Channel error: {0}")]
    Channel(String),
}

impl AppError {
    #[allow(dead_code)]
    pub fn error_code(&self) -> &'static str {
        match self {
            AppError::Database(_) => "database_error",
            AppError::Io(_) => "io_error",
            AppError::Json(_) => "json_error",
            AppError::Http(_) => "http_error",
            AppError::Provider(_) => "provider_error",
            AppError::InvalidInput(_) => "invalid_input",
            AppError::NotFound(_) => "not_found",
            AppError::InvalidTransition(_) => "invalid_transition",
            AppError::Transient(_) => "transient_error",
            AppError::Channel(_) => "channel_error",
        }
    }

    #[allow(dead_code)]
    pub fn is_transient(&self) -> bool {
        match self {
            AppError::Transient(_) => true,
            AppError::Io(e) => io_is_transient(e),
            AppError::Http(e) => e.is_timeout() || e.is_connect(),
            AppError::Provider(e) => e.is_transient(),
            AppError::Channel(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("timeout") || msg.contains("rate") || msg.contains("retry")
            }
            _ => false,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(Some(2))?;
        map.serialize_entry("code", self.error_code())?;
        map.serialize_entry("message", &self.to_string())?;
        map.end()
    }
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum ProviderError {
    #[error("Codex execution failed: {0}")]
    CodexFailed(String),

    #[error("Claude API failed: {0}")]
    ClaudeFailed(String),

    #[error("Copilot SDK failed: {0}")]
    CopilotFailed(String),

    #[error("OpenCode failed: {0}")]
    OpenCodeFailed(String),

    #[error("Gemini failed: {0}")]
    GeminiFailed(String),

    #[error("Cursor failed: {0}")]
    CursorFailed(String),

    #[error("PI agent failed: {0}")]
    PiFailed(String),

    #[error("Claude Code failed: {0}")]
    ClaudeCodeFailed(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("GitHub CLI error: {0}")]
    GhFailed(String),

    #[error("Submission failed: {0}")]
    SubmissionFailed(String),

    #[error("Provider not available: {0}")]
    NotAvailable(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ProviderError {
    #[allow(dead_code)]
    pub fn is_transient(&self) -> bool {
        match self {
            ProviderError::Io(e) => io_is_transient(e),
            ProviderError::CodexFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
            }
            ProviderError::ClaudeFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate") || msg.contains("529") || msg.contains("500")
            }
            ProviderError::CopilotFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
            }
            ProviderError::OpenCodeFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
                    || msg.contains("503")
                    || msg.contains("connection refused")
            }
            ProviderError::GeminiFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("resource_exhausted")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
                    || msg.contains("unavailable")
            }
            ProviderError::CursorFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
                    || msg.contains("503")
                    || msg.contains("unavailable")
                    || msg.contains("connection reset")
            }
            ProviderError::PiFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
                    || msg.contains("503")
                    || msg.contains("connection refused")
            }
            ProviderError::ClaudeCodeFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate")
                    || msg.contains("429")
                    || msg.contains("overloaded")
                    || msg.contains("retry")
                    || msg.contains("timeout")
                    || msg.contains("529")
            }
            ProviderError::GhFailed(msg) => {
                let msg = msg.to_lowercase();
                msg.contains("rate") || msg.contains("429") || msg.contains("timeout")
            }
            ProviderError::Cancelled | ProviderError::NotAvailable(_) => false,
            ProviderError::SubmissionFailed(_) | ProviderError::Json(_) => false,
        }
    }
}

fn io_is_transient(e: &std::io::Error) -> bool {
    // Avoid retrying on filesystem and permission errors; focus on network-ish/transient kinds.
    matches!(
        e.kind(),
        std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::Interrupted
            | std::io::ErrorKind::WouldBlock
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_serializes_as_json_object() {
        let err = AppError::Database(rusqlite::Error::QueryReturnedNoRows);
        let json = serde_json::to_string(&err).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["code"], "database_error");
        assert!(!parsed["message"].as_str().unwrap().is_empty());
    }

    #[test]
    fn test_app_error_transient_variant() {
        let err = AppError::Transient("timeout".into());
        let json = serde_json::to_string(&err).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["code"], "transient_error");
    }

    #[test]
    fn test_transient_error_classification() {
        assert!(AppError::Transient("timeout".into()).is_transient());
        assert!(!AppError::InvalidInput("bad".into()).is_transient());
        assert!(
            AppError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout"))
                .is_transient()
        );
        assert!(AppError::Provider(ProviderError::Io(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "timeout"
        )))
        .is_transient());
    }

    #[test]
    fn test_provider_error_transient_classification() {
        assert!(
            ProviderError::Io(std::io::Error::new(std::io::ErrorKind::TimedOut, "t"))
                .is_transient()
        );
        assert!(!ProviderError::Cancelled.is_transient());
        assert!(!ProviderError::NotAvailable("x".into()).is_transient());
        assert!(ProviderError::CodexFailed("rate limit exceeded".into()).is_transient());
        assert!(!ProviderError::CodexFailed("invalid model".into()).is_transient());
        assert!(ProviderError::ClaudeFailed("529 overloaded".into()).is_transient());

        // Copilot transient checks
        assert!(ProviderError::CopilotFailed("rate limit exceeded".into()).is_transient());
        assert!(ProviderError::CopilotFailed("429 too many requests".into()).is_transient());
        assert!(ProviderError::CopilotFailed("server overloaded".into()).is_transient());
        assert!(ProviderError::CopilotFailed("please retry".into()).is_transient());
        assert!(ProviderError::CopilotFailed("request timeout".into()).is_transient());
        assert!(!ProviderError::CopilotFailed("invalid model".into()).is_transient());
        assert!(!ProviderError::CopilotFailed("auth failed".into()).is_transient());
    }

    #[test]
    fn test_gemini_failed_transient_classification() {
        assert!(ProviderError::GeminiFailed("rate limit exceeded".into()).is_transient());
        assert!(ProviderError::GeminiFailed("429 too many requests".into()).is_transient());
        assert!(ProviderError::GeminiFailed("RESOURCE_EXHAUSTED".into()).is_transient());
        assert!(ProviderError::GeminiFailed("server overloaded".into()).is_transient());
        assert!(ProviderError::GeminiFailed("request timeout".into()).is_transient());
        assert!(ProviderError::GeminiFailed("service unavailable".into()).is_transient());
        assert!(!ProviderError::GeminiFailed("invalid api key".into()).is_transient());
        assert!(!ProviderError::GeminiFailed("auth failed".into()).is_transient());
        assert!(!ProviderError::GeminiFailed("permission denied".into()).is_transient());
    }

    #[test]
    fn test_pi_failed_transient_classification() {
        assert!(ProviderError::PiFailed("rate limit exceeded".into()).is_transient());
        assert!(ProviderError::PiFailed("429 too many requests".into()).is_transient());
        assert!(ProviderError::PiFailed("server overloaded".into()).is_transient());
        assert!(ProviderError::PiFailed("please retry".into()).is_transient());
        assert!(ProviderError::PiFailed("request timeout".into()).is_transient());
        assert!(ProviderError::PiFailed("503 service unavailable".into()).is_transient());
        assert!(ProviderError::PiFailed("connection refused".into()).is_transient());
        assert!(!ProviderError::PiFailed("invalid model".into()).is_transient());
        assert!(!ProviderError::PiFailed("auth failed".into()).is_transient());
        assert!(!ProviderError::PiFailed("session not found".into()).is_transient());
    }

    #[test]
    fn test_opencode_failed_transient_classification() {
        assert!(ProviderError::OpenCodeFailed("rate limit exceeded".into()).is_transient());
        assert!(ProviderError::OpenCodeFailed("429 too many requests".into()).is_transient());
        assert!(ProviderError::OpenCodeFailed("server overloaded".into()).is_transient());
        assert!(ProviderError::OpenCodeFailed("please retry".into()).is_transient());
        assert!(ProviderError::OpenCodeFailed("request timeout".into()).is_transient());
        assert!(ProviderError::OpenCodeFailed("503 service unavailable".into()).is_transient());
        assert!(ProviderError::OpenCodeFailed("connection refused".into()).is_transient());
        assert!(!ProviderError::OpenCodeFailed("invalid model".into()).is_transient());
        assert!(!ProviderError::OpenCodeFailed("auth failed".into()).is_transient());
        assert!(!ProviderError::OpenCodeFailed("session not found".into()).is_transient());
    }
}
