use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio_util::sync::CancellationToken;

use crate::errors::ProviderError;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderHealth {
    pub available: bool,
    pub version: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexReviewOutput {
    pub findings: Vec<RawFinding>,
    pub overall_assessment: Option<String>,
    pub overall_confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawFinding {
    pub title: String,
    pub body: String,
    pub file_path: Option<String>,
    pub line_start: Option<i32>,
    pub line_end: Option<i32>,
    pub severity: String,
    pub confidence: f64,
    pub evidence: Option<Vec<String>>,
    pub agent_type: String,
}

#[async_trait]
pub trait ReviewProvider: Send + Sync {
    async fn health_check(&self) -> ProviderHealth;
    async fn run_review(
        &self,
        diff: &str,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError>;
}
