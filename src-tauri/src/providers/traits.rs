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

/// Structured input for review providers. Prompt construction is owned by the
/// orchestration layer (via `providers::prompts`), not by individual providers.
/// Providers are "prompt in, JSON out" executors.
#[derive(Debug, Clone)]
pub struct ReviewInput {
    pub system_prompt: String,
    pub diff: String,
    pub output_schema: String,
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
    /// Set by the orchestration layer after provider returns (not by the provider itself).
    #[serde(default)]
    pub lane_id: Option<String>,
    #[serde(default)]
    pub provider_name: Option<String>,
}

#[async_trait]
pub trait ReviewProvider: Send + Sync {
    fn provider_name(&self) -> &str;
    async fn health_check(&self) -> ProviderHealth;
    async fn run_review(
        &self,
        input: &ReviewInput,
        cwd: &Path,
        cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError>;
}
