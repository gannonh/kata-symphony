use async_trait::async_trait;

use crate::error::Result;
use crate::spec::pipeline::{SpecTurnExecutor, SpecTurnOutcome, SpecTurnRequest};
use crate::triage::runner::{run_isolated_spec_turn, IsolatedSpecRunnerConfig};

/// Production A2 executor backed by A1's credential-cleared, clone-only runner.
#[async_trait]
impl SpecTurnExecutor for IsolatedSpecRunnerConfig {
    async fn execute(&self, request: SpecTurnRequest) -> Result<SpecTurnOutcome> {
        run_isolated_spec_turn(self, request).await
    }
}
