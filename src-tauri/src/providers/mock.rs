use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::traits::{CodexReviewOutput, ProviderHealth, ReviewInput, ReviewProvider};
use crate::errors::ProviderError;

/// Minimal mock provider for unit tests.
pub struct MockReviewProvider;

#[async_trait]
impl ReviewProvider for MockReviewProvider {
    fn provider_name(&self) -> &str {
        "mock"
    }

    async fn health_check(&self) -> ProviderHealth {
        ProviderHealth {
            available: true,
            version: Some("mock-test".into()),
            message: None,
        }
    }

    async fn run_review(
        &self,
        _input: &ReviewInput,
        _cwd: &Path,
        _cancel: CancellationToken,
    ) -> Result<CodexReviewOutput, ProviderError> {
        Ok(CodexReviewOutput {
            findings: vec![],
            overall_assessment: None,
            overall_confidence: None,
        })
    }
}

/// Create an `Arc<dyn ReviewProvider>` for tests.
pub fn mock_provider() -> Arc<dyn ReviewProvider> {
    Arc::new(MockReviewProvider)
}
