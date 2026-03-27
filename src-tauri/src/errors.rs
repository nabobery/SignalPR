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

    #[error("Provider error: {0}")]
    Provider(#[from] ProviderError),

    #[error("Invalid input: {0}")]
    InvalidInput(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("State transition error: {0}")]
    InvalidTransition(String),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.to_string().as_str())
    }
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum ProviderError {
    #[error("Codex execution failed: {0}")]
    CodexFailed(String),

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
